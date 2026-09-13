//! Transcript and composer rendering adapted from the Codex TUI cell layout.
//!
//! See `clients/tui/NOTICE` and `clients/tui/LICENSE-APACHE` for attribution.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph, Widget, Wrap};

use crate::app::{App, Message, MessageRole, Status, ToolCall};
use crate::presentation::{format_process_command, render_markdown, sanitize_terminal_text};
use crate::wrapping::{
    cursor_position, display_width, wrap_command_with_widths, wrap_lines,
    wrap_plain_no_hyphenation, wrap_text,
};

const TOOL_OUTPUT_MAX_ROWS: usize = 5;
const TRANSCRIPT_HINT: &str = "ctrl + t to view transcript";

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let width = frame.area().width;
    let composer_height = composer_height(app, width);
    let status_height = u16::from(status_is_visible(app)) * 2;
    let bottom_height = composer_height.saturating_add(status_height);
    let [history_area, bottom_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(bottom_height)])
            .areas(frame.area());

    draw_history(frame, app, history_area);
    draw_bottom_pane(frame, app, bottom_area, composer_height);
}

fn draw_history(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.is_empty() {
        return;
    }

    let lines = history_lines(app, area.width);
    if lines.is_empty() {
        return;
    }
    let total_rows = wrapped_line_count(&lines, area.width);
    let visible_rows = usize::from(area.height);
    let max_scroll = total_rows.saturating_sub(visible_rows);
    let scroll = max_scroll
        .saturating_sub(app.history_scroll())
        .min(u16::MAX as usize) as u16;
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        area,
    );
}

fn history_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut previous_role = None;
    for message in app.messages() {
        if needs_cell_separator(previous_role, message.role) {
            lines.push(Line::default());
        }
        if let Some(tool) = message.tool.as_ref() {
            lines.extend(tool_lines(tool, width));
        } else {
            lines.extend(message_lines(message, width));
        }
        previous_role = Some(message.role);
    }
    lines
}

fn needs_cell_separator(previous: Option<MessageRole>, current: MessageRole) -> bool {
    match (previous, current) {
        (Some(MessageRole::Atlas | MessageRole::System), MessageRole::Tool)
        | (Some(MessageRole::Tool), MessageRole::Atlas | MessageRole::System) => true,
        _ => false,
    }
}

fn message_lines(message: &Message, width: u16) -> Vec<Line<'static>> {
    match message.role {
        MessageRole::User => user_message_lines(&message.content, width),
        MessageRole::Atlas => agent_message_lines(&message.content, width),
        MessageRole::System => prefixed_message_lines(
            &message.content,
            width,
            "! ",
            Style::default().fg(Color::Red).dim(),
        ),
        MessageRole::Tool => Vec::new(),
    }
}

fn user_message_lines(content: &str, width: u16) -> Vec<Line<'static>> {
    let content = sanitize_terminal_text(content);
    let content = content.trim_end_matches(['\r', '\n']);
    if content.is_empty() {
        return Vec::new();
    }

    let wrap_width = usize::from(width).saturating_sub(3).max(1);
    let mut lines = vec![Line::default()];
    let mut first = true;
    for source_line in content.split('\n') {
        for line in wrap_text(source_line, wrap_width) {
            let prefix = if first { "› " } else { "  " };
            lines.push(prefixed_line(
                Line::from(line),
                prefix,
                Style::default().bold().dim(),
            ));
            first = false;
        }
    }
    lines.push(Line::default());
    lines
}

fn agent_message_lines(content: &str, width: u16) -> Vec<Line<'static>> {
    let body = render_markdown(content);
    let wrap_width = usize::from(width).saturating_sub(2).max(1);
    let mut lines = Vec::new();
    let mut first = true;
    for line in wrap_lines(body, wrap_width) {
        let prefix = if first { "• " } else { "  " };
        lines.push(prefixed_line(line, prefix, Style::default().dim()));
        first = false;
    }
    if lines.is_empty() {
        lines.push(prefixed_line(Line::default(), "• ", Style::default().dim()));
    }
    lines
}

