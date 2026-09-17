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
    stable_len: usize,
    width: Option<u16>,
    revision: u64,
}

impl MarkdownStreamState {
    fn push(&mut self, delta: &str) {
        self.source.push_str(delta);
        self.stable_len = stable_prefix_len(&self.source);
        self.revision = self.revision.wrapping_add(1);
    }

    fn source(&self) -> &str {
        &self.source
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
    fn set_width(&mut self, width: u16) -> bool {
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
    if fence_count.is_multiple_of(2) {
        prefix.len()
    } else {
        let fence_start = prefix.rfind("```").unwrap_or(0);
        source[..fence_start]
            .rfind('\n')
            .map_or(0, |index| index + 1)
    }
}

impl ChatWidget {
    pub(super) fn handle_streaming_event(&mut self, event: RuntimeEvent) {
        match event {
            RuntimeEvent::MessageDelta { message_id, delta } => {
                self.status = Status::Thinking;
                let source = {
                    let stream = self.stream_states.entry(message_id.clone()).or_default();
                    stream.push(&delta);
                    stream.source().to_owned()
                };
                if let Some(message) = self.find_active_agent_mut(&message_id) {
                    message.markdown_source = bounded_text(&source);
                } else {
                    let is_first_line = self.active_cells.is_empty();
                    self.active_cells.push(Box::new(AgentMessageCell::new(
                        message_id,
                        bounded_text(&source),
                        is_first_line,
                    )));
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
}
