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
    if source.lines().any(is_reference_link_definition) {
        return 0;
    }

    let mut stable_len = 0;
    let mut offset = 0;
    let mut fence = None;

    for segment in source.split_inclusive('\n') {
        let line = segment.trim_end_matches(['\n', '\r']);
        let trimmed = line.trim_start();
        if let Some((marker, marker_len)) = fence {
            if is_closing_fence(trimmed, marker, marker_len) {
                fence = None;
                stable_len = offset + segment.len();
            }
        } else if let Some(next_fence) = opening_fence(trimmed) {
            fence = Some(next_fence);
        } else {
            stable_len = offset + segment.len();
        }
        offset += segment.len();
    }

    table_holdback_start(&source[..stable_len]).unwrap_or(stable_len)
}

fn opening_fence(line: &str) -> Option<(char, usize)> {
    let marker = line.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let marker_len = line
        .chars()
        .take_while(|character| *character == marker)
        .count();
    (marker_len >= 3).then_some((marker, marker_len))
}

fn is_closing_fence(line: &str, marker: char, marker_len: usize) -> bool {
    let count = line
        .chars()
        .take_while(|character| *character == marker)
        .count();
    count >= marker_len && line.chars().skip(count).all(char::is_whitespace)
}

fn is_reference_link_definition(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix('[') else {
        return false;
    };
    let Some(label_end) = rest.find("]:") else {
        return false;
    };
    label_end > 0 && !rest[label_end + 2..].trim().is_empty()
}

fn table_holdback_start(source: &str) -> Option<usize> {
    let mut cursor = source.len();
    if source[..cursor].ends_with('\n') {
        cursor = cursor.saturating_sub(1);
    }
    let mut block_start = cursor;
    let mut block_has_separator = false;
    while cursor > 0 {
        let line_start = source[..cursor].rfind('\n').map_or(0, |index| index + 1);
        let line = source[line_start..cursor].trim();
        if !line.contains('|') {
            break;
        }
        block_start = line_start;
        block_has_separator |= is_table_separator(line);
        cursor = line_start.saturating_sub(1);
    }
    block_has_separator.then_some(block_start)
}

fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim_matches('|').trim();
    !trimmed.is_empty()
        && trimmed.split('|').all(|cell| {
            let cell = cell.trim();
            cell.len() >= 3
                && cell.starts_with('-')
                && cell.ends_with('-')
                && cell
                    .chars()
                    .all(|character| matches!(character, '-' | ':' | ' '))
        })
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
    fn commit_tick_materializes_stable_and_tail_then_finalizes_one_cell() {
        let mut widget = ChatWidget::new();
        widget.handle_runtime_event(RuntimeEvent::MessageDelta {
            message_id: "message-1".to_owned(),
            delta: "intro\n```rust\nlet answer = 42;\n".to_owned(),
        });
        widget.tick();
        assert_eq!(widget.active_cells().len(), 2);
        assert!(
            widget.active_cells()[0]
                .display_lines(80)
                .iter()
                .any(|line| line.spans.iter().any(|span| span.content.contains("intro")))
        );
        assert!(
            widget.active_cells()[1]
                .display_lines(80)
                .iter()
                .any(|line| line
                    .spans
                    .iter()
                    .any(|span| span.content.contains("answer")))
        );

        widget.handle_runtime_event(RuntimeEvent::MessageDelta {
            message_id: "message-1".to_owned(),
            delta: "```\nfinal\n".to_owned(),
        });
        widget.tick();
        assert_eq!(widget.active_cells().len(), 1);

        widget.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-1".to_owned(),
            content: "intro\n```rust\nlet answer = 42;\n```\nfinal\n".to_owned(),
        });
        assert!(widget.active_cells().is_empty());
        assert_eq!(widget.cells().len(), 1);
    }

    #[test]
    fn snapshots_materialized_stable_and_tail_stream() {
        let mut widget = ChatWidget::new();
        widget.handle_runtime_event(RuntimeEvent::MessageDelta {
            message_id: "snapshot-stream".to_owned(),
            delta: "intro\n```rust\nlet answer = 42;\n".to_owned(),
        });
        widget.tick();
        let rendered = widget
            .active_cells()
            .iter()
            .flat_map(|cell| cell.display_lines(40))
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!("streaming_materialized_stable_tail", rendered);
    }

    #[test]
    fn completion_compacts_stable_and_tail_even_without_a_closing_delta() {
        let mut widget = ChatWidget::new();
        widget.handle_runtime_event(RuntimeEvent::MessageDelta {
            message_id: "message-2".to_owned(),
            delta: "intro\n```rust\nlet answer = 42;\n".to_owned(),
        });
        widget.tick();
        assert_eq!(widget.active_cells().len(), 2);

        widget.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-2".to_owned(),
            content: "intro\n```rust\nlet answer = 42;\n```".to_owned(),
        });
        assert!(widget.active_cells().is_empty());
        assert_eq!(widget.cells().len(), 1);
        assert!(widget.cells()[0].display_lines(80).iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content.contains("answer"))
        }));
    }

    #[test]
    fn reference_definitions_keep_prior_links_mutable() {
        let mut state = MarkdownStreamState::default();
        state.push("[Atlas][home]\n\n[home]: https://atlas.invalid\n");

        assert_eq!(state.stable_source(), "");
        assert_eq!(state.tail_source(), state.source());
    }

    #[test]
    fn inline_backticks_do_not_open_a_streaming_fence() {
        let mut state = MarkdownStreamState::default();
        state.push("before\nUse ```inline``` without a block.\n");

        assert_eq!(state.stable_source(), state.source());
        assert_eq!(state.tail_source(), "");
    }

    #[test]
    fn tilde_fences_hold_back_until_the_matching_fence_closes() {
        let mut state = MarkdownStreamState::default();
        state.push("before\n~~~text\ninside\n");
        assert_eq!(state.stable_source(), "before\n");

        state.push("~~~\nafter\n");
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

    #[test]
    fn pipe_text_without_separator_does_not_hold_back_the_tail() {
        let mut state = MarkdownStreamState::default();
        state.push("before\nUse `a | b` when needed.\n");

        assert_eq!(state.stable_source(), state.source());
        assert_eq!(state.tail_source(), "");
    }
}