fn prefixed_message_lines(
    content: &str,
    width: u16,
    prefix: &str,
    prefix_style: Style,
) -> Vec<Line<'static>> {
    let wrap_width = usize::from(width)
        .saturating_sub(display_width(prefix))
        .max(1);
    let mut lines = Vec::new();
    let mut first = true;
    for source_line in sanitize_terminal_text(content).split('\n') {
        for line in wrap_text(source_line, wrap_width) {
            lines.push(prefixed_line(
                Line::from(line),
                if first { prefix } else { "  " },
                prefix_style,
            ));
            first = false;
        }
    }
    lines
}

fn tool_lines(tool: &ToolCall, width: u16) -> Vec<Line<'static>> {
    if tool.name == "process.exec" {
        return exec_lines(tool, width);
    }

    let title = if tool.completed { "Ran" } else { "Running" };
    let mut line = Line::from(vec![
        execution_marker(tool),
        Span::raw(" "),
        Span::styled(title, Style::default().bold()),
    ]);
    line.push_span(format!(" {}", tool.name).cyan());
    vec![line]
}

fn exec_lines(tool: &ToolCall, width: u16) -> Vec<Line<'static>> {
    let command = tool
        .program
        .as_deref()
        .map(|program| format_process_command(program, &tool.args))
        .unwrap_or_default();
    let title = if tool.completed { "Ran" } else { "Running" };
    let bullet = execution_marker(tool);
    let mut header = Line::from(vec![
        bullet,
        Span::raw(" "),
        Span::styled(title, Style::default().bold()),
    ]);
    let header_width = header.width();
    let mut lines = Vec::new();

    if !command.is_empty() {
        let command_lines = wrap_command_with_widths(
            &command,
            usize::from(width).saturating_sub(header_width).max(1),
            usize::from(width).saturating_sub(4).max(1),
        );
        if let Some(first) = command_lines.first() {
            header.push_span(" ");
            header.push_span(Span::styled(first.clone(), command_style()));
        }
        lines.push(header);
        let continuation_count = command_lines.len().saturating_sub(1);
        for continuation in command_lines.iter().skip(1).take(2) {
            lines.push(prefixed_line(
                Line::from(Span::styled(continuation.clone(), command_style())),
                "  │ ",
                Style::default().dim(),
            ));
        }
        if continuation_count > 2 {
            lines.push(prefixed_line(
                Line::from(format!("… +{} lines", continuation_count - 2)),
                "  │ ",
                Style::default().dim(),
            ));
        }
    } else {
        lines.push(header);
    }

    if tool.completed {
        append_exec_output(&mut lines, tool, width);
        if let Some(duration) = tool.duration {
            if tool.success {
                lines[0].push_span(format!(" · {}", format_duration(duration)).dim());
            } else {
                let exit_code = tool
                    .exit_code
                    .map_or_else(|| "?".to_owned(), |code| code.to_string());
                lines.push(prefixed_line(
                    Line::from(format!(
                        "✗ exit {exit_code} · {}",
                        format_duration(duration)
                    )),
                    "  ",
                    Style::default().fg(Color::Red).dim(),
                ));
            }
        }
    }
    lines
}

fn execution_marker(tool: &ToolCall) -> Span<'static> {
    if !tool.completed {
        let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let elapsed = tool
            .started_at
            .map(|started| started.elapsed().as_millis() / 120)
            .unwrap_or_default();
        return Span::styled(
            frames[(elapsed as usize) % frames.len()],
            Style::default().dim(),
        );
    }
    Span::styled(
        "•",
        if tool.success {
            Style::default().fg(Color::Green).bold()
        } else {
            Style::default().fg(Color::Red).bold()
        },
    )
}

fn command_style() -> Style {
    Style::default().fg(Color::Cyan)
}

