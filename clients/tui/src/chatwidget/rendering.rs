use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;

use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::app::App;
use crate::bottom_pane::BottomPaneView;
use crate::history_cell::HistoryCell;
use crate::render::renderable::Renderable;
use crate::ui_consts::CONVERSATION_HORIZONTAL_INSET;
use crate::ui_consts::surface_style;

pub(crate) fn render(
    area: Rect,
    buffer: &mut Buffer,
    app: &mut App,
    bottom_pane: &dyn BottomPaneView,
) -> Option<(u16, u16)> {
    if area.is_empty() {
        return None;
    }
    buffer.set_style(area, surface_style());
    let conversation_x = area.x.saturating_add(CONVERSATION_HORIZONTAL_INSET);
    let conversation_width = area
        .width
        .saturating_sub(CONVERSATION_HORIZONTAL_INSET.saturating_mul(2));
    let conversation_area = Rect::new(conversation_x, area.y, conversation_width, area.height);
    let composer_height = bottom_pane
        .desired_height(app, conversation_area.width)
        .min(conversation_area.height.saturating_sub(1));
    let [history_area, composer_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(composer_height)])
            .areas(conversation_area);

    app.set_stream_width(history_area.width);
    render_history(buffer, app, history_area);
    if app.transcript_open() {
        app.transcript_overlay().render(app, area, buffer);
        return None;
    }
    bottom_pane.render(app, composer_area, buffer);
    bottom_pane.cursor_pos(app, composer_area)
}

fn render_history(buffer: &mut Buffer, app: &mut App, area: Rect) {
    if area.is_empty() {
        app.set_thought_hit_regions(Vec::new());
        return;
    }
    let width = area.width.max(1);
    let mut cells = Vec::new();
    let mut thought_regions = Vec::new();
    let mut total_height = 0usize;
    let history_count = app.cells().len();
    for (cell_index, cell) in app
        .cells()
        .iter()
        .chain(app.active_cells().iter())
        .enumerate()
    {
        let rendered = TranscriptAreaRenderable {
            child: cell.as_ref(),
            top: if cells.is_empty() || cell.is_stream_continuation() {
                0
            } else {
                1
            },
        };
        let height = usize::from(rendered.desired_height(width));
        cells.push((
            total_height,
            height,
            rendered,
            cell.as_any().is::<crate::history_cell::ThoughtCell>(),
            cell_index >= history_count,
            cell_index,
        ));
        total_height = total_height.saturating_add(height);
    }
    Clear.render(area, buffer);
    buffer.set_style(area, surface_style());
    let max_scroll = total_height.saturating_sub(usize::from(area.height));
    let scroll = max_scroll
        .saturating_sub(app.history_scroll())
        .min(max_scroll);
    for (start, height, rendered, thought, active, cell_index) in cells {
        let start = start as isize - scroll as isize;
        let end = start.saturating_add(height as isize);
        let viewport_height = isize::try_from(area.height).unwrap_or(isize::MAX);
        if end <= 0 || start >= viewport_height {
            continue;
        }
        let skip = usize::try_from(start.saturating_neg()).unwrap_or(0);
        let y = area
            .y
            .saturating_add(u16::try_from(start.max(0)).unwrap_or(0));
        let visible_height = usize::try_from(end.min(viewport_height).saturating_sub(start.max(0)))
            .unwrap_or(0)
            .min(usize::from(area.height));
        if visible_height == 0 {
            continue;
        }
        let cell_area = Rect::new(
            area.x,
            y,
            area.width,
            u16::try_from(visible_height).unwrap_or(u16::MAX),
        );
        if thought {
            thought_regions.push((
                cell_area.y,
                cell_area.y.saturating_add(cell_area.height),
                active,
                cell_index,
            ));
        }
        if skip == 0 || !rendered.render_scrolled(cell_area, buffer, skip as u16) {
            rendered.render(cell_area, buffer);
        }
    }
    app.set_thought_hit_regions(thought_regions);
    app.record_history_viewport_height(area.height);
    app.record_history_content_height(total_height);
}

