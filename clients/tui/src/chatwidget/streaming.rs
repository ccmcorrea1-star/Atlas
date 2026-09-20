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
    /// O tail comeca um bloco novo depois da regiao estavel.
    pub(super) block_break: bool,
    committed_len: usize,
    width: Option<u16>,
    revision: u64,
}

impl MarkdownStreamState {
    fn push(&mut self, delta: &str) {
        self.source.push_str(delta);
        let split = split_point(&self.source);
        self.stable_len = split.stable_len;
        self.block_break = split.block_break;
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

struct StreamSplit {
    stable_len: usize,
    block_break: bool,
}

/// Resolve o corte entre regiao estavel e tail e se o corte abre um bloco novo.
fn split_point(source: &str) -> StreamSplit {
    let metadata = crate::markdown_streaming::scan(source);
    if metadata.has_reference_link_definition {
        return StreamSplit {
            stable_len: 0,
            block_break: false,
        };
    }
    let block_break = metadata.last_top_level_block_start.is_some();
    let stable_len = metadata
        .last_top_level_block_start
        .unwrap_or(0)
        .min(metadata.pending_math_start.unwrap_or(usize::MAX));
    let stable_len = table_holdback_start(&source[..stable_len]).unwrap_or(stable_len);
    StreamSplit {
        stable_len,
        block_break: block_break && stable_len > 0,
    }
}

fn table_holdback_start(source: &str) -> Option<usize> {
    let mut fences = crate::table_detect::FenceTracker::new();
    let mut previous: Option<(usize, crate::table_detect::FenceKind, bool)> = None;
    let mut pending_header = None;
    let mut confirmed_table = None;
    let mut markdown_fence_start = None;
    let mut offset = 0;

    for segment in source.split_inclusive('\n') {
        let line = segment.trim_end_matches(['\n', '\r']);
        let fence_kind = fences.kind();
        let opens_markdown_fence =
            fence_kind == crate::table_detect::FenceKind::Outside && is_markdown_fence_start(line);
        let candidate = (fence_kind != crate::table_detect::FenceKind::Other)
            .then(|| crate::table_detect::strip_blockquote_prefix(line).trim())
            .filter(|line| crate::table_detect::parse_table_segments(line).is_some());
        let is_explicit_pipe_row =
            candidate.is_some_and(|line| line.starts_with('|') || line.ends_with('|'));
        let is_header = is_explicit_pipe_row
            && candidate.is_some_and(crate::table_detect::is_table_header_line);
        let is_delimiter = candidate.is_some_and(crate::table_detect::is_table_delimiter_line);

        if let Some((start, previous_kind, previous_header)) = previous
            && previous_kind != crate::table_detect::FenceKind::Other
            && fence_kind != crate::table_detect::FenceKind::Other
            && previous_header
            && is_delimiter
        {
            let table_start = if previous_kind == crate::table_detect::FenceKind::Markdown {
                markdown_fence_start.unwrap_or(start)
            } else {
                start
            };
            confirmed_table.get_or_insert(table_start);
            pending_header = None;
        }
        if confirmed_table.is_none() && !line.trim().is_empty() {
            pending_header = is_header.then_some(offset);
        }

        previous = Some((offset, fence_kind, is_header));
        fences.advance(line);
        if opens_markdown_fence {
            markdown_fence_start = Some(offset);
        } else if fence_kind == crate::table_detect::FenceKind::Markdown
            && fences.kind() == crate::table_detect::FenceKind::Outside
        {
            markdown_fence_start = None;
        }
        offset += segment.len();
    }

    confirmed_table.or(pending_header)
}

fn is_markdown_fence_start(line: &str) -> bool {
    let leading_spaces = line.bytes().take_while(|byte| *byte == b' ').count();
    if leading_spaces > 3 {
        return false;
    }
    let line = crate::table_detect::strip_blockquote_prefix(&line[leading_spaces..]);
    let Some((_, marker_len)) = crate::table_detect::parse_fence_marker(line) else {
        return false;
    };
    crate::table_detect::is_markdown_fence_info(line, marker_len)
}

impl ChatWidget {
    pub(super) fn handle_streaming_event(&mut self, event: RuntimeEvent) {
        match event {
            RuntimeEvent::MessageDelta { message_id, delta } => {
                self.finalize_reasoning();
                self.status = Status::Working;
                let (source, stable_len, block_break) = {
                    let stream = self.stream_states.entry(message_id.clone()).or_default();
                    stream.push(&delta);
                    (
                        stream.source().to_owned(),
                        stream.stable_len,
                        stream.block_break,
                    )
                };
                let display_source = bounded_text(&source);
                if !self.active_agent_positions(&message_id).is_empty() {
                    self.apply_stream_parts(&message_id, &display_source, stable_len, block_break);
                } else {
                    let is_first_line = self.active_cells.is_empty();
                    let mut cell =
                        AgentMessageCell::new(message_id, display_source.clone(), is_first_line);
                    cell.set_stream_parts(&display_source, stable_len, block_break);
                    self.active_cells.push(Box::new(cell));
                }
                self.bump_active_revision();
                self.history_changed();
            }
            RuntimeEvent::MessageCompleted {
                message_id,
                content,
            } => {
                self.finalize_reasoning();
                self.status = Status::Working;
                self.stream_states.remove(&message_id);
                let content = bounded_text(&content);
                if !self.active_agent_positions(&message_id).is_empty() {
                    self.set_active_agent_content(&message_id, &content);
                    if let Some(message) = self.find_active_agent_mut(&message_id) {
                        message.clear_stream_parts();
                        message.completed = true;
                    }
                    self.commit_active_agent(&message_id);
                } else if let Some(message) = self.find_agent_mut(&message_id) {
                    message.markdown_source = content;
                    message.completed = true;
                } else {
                    self.cells.push(Box::new(AgentMarkdownCell::with_message_id(
                        Some(message_id),
                        content,
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
    fn incomplete_final_line_stays_in_the_mutable_tail() {
        let mut state = MarkdownStreamState::default();
        state.push("one\ntwo");

        assert_eq!(state.stable_source(), "");
        assert_eq!(state.tail_source(), "one\ntwo");
    }

    #[test]
    fn table_holdback_ignores_pipes_inside_non_markdown_fences() {
        let mut state = MarkdownStreamState::default();
        state.push("```rust\n| not | a | table |\n```\nplain\n");

        assert_eq!(state.stable_source(), "```rust\n| not | a | table |\n```\n");
        assert_eq!(state.tail_source(), "plain\n");
    }

    #[test]
    fn table_holdback_supports_blockquote_tables() {
        let mut state = MarkdownStreamState::default();
        state.push("> | A | B |\n> | --- | --- |\n");

        assert_eq!(state.stable_source(), "");
        assert_eq!(state.tail_source(), "> | A | B |\n> | --- | --- |\n");
    }

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

        assert_eq!(
            state.stable_source(),
            "intro\n```rust\nlet answer = 42;\n```\n"
        );
        assert_eq!(state.tail_source(), "final\n");
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
        assert_eq!(widget.active_cells().len(), 2);

        widget.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-1".to_owned(),
            content: "intro\n```rust\nlet answer = 42;\n```\nfinal\n".to_owned(),
        });
        assert!(widget.active_cells().is_empty());
        assert_eq!(widget.cells().len(), 1);
    }

    fn row_text(line: &ratatui::text::Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    }

    fn rows_of(cells: &[Box<dyn HistoryCell>], width: u16) -> Vec<String> {
        cells
            .iter()
            .flat_map(|cell| cell.display_lines(width))
            .map(|line| row_text(&line))
            .collect()
    }

    fn stream_chunks(content: &str, size: usize) -> Vec<String> {
        content
            .chars()
            .collect::<Vec<_>>()
            .chunks(size)
            .map(|chunk| chunk.iter().collect::<String>())
            .collect()
    }

    fn streamed_rows(content: &str, size: usize, width: u16) -> Vec<String> {
        let mut widget = ChatWidget::new();
        for chunk in stream_chunks(content, size) {
            widget.handle_runtime_event(RuntimeEvent::MessageDelta {
                message_id: "message".to_owned(),
                delta: chunk,
            });
            widget.tick();
        }
        rows_of(widget.active_cells(), width)
    }

    fn completed_rows(content: &str, width: u16) -> Vec<String> {
        let mut widget = ChatWidget::new();
        widget.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message".to_owned(),
            content: content.to_owned(),
        });
        rows_of(widget.cells(), width)
    }

    fn reference_rows(content: &str, width: u16) -> Vec<String> {
        AgentMarkdownCell::with_message_id(None, content)
            .display_lines(width)
            .iter()
            .map(row_text)
            .collect()
    }

    fn document_rows() -> Vec<(&'static str, &'static str)> {
        vec![
            ("heading", "## Titulo\n\nTexto do paragrafo."),
            (
                "titulo em negrito",
                "**Bridge de capabilities**\n\nO bridge de capabilities funciona como uma camada de integracao.",
            ),
            (
                "dois paragrafos",
                "Primeiro paragrafo.\n\nSegundo paragrafo.",
            ),
            (
                "lista",
                "Passos:\n\n- primeiro item\n- segundo item\n- terceiro item\n\nFim.",
            ),
            (
                "code fence",
                "Antes.\n\n```rust\nlet answer = 42;\nprintln!(\"ok\");\n```\n\nDepois.",
            ),
            (
                "tabela",
                "Tabela:\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n\nFim.",
            ),
        ]
    }

    #[test]
    fn streaming_keeps_the_same_structure_as_the_completed_message() {
        for (label, content) in document_rows() {
            for size in [3usize, 7, 40] {
                let streamed = streamed_rows(content, size, 60);
                let completed = completed_rows(content, 60);
                assert_eq!(
                    streamed, completed,
                    "{label}: stream de {size} chars divergiu da mensagem concluida"
                );
                assert_eq!(
                    completed,
                    reference_rows(content, 60),
                    "{label}: conteudo concluido divergiu do conteudo recebido"
                );
            }
        }
    }

    #[test]
    fn stable_region_and_tail_keep_the_blank_line_between_blocks() {
        let mut widget = ChatWidget::new();
        widget.handle_runtime_event(RuntimeEvent::MessageDelta {
            message_id: "message".to_owned(),
            delta: "intro\n\ncodigo\n\n".to_owned(),
        });
        widget.tick();
        widget.handle_runtime_event(RuntimeEvent::MessageDelta {
            message_id: "message".to_owned(),
            delta: "```rust\nlet answer = 42;\n".to_owned(),
        });
        widget.tick();

        let rendered = rows_of(widget.active_cells(), 40);
        assert_eq!(rendered[0], "intro");
        assert_eq!(rendered[1], "  ");
        assert!(
            rendered.iter().any(|row| row.contains("let answer = 42;")),
            "a fence do tail deve aparecer depois do separador: {rendered:?}"
        );
    }

    #[test]
    fn materialization_does_not_repeat_the_stable_region_in_the_tail() {
        let content = "**Bridge de capabilities**\n\nO bridge de capabilities funciona.";
        let mut widget = ChatWidget::new();
        for chunk in stream_chunks(content, 6) {
            widget.handle_runtime_event(RuntimeEvent::MessageDelta {
                message_id: "message".to_owned(),
                delta: chunk,
            });
            widget.tick();

            let rendered = rows_of(widget.active_cells(), 80);
            let heads = rendered
                .iter()
                .filter(|row| row.contains("Bridge de capabilities"))
                .filter(|row| !row.contains("O bridge"))
                .count();
            assert!(
                heads <= 1,
                "o titulo nao pode aparecer duas vezes durante o stream: {rendered:?}"
            );
            assert!(
                !rendered.iter().any(|row| row.contains("capabilitiesO")),
                "blocos nao podem ser concatenados sem separador: {rendered:?}"
            );
        }
    }

    #[test]
    fn final_completion_after_many_deltas_matches_the_received_content() {
        let content = "## Resumo\n\nPrimeiro paragrafo.\n\nSegundo paragrafo.\n\n- item\n- outro\n";
        let mut widget = ChatWidget::new();
        for chunk in stream_chunks(content, 2) {
            widget.handle_runtime_event(RuntimeEvent::MessageDelta {
                message_id: "message".to_owned(),
                delta: chunk,
            });
            widget.tick();
        }
        widget.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message".to_owned(),
            content: content.to_owned(),
        });

        assert!(widget.active_cells().is_empty());
        assert_eq!(rows_of(widget.cells(), 60), reference_rows(content, 60));
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

        assert_eq!(state.stable_source(), "");
        assert_eq!(state.tail_source(), state.source());
    }

    #[test]
    fn tilde_fences_hold_back_until_the_matching_fence_closes() {
        let mut state = MarkdownStreamState::default();
        state.push("before\n~~~text\ninside\n");
        assert_eq!(state.stable_source(), "before\n");

        state.push("~~~\nafter\n");
        assert_eq!(state.stable_source(), "before\n~~~text\ninside\n~~~\n");
        assert_eq!(state.tail_source(), "after\n");
    }

    #[test]
    fn commit_tick_drains_only_new_stable_text() {
        let mut state = MarkdownStreamState::default();
        state.push("one\ntwo");

        assert!(!state.commit_tick());
        assert!(!state.commit_tick());
        state.push("\n\nthree");
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
    fn markdown_fence_tables_keep_the_whole_fence_in_the_mutable_tail() {
        let mut state = MarkdownStreamState::default();
        state.push("before\n```md\n| a | b |\n| --- | --- |\n| 1 | 2 |\n```\nafter\n");

        assert_eq!(state.stable_source(), "before\n");
        assert!(state.tail_source().starts_with("```md\n| a | b |"));
    }

    #[test]
    fn pipe_text_without_separator_does_not_hold_back_the_tail() {
        let mut state = MarkdownStreamState::default();
        state.push("before\nUse `a | b` when needed.\n");

        assert_eq!(state.stable_source(), "");
        assert_eq!(state.tail_source(), state.source());
    }

    #[test]
    fn incomplete_display_math_stays_in_the_mutable_tail() {
        let mut state = MarkdownStreamState::default();
        state.push("intro\n\n$$\nx^2\n");

        assert_eq!(state.stable_source(), "intro\n\n");
        assert_eq!(state.tail_source(), "$$\nx^2\n");
    }
}