fn append_exec_output(lines: &mut Vec<Line<'static>>, tool: &ToolCall, width: u16) {
    let output = [
        tool.output.as_deref().unwrap_or_default(),
        tool.stderr.as_deref().unwrap_or_default(),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join("\n");

    if output.is_empty() {
        lines.push(prefixed_line(
            Line::from("(no output)"),
            "  └ ",
            Style::default().dim(),
        ));
        return;
    }

    let output_width = usize::from(width).saturating_sub(4).max(1);
    let mut rendered = Vec::new();
    for (index, output_line) in output.lines().enumerate() {
        let prefix = if index == 0 { "  └ " } else { "    " };
        for wrapped in wrap_plain_no_hyphenation(output_line, output_width) {
            rendered.push(prefixed_line(
                Line::from(wrapped),
                prefix,
                Style::default().dim(),
            ));
        }
    }

    if rendered.len() > TOOL_OUTPUT_MAX_ROWS {
        let keep = TOOL_OUTPUT_MAX_ROWS.saturating_sub(1);
        let head = keep / 2;
        let tail = keep - head;
        let hidden = rendered.len() - keep;
        let mut limited = rendered[..head].to_vec();
        limited.push(prefixed_line(
            Line::from(format!("… +{hidden} lines ({TRANSCRIPT_HINT})")),
            "    ",
            Style::default().dim(),
        ));
        limited.extend(rendered[rendered.len() - tail..].iter().cloned());
        lines.extend(limited);
    } else {
        lines.extend(rendered);
    }
}

fn draw_bottom_pane(frame: &mut Frame<'_>, app: &App, area: Rect, composer_height: u16) {
    if area.is_empty() {
        return;
    }
    let composer_area = if status_is_visible(app) {
        let status_area = Rect { height: 1, ..area };
        frame.render_widget(Paragraph::new(status_line(app)), status_area);
        Rect {
            y: area.y + 2,
            height: composer_height,
            ..area
        }
    } else {
        area
    };
    draw_composer(frame, app, composer_area);
}

fn draw_composer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.is_empty() {
        return;
    }
    Block::default().render(area, frame.buffer_mut());
    let input_rows = area.height.saturating_sub(3);
    if input_rows == 0 {
        return;
    }

    let content_width = usize::from(area.width).saturating_sub(3).max(1);
    let rows = wrap_text(app.input(), content_width);
    let first_visible = rows.len().saturating_sub(usize::from(input_rows));
    let visible = rows.iter().skip(first_visible).enumerate();
    for (index, text) in visible {
        let row = first_visible + index;
        let prompt = if row == 0 { "› " } else { "  " };
        let prompt_style = if row == 0 {
            Style::default().bold()
        } else {
            Style::default()
        };
        let line = Line::from(vec![
            Span::styled(prompt, prompt_style),
            Span::raw(text.clone()),
        ]);
        frame.render_widget(
            Paragraph::new(line),
            Rect {
                x: area.x,
                y: area.y + 1 + index as u16,
                width: area.width,
                height: 1,
            },
        );
    }
    if app.input().is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "Ask Atlas to do anything",
                Style::default().dim(),
            )),
            Rect {
                x: area.x + 2,
                y: area.y + 1,
                width: area.width.saturating_sub(2),
                height: 1,
            },
        );
    }

    let (cursor_row, cursor_column) =
        cursor_position(app.input(), app.cursor_byte_position(), content_width);
    if cursor_row >= first_visible && cursor_row - first_visible < usize::from(input_rows) {
        let cursor_x = area
            .x
            .saturating_add(2)
            .saturating_add(cursor_column as u16)
            .min(area.x.saturating_add(area.width.saturating_sub(1)));
        frame.set_cursor_position(Position::new(
            cursor_x,
            area.y + 1 + (cursor_row - first_visible) as u16,
        ));
    }

    let footer_area = Rect {
        y: area.y + area.height - 1,
        height: 1,
        ..area
    };
    frame.render_widget(
        Paragraph::new(footer_line(app, footer_area.width)),
        footer_area,
    );
}

fn footer_line(app: &App, width: u16) -> Line<'static> {
    let left = if app.input().is_empty() && !status_is_visible(app) {
        Some(Line::from(vec![
            Span::raw("?"),
            Span::styled(" for shortcuts", Style::default().dim()),
        ]))
    } else if !app.input().is_empty() && status_is_visible(app) {
        Some(Line::from(vec![
            Span::raw("tab"),
            Span::styled(" to queue message", Style::default().dim()),
        ]))
    } else {
        None
    };
    let right = Line::from(Span::styled("100% context left", Style::default().dim()));
    let left_width = left.as_ref().map_or(0, Line::width);
    let right_width = right.width();
    let available_left = usize::from(width)
        .saturating_sub(right_width)
        .saturating_sub(1);
    let mut spans = Vec::new();
    if let Some(mut left) = left {
        if left_width <= available_left {
            spans.append(&mut left.spans);
            spans.push(Span::raw(" ".repeat(available_left - left_width)));
        }
    } else {
        spans.push(Span::raw(" ".repeat(available_left)));
    }
    spans.extend(right.spans);
    Line::from(spans)
}