struct TranscriptAreaRenderable<'a> {
    child: &'a dyn HistoryCell,
    top: u16,
}

impl Renderable for TranscriptAreaRenderable<'_> {
    fn render(&self, area: Rect, buffer: &mut Buffer) {
        let child_area = self.child_area(area);
        if child_area.is_empty() {
            return;
        }
        if let Some(style) = self.child.background_style() {
            buffer.set_style(child_area, style);
        }
        Paragraph::new(self.child.display_lines(child_area.width)).render(child_area, buffer);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.top
            .saturating_add(self.child.desired_height(width.max(1)))
    }

    fn render_scrolled(&self, area: Rect, buffer: &mut Buffer, scroll_offset: u16) -> bool {
        let top_visible = self.top.saturating_sub(scroll_offset);
        let child_offset = scroll_offset.saturating_sub(self.top);
        let child_area = Rect::new(
            area.x,
            area.y.saturating_add(top_visible),
            area.width.max(1),
            area.height.saturating_sub(top_visible),
        );
        if child_area.is_empty() {
            return true;
        }
        if let Some(style) = self.child.background_style() {
            buffer.set_style(child_area, style);
        }
        Paragraph::new(self.child.display_lines(child_area.width))
            .scroll((child_offset, 0))
            .render(child_area, buffer);
        true
    }
}

