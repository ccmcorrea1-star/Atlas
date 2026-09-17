//! Adapter de deltas do Runtime Atlas para o estado de streaming do transcript.

use super::*;

/// Estado local do adapter de Markdown durante um stream Atlas.
///
/// A região estável termina na última quebra de linha completa fora de uma fence aberta; o
/// restante permanece como tail mutável e é re-renderizado com cada delta. Isso mantém o protocolo
/// Atlas independente do `MarkdownStreamCollector` interno do Codex.
#[derive(Debug, Default)]
pub(super) struct MarkdownStreamState {
    source: String,
    pub(super) stable_len: usize,
    committed_len: usize,
    width: Option<u16>,
    revision: u64,
}

impl MarkdownStreamState {
    fn push(&mut self, delta: &str) {
        self.source.push_str(delta);
        self.stable_len = stable_prefix_len(&self.source);
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn source(&self) -> &str {
        &self.source
    }

    pub(super) fn commit_tick(&mut self) -> bool {
        if self.committed_len == self.stable_len {
            return false;
        }
        self.committed_len = self.stable_len;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    #[allow(dead_code)]
    fn stable_source(&self) -> &str {
        &self.source[..self.stable_len]
    }

    #[allow(dead_code)]
    fn tail_source(&self) -> &str {
        &self.source[self.stable_len..]
    }

    #[allow(dead_code)]
    pub(super) fn set_width(&mut self, width: u16) -> bool {
        let changed = self.width != Some(width);
        self.width = Some(width);
        changed
    }

    #[allow(dead_code)]
    fn revision(&self) -> u64 {
        self.revision
    }
}

fn stable_prefix_len(source: &str) -> usize {
    let Some(last_newline) = source.rfind('\n') else {
        return 0;
    };
    let prefix = &source[..=last_newline];
    let fence_count = prefix.match_indices("```").count();
    let fence_stable_len = if fence_count.is_multiple_of(2) {
        prefix.len()
    } else {
        let fence_start = prefix.rfind("```").unwrap_or(0);
        source[..fence_start]
            .rfind('\n')
            .map_or(0, |index| index + 1)
    };
    table_holdback_start(&source[..fence_stable_len]).unwrap_or(fence_stable_len)
}

fn table_holdback_start(source: &str) -> Option<usize> {
    let mut cursor = source.len();
    if source[..cursor].ends_with('\n') {
        cursor = cursor.saturating_sub(1);
    }
    let mut first_table_line = None;
    while cursor > 0 {
        let line_start = source[..cursor].rfind('\n').map_or(0, |index| index + 1);
        let line = source[line_start..cursor].trim();
        if !line.contains('|') {
            break;
        }
        first_table_line = Some(line_start);
        cursor = line_start.saturating_sub(1);
    }
    first_table_line
}

impl ChatWidget {
    pub(super) fn handle_streaming_event(&mut self, event: RuntimeEvent) {
        match event {
            RuntimeEvent::MessageDelta { message_id, delta } => {
                self.status = Status::Thinking;
                let (source, stable_len) = {
                    let stream = self.stream_states.entry(message_id.clone()).or_default();
                    stream.push(&delta);
                    (stream.source().to_owned(), stream.stable_len)
                };
                let display_source = bounded_text(&source);
                if let Some(message) = self.find_active_agent_mut(&message_id) {
                    message.set_stream_parts(&display_source, stable_len);
                } else {
                    let is_first_line = self.active_cells.is_empty();
                    let mut cell =
                        AgentMessageCell::new(message_id, display_source.clone(), is_first_line);
                    cell.set_stream_parts(&display_source, stable_len);
                    self.active_cells.push(Box::new(cell));
                }
                self.bump_active_revision();
                self.history_changed();
            }
            RuntimeEvent::MessageCompleted {
                message_id,
                content,
            } => {
                self.status = Status::Thinking;
                self.stream_states.remove(&message_id);
                if let Some(message) = self.find_active_agent_mut(&message_id) {
                    message.markdown_source = bounded_text(&content);
                    message.clear_stream_parts();
                    message.completed = true;
                    self.commit_active_agent(&message_id);
                } else if let Some(message) = self.find_agent_mut(&message_id) {
                    message.markdown_source = bounded_text(&content);
                    message.completed = true;
                } else {
                    self.cells.push(Box::new(AgentMarkdownCell::with_message_id(
                        Some(message_id),
                        bounded_text(&content),
                    )));
                }
                self.history_changed();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_region_stops_before_an_open_code_fence() {
        let mut state = MarkdownStreamState::default();
        state.push("intro\n```rust\nlet answer = 42;\n");

        assert_eq!(state.stable_source(), "intro\n");
        assert!(state.tail_source().starts_with("```rust"));
    }

    #[test]
    fn stable_region_advances_after_the_code_fence_closes() {
        let mut state = MarkdownStreamState::default();
        state.push("intro\n```rust\nlet answer = 42;\n```\nfinal\n");

        assert_eq!(state.tail_source(), "");
        assert_eq!(state.stable_source(), state.source());
    }

    #[test]
    fn commit_tick_drains_only_new_stable_text() {
        let mut state = MarkdownStreamState::default();
        state.push("one\ntwo");

        assert!(state.commit_tick());
        assert!(!state.commit_tick());
        state.push("\nthree");
        assert!(state.commit_tick());
    }

    #[test]
    fn table_rows_remain_in_the_mutable_tail_until_the_table_is_followed_by_text() {
        let mut state = MarkdownStreamState::default();
        state.push("before\n| a | b |\n|---|---|\n| 1 | 2 |\n");

        assert_eq!(state.stable_source(), "before\n");
        assert!(state.tail_source().starts_with("| a | b |"));
    }
}