fn status_line(app: &App) -> Line<'static> {
    match app.status() {
        Status::Error(message) => Line::from(vec![
            Span::styled("!", Style::default().fg(Color::Red).bold()),
            Span::raw(" "),
            Span::styled(message.clone(), Style::default().fg(Color::Red)),
        ]),
        Status::Ready => Line::default(),
        Status::Sending | Status::Thinking | Status::Tool(_) => Line::from(vec![
            Span::styled("•", Style::default().dim()),
            Span::raw(" "),
            Span::styled("Working", Style::default().bold()),
        ]),
    }
}

fn status_is_visible(app: &App) -> bool {
    !matches!(app.status(), Status::Ready)
}

fn composer_height(app: &App, width: u16) -> u16 {
    let content_width = usize::from(width).saturating_sub(3).max(1);
    wrap_text(app.input(), content_width).len() as u16 + 3
}

fn prefixed_line(mut line: Line<'static>, prefix: &str, style: Style) -> Line<'static> {
    let mut spans = vec![Span::styled(prefix.to_owned(), style)];
    spans.append(&mut line.spans);
    Line::from(spans).style(line.style)
}

fn wrapped_line_count(lines: &[Line<'static>], width: u16) -> usize {
    let width = usize::from(width.max(1));
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum()
}

fn format_duration(duration: Duration) -> String {
    if duration.as_millis() < 1_000 {
        return format!("{}ms", duration.as_millis());
    }
    if duration.as_secs() < 60 {
        return format!("{:.1}s", duration.as_secs_f64());
    }
    format!(
        "{}m {:02}s",
        duration.as_secs() / 60,
        duration.as_secs() % 60
    )
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::draw;
    use crate::app::App;
    use crate::runtime::RuntimeEvent;

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

    fn render_fixture(app: App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal.draw(|frame| draw(frame, &app)).expect("draw");
        terminal_rows(&terminal)
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
        app.handle_runtime_event(RuntimeEvent::TurnCompleted);
        let rows = render_fixture(app, 80, 10);

        assert!(rows.iter().any(|row| row == "› hi"));
        assert!(
            rows.iter()
                .any(|row| row == "• Resposta normal com markdown.")
        );
        assert!(!rows.iter().any(|row| row.contains("Atlas:")));
        assert!(!rows.iter().any(|row| row.contains("READY")));
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
        failed.handle_runtime_event(RuntimeEvent::TurnCompleted);
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
    fn renders_long_markdown_and_both_reference_sizes() {
        for (width, height) in [(80, 24), (120, 30)] {
            let mut app = App::new("conversation".to_owned());
            app.handle_runtime_event(RuntimeEvent::MessageCompleted {
                message_id: "message-1".to_owned(),
                content: "# Atlas\n\nTexto longo para testar wrapping igual ao transcript do Codex com **ênfase** e uma URL https://example.com/a/b/c.".to_owned(),
            });
            app.handle_runtime_event(RuntimeEvent::TurnCompleted);
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
    }

    fn assert_fixture_snapshot(app: App, width: u16, height: u16, expected: &str) {
        let rendered = render_fixture(app, width, height)
            .into_iter()
            .map(|row| {
                let row = row.trim();
                if let Some(prefix) = row.strip_suffix("100% context left") {
                    let prefix = prefix.trim_end();
                    return format!("{prefix}<right>100% context left");
                }
                if let Some((_, rest)) = row.split_once(' ')
                    && rest.starts_with("Running")
                {
                    format!("<spinner> {rest}")
                } else {
                    row.to_owned()
                }
            })
            .filter(|row| !row.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            rendered,
            expected.trim(),
            "fixture snapshot {width}x{height}"
        );
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
        app.handle_runtime_event(RuntimeEvent::TurnCompleted);
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
        app.handle_runtime_event(RuntimeEvent::TurnCompleted);
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
        app.handle_runtime_event(RuntimeEvent::TurnCompleted);
        app
    }

    fn fixture_long_markdown() -> App {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-1".to_owned(),
            content: "# Atlas\n\nTexto longo para validar o wrapping do transcript em uma janela estreita, com **ênfase**, `comando` e uma URL https://example.com/a/b/c.".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::TurnCompleted);
        app
    }

    fn fixture_markdown_compact() -> App {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-1".to_owned(),
            content: "- primeiro item\n- segundo item\n\n```text\nhello\n```".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::TurnCompleted);
        app
    }
}