impl TranscriptAreaRenderable<'_> {
    fn child_area(&self, area: Rect) -> Rect {
        Rect::new(
            area.x,
            area.y.saturating_add(self.top),
            area.width.max(1),
            area.height.saturating_sub(self.top),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::TranscriptAreaRenderable;
    use crate::app::App;
    use crate::bottom_pane::bottom_pane_view::ChatComposerView;
    use crate::history_cell::HistoryCell;
    use crate::history_cell::UserHistoryCell;
    use crate::render::renderable::Renderable;
    use crate::runtime::RuntimeEvent;
    use crate::ui_consts::COLOR_SURFACE_USER;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::text::Line;
    use ratatui::text::Span;

    /// Fundo determinista: o caminho exibido nao depende do ambiente do runner.
    fn snapshot_app(conversation_id: &str) -> App {
        let mut app = App::new(conversation_id.to_owned());
        app.set_display_paths("/home/kyle/projetos/Atlas", "/home/kyle");
        app
    }

    #[derive(Debug)]
    struct TestCell;

    impl HistoryCell for TestCell {
        fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
            vec![Line::from(Span::raw("123456789"))]
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    fn screen_rows(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let area = Rect::new(0, 0, width, height);
        let mut buffer = Buffer::empty(area);
        super::render(area, &mut buffer, app, &ChatComposerView);
        buffer
            .content
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect()
    }

    #[test]
    fn renders_active_shell_exec_and_working_composer() {
        let mut app = App::new("render-test".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        app.handle_runtime_event(RuntimeEvent::ExecutionStarted {
            execution_id: "exec-1".to_owned(),
            capability: "shell.exec".to_owned(),
            program: "sh".to_owned(),
            args: vec!["-c".to_owned(), "node --version".to_owned()],
            cwd: Some("/tmp".to_owned()),
            target: Some("local".to_owned()),
        });

        let area = Rect::new(0, 0, 100, 37);
        let mut buffer = Buffer::empty(area);
        super::render(area, &mut buffer, &mut app, &ChatComposerView);
        let screen = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(screen.contains("Running node --version"));
        assert!(screen.contains("Running node --version"));
        assert!(screen.contains("Ask Atlas to do anything"));
    }

    #[test]
    fn groups_concurrent_executions_into_one_compact_card() {
        let mut app = App::new("render-group".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        for (id, command) in [
            ("exec-1", "npm test"),
            ("exec-2", "npm run lint"),
            ("exec-3", "npm run typecheck"),
        ] {
            app.handle_runtime_event(RuntimeEvent::ExecutionStarted {
                execution_id: id.to_owned(),
                capability: "shell.exec".to_owned(),
                program: "sh".to_owned(),
                args: vec!["-c".to_owned(), command.to_owned()],
                cwd: None,
                target: None,
            });
        }

        let screen = screen_rows(&mut app, 100, 37).join("\n");
        assert!(screen.contains("• Running 3 commands"));
        assert!(screen.contains("npm test"));
        assert!(screen.contains("npm run lint"));
        assert!(screen.contains("npm run typecheck"));
        assert!(!screen.contains("• Running npm test"));
    }

    #[test]
    fn keeps_completed_parallel_executions_in_one_history_card() {
        let mut app = App::new("render-group-complete".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        for (id, command) in [("exec-1", "npm test"), ("exec-2", "npm run lint")] {
            app.handle_runtime_event(RuntimeEvent::ExecutionStarted {
                execution_id: id.to_owned(),
                capability: "shell.exec".to_owned(),
                program: "sh".to_owned(),
                args: vec!["-c".to_owned(), command.to_owned()],
                cwd: None,
                target: None,
            });
        }
        for id in ["exec-1", "exec-2"] {
            app.handle_runtime_event(RuntimeEvent::ExecutionCompleted {
                execution_id: id.to_owned(),
                capability: "shell.exec".to_owned(),
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                duration_ms: 42,
                status: "success".to_owned(),
            });
        }

        let screen = screen_rows(&mut app, 100, 37).join("\n");
        assert!(screen.contains("• Ran 2 commands"));
        assert_eq!(screen.matches("commands").count(), 1);
        assert!(!screen.contains("Running 2 commands"));
        assert!(screen.contains("npm run lint"));
    }

    #[test]
    fn renders_terminal_error_without_stale_working_status() {
        let mut app = App::new("render-error".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        app.handle_runtime_event(RuntimeEvent::Error {
            message: "provider unavailable".to_owned(),
        });

        let screen = screen_rows(&mut app, 80, 24).join("\n");
        assert!(screen.contains("! provider unavailable"));
        assert!(!screen.contains("Thinking ("));
    }

    #[test]
    fn keeps_long_streaming_output_inside_the_history_viewport() {
        let mut app = App::new("render-stream".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        for line in 0..40 {
            app.handle_runtime_event(RuntimeEvent::MessageDelta {
                message_id: "assistant-1".to_owned(),
                delta: format!("stream line {line}\n"),
            });
        }

        let rows = screen_rows(&mut app, 80, 24);
        assert_eq!(rows.len(), 24);
        assert!(rows.iter().any(|row| row.contains("stream line 39")));
        assert!(rows.iter().all(|row| row.chars().count() == 80));
    }

    #[test]
    fn renders_user_message_background_across_the_cell_width() {
        let cell = UserHistoryCell::new("hello");
        let area = Rect::new(0, 0, 20, 4);
        let mut buffer = Buffer::empty(area);
        let renderable = TranscriptAreaRenderable {
            child: &cell,
            top: 0,
        };

        renderable.render(area, &mut buffer);

        assert!(
            buffer
                .content
                .iter()
                .all(|cell| cell.bg == COLOR_SURFACE_USER)
        );
    }

    #[test]
    fn keeps_working_status_and_composer_after_resize() {
        let mut app = App::new("render-resize-active".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);

        for (width, height) in [(80, 24), (120, 40), (80, 24)] {
            let rows = screen_rows(&mut app, width, height);
            assert_eq!(rows.len(), usize::from(height));
            assert!(!rows.iter().any(|row| row.contains("Thinking")));
            assert!(
                rows.iter()
                    .any(|row| row.contains("Ask Atlas to do anything"))
            );
            assert!(
                rows.iter()
                    .all(|row| row.chars().count() == usize::from(width))
            );
        }
    }

    #[test]
    fn reserves_transcript_separator_without_a_local_right_inset() {
        let cell = TestCell;
        let renderable = TranscriptAreaRenderable {
            child: &cell,
            top: 1,
        };

        assert_eq!(renderable.desired_height(10), 2);
    }

    #[test]
    fn snapshots_narrow_terminal_priority() {
        let mut app = snapshot_app("render-narrow");
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        app.insert_text("fix the error path");

        insta::assert_snapshot!("terminal_narrow", screen_rows(&mut app, 24, 12).join("\n"));
    }

    #[test]
    fn snapshots_initial_layout() {
        let mut app = snapshot_app("render-initial");

        insta::assert_snapshot!("initial_layout", screen_rows(&mut app, 80, 12).join("\n"));
    }

    #[test]
    fn snapshots_connected_layout() {
        let mut app = snapshot_app("render-connected");
        app.handle_runtime_event(RuntimeEvent::SessionUpdated {
            model: "gpt-5.6-luna".to_owned(),
            provider: "opencode-go".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::ContextUpdated {
            context: crate::runtime::ContextUsage {
                used_tokens: 1_560,
                context_window: 156_000,
            },
        });

        insta::assert_snapshot!("connected_layout", screen_rows(&mut app, 80, 12).join("\n"));
    }

    #[test]
    fn snapshots_user_message_layout() {
        let mut app = snapshot_app("render-user-message");
        app.handle_runtime_event(RuntimeEvent::SessionUpdated {
            model: "gpt-5.6-luna".to_owned(),
            provider: "opencode-go".to_owned(),
        });
        app.insert_text("Review the auth flow");
        assert_eq!(app.submit_input().as_deref(), Some("Review the auth flow"));

        insta::assert_snapshot!(
            "user_message_layout",
            screen_rows(&mut app, 80, 12).join("\n")
        );
    }

    #[test]
    fn snapshots_turn_lifecycle_with_one_conversation_inset() {
        let mut app = snapshot_app("render-lifecycle");
        app.insert_text("question");
        assert_eq!(app.submit_input().as_deref(), Some("question"));
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        app.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "read-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
            target: Some("README.md".to_owned()),
        });
        app.handle_runtime_event(RuntimeEvent::ToolCompleted {
            tool_id: "read-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
            output: Some(
                serde_json::json!({
                    "status": "success",
                    "path": "README.md",
                    "total_lines": 42
                })
                .to_string(),
            ),
        });
        app.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "patch-1".to_owned(),
            tool_name: "filesystem.patch".to_owned(),
            target: Some("src/main.rs".to_owned()),
        });
        app.handle_runtime_event(RuntimeEvent::ToolCompleted {
            tool_id: "patch-1".to_owned(),
            tool_name: "filesystem.patch".to_owned(),
            output: Some(
                serde_json::json!({
                    "status": "success",
                    "changes": [{
                        "action": "edit",
                        "path": "src/main.rs",
                        "diff": "@@ -1,1 +1,1 @@\n-old\n+new\n"
                    }]
                })
                .to_string(),
            ),
        });
        app.handle_runtime_event(RuntimeEvent::MessageDelta {
            message_id: "answer-1".to_owned(),
            delta: "answer".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::TurnCompleted {
            content: "answer".to_owned(),
            message_id: Some("answer-1".to_owned()),
            context: None,
        });

        insta::assert_snapshot!(
            "turn_lifecycle_shared_inset",
            screen_rows(&mut app, 80, 24).join("\n")
        );
    }

    #[test]
    fn snapshots_cancelled_turn_without_error_cell_or_footer_thinking() {
        let mut app = snapshot_app("render-cancelled");
        app.insert_text("long task");
        assert_eq!(app.submit_input().as_deref(), Some("long task"));
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        app.handle_runtime_event(RuntimeEvent::MessageDelta {
            message_id: "partial-1".to_owned(),
            delta: "partial result".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::TurnCancelled {
            content: "partial result".to_owned(),
            message_id: Some("partial-1".to_owned()),
        });

        insta::assert_snapshot!("cancelled_turn", screen_rows(&mut app, 80, 16).join("\n"));
    }
}
