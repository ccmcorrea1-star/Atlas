use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};

use crate::app::App;
use crate::composer::ComposerPanel;
use crate::status::{ShortcutsOverlay, StatusBar};
use crate::transcript::ChatPanel;

pub fn draw(frame: &mut Frame<'_>, app: &mut App) {
    let composer_height = ComposerPanel::height(app, frame.area().width);
    let [transcript_area, composer_area, status_area] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(composer_height),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    ChatPanel::draw(frame, app, transcript_area);
    ComposerPanel::draw(frame, app, composer_area);
    StatusBar::draw(frame, app, status_area);
    if app.shortcuts_open() {
        ShortcutsOverlay::draw(frame, frame.area());
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::draw;
    use crate::app::App;
    use crate::runtime::{ContextUsage, RuntimeEvent};

    fn terminal_rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|column| buffer[(column, row)].symbol().to_owned())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    fn render_fixture(mut app: App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal.draw(|frame| draw(frame, &mut app)).expect("draw");
        terminal_rows(&terminal)
    }

    fn with_context(app: &mut App) {
        app.set_context_usage(ContextUsage {
            used_tokens: 6_600,
            context_window: 256_000,
        });
    }

    #[test]
    fn renders_user_and_agent_cells_like_codex() {
        let mut app = App::new("conversation".to_owned());
        app.insert_character('h');
        app.insert_character('i');
        assert_eq!(app.submit_input().as_deref(), Some("hi"));
        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-1".to_owned(),
            content: "Resposta **normal** com `markdown`.".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::TurnCompleted { context: None });
        let rows = render_fixture(app, 80, 10);

        assert!(rows.iter().any(|row| row.contains("› hi")));
        assert!(
            rows.iter()
                .any(|row| row.contains("  │ Resposta normal com markdown."))
        );
        assert!(!rows.iter().any(|row| row.contains("Atlas:")));
    }

    #[test]
    fn renders_fenced_cpp_as_code_without_an_empty_agent_marker() {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-1".to_owned(),
            content: "```cpp\nint main() { return 0; }\n```".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::TurnCompleted { context: None });
        let rows = render_fixture(app, 80, 10);

        assert!(rows.iter().any(|row| row.contains("┌─ cpp")));
        assert!(rows.iter().any(|row| row.contains("int main()")));
        assert!(rows.iter().any(|row| row.contains("└─")));
    }

    #[test]
    fn renders_process_exec_completion_without_capability_label() {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::ExecutionStarted {
            execution_id: "tool-1".to_owned(),
            capability: "process.exec".to_owned(),
            program: "printf".to_owned(),
            args: vec!["hello".to_owned()],
            cwd: None,
            target: Some("local".to_owned()),
        });
        app.handle_runtime_event(RuntimeEvent::ExecutionCompleted {
            execution_id: "tool-1".to_owned(),
            capability: "process.exec".to_owned(),
            stdout: "hello".to_owned(),
            stderr: String::new(),
            exit_code: 0,
            duration_ms: 12,
            status: "success".to_owned(),
        });
        let rows = render_fixture(app, 80, 10);

        assert!(rows.iter().any(|row| row.contains("• Ran printf hello")));
        assert!(rows.iter().any(|row| row.contains("└ hello")));
        assert!(!rows.iter().any(|row| row.contains("process.exec")));
    }

    #[test]
    fn redraws_the_same_execution_cell_with_complete_output() {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::ExecutionStarted {
            execution_id: "execution-1".to_owned(),
            capability: "process.exec".to_owned(),
            program: "node".to_owned(),
            args: vec!["--version".to_owned()],
            cwd: None,
            target: Some("local".to_owned()),
        });

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
        terminal.draw(|frame| draw(frame, &mut app)).expect("draw");

        app.handle_runtime_event(RuntimeEvent::ExecutionCompleted {
            execution_id: "execution-1".to_owned(),
            capability: "process.exec".to_owned(),
            stdout: "stdout one\nstdout two\nstdout three".to_owned(),
            stderr: "stderr one\nstderr two".to_owned(),
            exit_code: 0,
            duration_ms: 8,
            status: "success".to_owned(),
        });
        terminal.draw(|frame| draw(frame, &mut app)).expect("draw");

        let rows = terminal_rows(&terminal);
        assert!(rows.iter().any(|row| row.contains("stdout three")));
        assert!(rows.iter().any(|row| row.contains("stderr two")));
        assert!(rows.iter().any(|row| row.contains("Ran node --version")));
    }

    #[test]
    fn renders_running_and_failed_exec_states() {
        let mut running = App::new("conversation".to_owned());
        running.handle_runtime_event(RuntimeEvent::ExecutionStarted {
            execution_id: "tool-1".to_owned(),
            capability: "process.exec".to_owned(),
            program: "node".to_owned(),
            args: vec!["--version".to_owned()],
            cwd: None,
            target: Some("local".to_owned()),
        });
        let running_rows = render_fixture(running, 80, 8);
        assert!(running_rows.iter().any(|row| row.contains("Running")));
        assert!(
            running_rows
                .iter()
                .any(|row| row.contains("node --version"))
        );
        assert!(running_rows.iter().any(|row| row.contains("aguardando...")));

        let mut failed = App::new("conversation".to_owned());
        failed.handle_runtime_event(RuntimeEvent::ExecutionStarted {
            execution_id: "tool-2".to_owned(),
            capability: "process.exec".to_owned(),
            program: "node".to_owned(),
            args: vec!["script.js".to_owned()],
            cwd: None,
            target: Some("local".to_owned()),
        });
        failed.handle_runtime_event(RuntimeEvent::ExecutionCompleted {
            execution_id: "tool-2".to_owned(),
            capability: "process.exec".to_owned(),
            stdout: String::new(),
            stderr: "failed".to_owned(),
            exit_code: 1,
            duration_ms: 120,
            status: "failed".to_owned(),
        });
        failed.handle_runtime_event(RuntimeEvent::TurnCompleted { context: None });
        let failed_rows = render_fixture(failed, 80, 10);
        assert!(
            failed_rows
                .iter()
                .any(|row| row.contains("Ran node script.js"))
        );
        assert!(failed_rows.iter().any(|row| row.contains("failed")));
        assert!(failed_rows.iter().any(|row| row.contains("exit 1 · 120ms")));
    }

    #[test]
    fn renders_generic_tool_output_and_status() {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::ToolCompleted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
            output: Some(
                r#"{\"stdout\":\"content\",\"status\":\"success\",\"duration_ms\":12}"#.to_owned(),
            ),
        });

        let rows = render_fixture(app, 80, 12);
        assert!(rows.iter().any(|row| row.contains("Ran filesystem.read")));
        assert!(rows.iter().any(|row| row.contains("content")));
        assert!(rows.iter().any(|row| row.contains("12ms")));
    }

    #[test]
    fn renders_long_markdown_and_both_reference_sizes() {
        for (width, height) in [(80, 24), (120, 30)] {
            let mut app = App::new("conversation".to_owned());
            app.handle_runtime_event(RuntimeEvent::MessageCompleted {
                message_id: "message-1".to_owned(),
                content: "# Atlas\n\nTexto longo para testar wrapping igual ao transcript do Codex com **ênfase** e uma URL https://example.com/a/b/c.".to_owned(),
            });
            app.handle_runtime_event(RuntimeEvent::TurnCompleted { context: None });
            let rows = render_fixture(app, width, height);
            assert_eq!(rows.len(), usize::from(height));
            assert!(rows.iter().any(|row| row.contains("Atlas")));
            assert!(
                rows.iter()
                    .all(|row| row.chars().count() <= usize::from(width))
            );
        }
    }

    #[test]
    fn renders_context_usage_in_the_footer() {
        let mut app = App::new("conversation".to_owned());
        with_context(&mut app);
        let rows = render_fixture(app, 80, 24);

        assert!(rows.iter().any(|row| row.contains("6.6K / 256K (2%)")));
        assert!(rows.iter().any(|row| row.contains("Ctrl+P shortcuts")));
        assert!(!rows.iter().any(|row| row.contains("? for shortcuts")));
        assert!(!rows.iter().any(|row| row.contains("context left")));
    }

    #[test]
    fn renders_the_requested_zero_percent_footer_format() {
        let mut app = App::new("conversation".to_owned());
        app.set_context_usage(ContextUsage {
            used_tokens: 1_200,
            context_window: 256_000,
        });
        let rows = render_fixture(app, 80, 24);

        assert!(
            rows.iter().any(|row| {
                row.contains("Ctrl+P shortcuts") && row.contains("1.2K / 256K (0%)")
            })
        );
    }

    #[test]
    fn renders_shortcuts_overlay_over_the_chat_surface() {
        let mut app = App::new("conversation".to_owned());
        app.open_shortcuts();
        let rows = render_fixture(app, 80, 24);

        assert!(rows.iter().any(|row| row.contains("Shortcuts")));
        assert!(rows.iter().any(|row| row.contains("Open shortcuts")));
        assert!(rows.iter().any(|row| row.contains("Close overlay")));
    }

    #[test]
    fn renders_a_compact_shortcuts_hint_on_a_small_terminal() {
        let mut app = App::new("conversation".to_owned());
        app.open_shortcuts();
        let rows = render_fixture(app, 19, 7);

        assert!(rows.iter().any(|row| row.contains("Esc close")));
    }

    #[test]
    fn matches_codex_fixture_snapshots_at_reference_sizes() {
        assert_fixture_snapshot(
            fixture_user_answer(),
            80,
            24,
            include_str!("fixtures/user-answer-80x24.snap"),
        );
        assert_fixture_snapshot(
            fixture_user_answer(),
            120,
            30,
            include_str!("fixtures/user-answer-120x30.snap"),
        );
        assert_fixture_snapshot(
            fixture_user_exec_output_answer(),
            80,
            24,
            include_str!("fixtures/user-exec-output-answer-80x24.snap"),
        );
        assert_fixture_snapshot(
            fixture_user_exec_output_answer(),
            120,
            30,
            include_str!("fixtures/user-exec-output-answer-120x30.snap"),
        );
        assert_fixture_snapshot(
            fixture_tool_running(),
            80,
            24,
            include_str!("fixtures/tool-running-80x24.snap"),
        );
        assert_fixture_snapshot(
            fixture_tool_running(),
            120,
            30,
            include_str!("fixtures/tool-running-120x30.snap"),
        );
        assert_fixture_snapshot(
            fixture_tool_error(),
            80,
            24,
            include_str!("fixtures/tool-error-80x24.snap"),
        );
        assert_fixture_snapshot(
            fixture_tool_error(),
            120,
            30,
            include_str!("fixtures/tool-error-120x30.snap"),
        );
        assert_fixture_snapshot(
            fixture_long_markdown(),
            80,
            24,
            include_str!("fixtures/long-markdown-80x24.snap"),
        );
        assert_fixture_snapshot(
            fixture_long_markdown(),
            120,
            30,
            include_str!("fixtures/long-markdown-120x30.snap"),
        );
        assert_fixture_snapshot(
            fixture_markdown_compact(),
            80,
            24,
            include_str!("fixtures/markdown-compact-80x24.snap"),
        );
        assert_fixture_snapshot(
            fixture_markdown_compact(),
            120,
            30,
            include_str!("fixtures/markdown-compact-120x30.snap"),
        );
        assert_fixture_snapshot(
            fixture_shortcuts(),
            80,
            24,
            include_str!("fixtures/shortcuts-80x24.snap"),
        );
        assert_fixture_snapshot(
            fixture_shortcuts(),
            120,
            30,
            include_str!("fixtures/shortcuts-120x30.snap"),
        );
        assert_fixture_snapshot(
            fixture_long_execution_output(),
            80,
            24,
            include_str!("fixtures/tool-long-output-80x24.snap"),
        );
        assert_fixture_snapshot(
            fixture_long_execution_output(),
            120,
            30,
            include_str!("fixtures/tool-long-output-120x30.snap"),
        );
    }

    fn assert_fixture_snapshot(app: App, width: u16, height: u16, expected: &str) {
        let rendered = render_fixture(app, width, height)
            .into_iter()
            .filter_map(|row| {
                let row = compact_snapshot_row(&row);
                let panel_spacer = row
                    .strip_prefix('│')
                    .and_then(|row| row.strip_suffix('│'))
                    .is_some_and(|row| row.trim().is_empty());
                (!row.is_empty() && !panel_spacer).then_some(row)
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            rendered,
            expected.trim(),
            "fixture snapshot {width}x{height}"
        );
    }

    fn compact_snapshot_row(row: &str) -> String {
        let row = row.trim();
        if let Some(content) = row.strip_prefix('│').and_then(|row| row.strip_suffix('│')) {
            return format!("│{}│", content.trim_end());
        }
        row.to_owned()
    }

    fn fixture_user_answer() -> App {
        let mut app = App::new("conversation".to_owned());
        for character in "Liste os arquivos".chars() {
            app.insert_character(character);
        }
        app.submit_input();
        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-1".to_owned(),
            content: "Resposta simples.".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::TurnCompleted { context: None });
        with_context(&mut app);
        app
    }

    fn fixture_user_exec_output_answer() -> App {
        let mut app = App::new("conversation".to_owned());
        for character in "Execute o comando".chars() {
            app.insert_character(character);
        }
        app.submit_input();
        app.handle_runtime_event(RuntimeEvent::ExecutionStarted {
            execution_id: "tool-1".to_owned(),
            capability: "process.exec".to_owned(),
            program: "printf".to_owned(),
            args: vec!["ok".to_owned()],
            cwd: None,
            target: Some("local".to_owned()),
        });
        app.handle_runtime_event(RuntimeEvent::ExecutionCompleted {
            execution_id: "tool-1".to_owned(),
            capability: "process.exec".to_owned(),
            stdout: "ok".to_owned(),
            stderr: String::new(),
            exit_code: 0,
            duration_ms: 12,
            status: "success".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-2".to_owned(),
            content: "Comando concluído.".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::TurnCompleted { context: None });
        with_context(&mut app);
        app
    }

    fn fixture_tool_running() -> App {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::ExecutionStarted {
            execution_id: "tool-1".to_owned(),
            capability: "process.exec".to_owned(),
            program: "node".to_owned(),
            args: vec!["--version".to_owned()],
            cwd: None,
            target: Some("local".to_owned()),
        });
        with_context(&mut app);
        app
    }

    fn fixture_tool_error() -> App {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::ExecutionStarted {
            execution_id: "tool-1".to_owned(),
            capability: "process.exec".to_owned(),
            program: "false".to_owned(),
            args: Vec::new(),
            cwd: None,
            target: Some("local".to_owned()),
        });
        app.handle_runtime_event(RuntimeEvent::ExecutionCompleted {
            execution_id: "tool-1".to_owned(),
            capability: "process.exec".to_owned(),
            stdout: String::new(),
            stderr: "permission denied".to_owned(),
            exit_code: 1,
            duration_ms: 7,
            status: "failed".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::TurnCompleted { context: None });
        with_context(&mut app);
        app
    }

    fn fixture_long_markdown() -> App {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-1".to_owned(),
            content: "# Atlas\n\nTexto longo para validar o wrapping do transcript em uma janela estreita, com **ênfase**, `comando` e uma URL https://example.com/a/b/c.".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::TurnCompleted { context: None });
        with_context(&mut app);
        app
    }

    fn fixture_markdown_compact() -> App {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-1".to_owned(),
            content: "- primeiro item\n- segundo item\n\n```cpp\nint main() { return 0; }\n```"
                .to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::TurnCompleted { context: None });
        with_context(&mut app);
        app
    }

    fn fixture_shortcuts() -> App {
        let mut app = App::new("conversation".to_owned());
        app.open_shortcuts();
        app
    }

    fn fixture_long_execution_output() -> App {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::ExecutionStarted {
            execution_id: "execution-1".to_owned(),
            capability: "process.exec".to_owned(),
            program: "node".to_owned(),
            args: vec!["--version".to_owned()],
            cwd: None,
            target: Some("local".to_owned()),
        });
        app.handle_runtime_event(RuntimeEvent::ExecutionCompleted {
            execution_id: "execution-1".to_owned(),
            capability: "process.exec".to_owned(),
            stdout: "stdout line one\nstdout line two\nstdout line three\nstdout line four"
                .to_owned(),
            stderr: "stderr warning one\nstderr warning two".to_owned(),
            exit_code: 0,
            duration_ms: 8,
            status: "success".to_owned(),
        });
        with_context(&mut app);
        app
    }
}
