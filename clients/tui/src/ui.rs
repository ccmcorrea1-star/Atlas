use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, Message, MessageRole, Status, ToolCall};
use crate::presentation::render_markdown;

const MAX_INPUT_ROWS: usize = 4;
const TOOL_OUTPUT_PREVIEW_LINES: usize = 6;
const HISTORY_INDENT: &str = "  ";
const SPINNER_FRAMES: &[&str] = &["·", "o", "O", "o"];

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let input_rows = wrapped_input_lines(app.input(), frame.area().width)
        .len()
        .clamp(1, MAX_INPUT_ROWS);
    let areas = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length((input_rows + 1) as u16),
        Constraint::Length(1),
    ])
    .split(frame.area());

    draw_header(frame, app, areas[0]);
    draw_history(frame, app, areas[1]);
    draw_composer(frame, app, areas[2]);
    draw_footer(frame, app, areas[3]);
}

fn draw_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.is_empty() {
        return;
    }

    let left = "  ATLAS";
    let right = format!("{}  ", app.conversation_id());
    let gap = usize::from(area.width)
        .saturating_sub(display_width(left))
        .saturating_sub(display_width(&right));
    let title = Line::from(vec![
        Span::styled(
            "  ATLAS",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(gap)),
        Span::styled(right, Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(title), area);

    if area.height > 1 {
        let rule = Line::from(Span::styled(
            "─".repeat(usize::from(area.width)),
            Style::default().fg(Color::DarkGray),
        ));
        frame.render_widget(
            Paragraph::new(rule),
            Rect {
                y: area.y + 1,
                height: 1,
                ..area
            },
        );
    }
}

fn draw_history(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.is_empty() {
        return;
    }

    let lines = history_lines(app, area.width);
    let total_rows = wrapped_line_count(&lines, area.width);
    let paragraph = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false });
    let visible_rows = usize::from(area.height);
    let max_scroll = total_rows.saturating_sub(visible_rows);
    let scroll = max_scroll
        .saturating_sub(app.history_scroll())
        .min(u16::MAX as usize) as u16;
    frame.render_widget(paragraph.scroll((scroll, 0)), area);
}

fn history_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (index, message) in app.messages().iter().enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }
        if let Some(tool) = message.tool.as_ref() {
            lines.extend(tool_lines(tool, width));
        } else {
            lines.extend(message_lines(message));
        }
    }
    lines
}

fn message_lines(message: &Message) -> Vec<Line<'static>> {
    let label_style = match message.role {
        MessageRole::User => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        MessageRole::Atlas => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        MessageRole::System => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        MessageRole::Tool => Style::default(),
    };
    let mut lines = vec![Line::from(vec![
        Span::raw(HISTORY_INDENT),
        Span::styled(message.role.label(), label_style),
    ])];

    let body = render_markdown(&message.content);
    lines.extend(
        body.into_iter()
            .map(|line| prefixed_line(line, HISTORY_INDENT)),
    );
    lines
}

fn tool_lines(tool: &ToolCall, width: u16) -> Vec<Line<'static>> {
    let mut lines = vec![tool_header(tool, width)];
    let output_lines = tool_preview_lines(tool.output.as_deref().unwrap_or_default());
    let mut first_output = true;
    for output in output_lines {
        for segment in wrap_text_line(&output, usize::from(width).saturating_sub(4).max(1)) {
            let prefix = if first_output { "  └ " } else { "    " };
            lines.push(Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::DarkGray)),
                Span::styled(segment, Style::default().dim()),
            ]));
            first_output = false;
        }
    }
    lines
}

fn tool_header(tool: &ToolCall, width: u16) -> Line<'static> {
    let (right, right_style) = if tool.completed {
        (
            format!(
                "✓ {}",
                tool.duration
                    .map(format_duration)
                    .unwrap_or_else(|| "done".to_owned())
            ),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            format!("{} RUNNING", spinner_frame()),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
    };
    let right_width = display_width(&right);
    let available_left = usize::from(width)
        .saturating_sub(right_width)
        .saturating_sub(1);
    let left = truncate_to_width(&format!("{HISTORY_INDENT}{}", tool.name), available_left);
    let gap = usize::from(width)
        .saturating_sub(display_width(&left))
        .saturating_sub(right_width);
    Line::from(vec![
        Span::styled(left, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" ".repeat(gap)),
        Span::styled(right, right_style),
    ])
}

fn tool_preview_lines(output: &str) -> Vec<String> {
    if output.is_empty() {
        return Vec::new();
    }
    let raw_lines = output.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
    if raw_lines.len() <= TOOL_OUTPUT_PREVIEW_LINES {
        return raw_lines;
    }

    let head = TOOL_OUTPUT_PREVIEW_LINES / 2;
    let tail = TOOL_OUTPUT_PREVIEW_LINES.saturating_sub(head + 1);
    let omitted = raw_lines.len().saturating_sub(head + tail);
    raw_lines[..head]
        .iter()
        .cloned()
        .chain(std::iter::once(format!("… +{omitted} lines")))
        .chain(raw_lines[raw_lines.len() - tail..].iter().cloned())
        .collect()
}

fn draw_composer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.is_empty() {
        return;
    }
    let rule = Line::from(Span::styled(
        "─".repeat(usize::from(area.width)),
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(rule), Rect { height: 1, ..area });

    if area.height <= 1 {
        return;
    }
    let input_area = Rect {
        y: area.y + 1,
        height: area.height - 1,
        ..area
    };
    let rows = wrapped_input_lines(app.input(), area.width);
    let first_visible = rows.len().saturating_sub(usize::from(input_area.height));
    let visible = rows
        .iter()
        .skip(first_visible)
        .enumerate()
        .map(|(index, text)| {
            let prefix = if first_visible + index == 0 {
                "> "
            } else {
                "  "
            };
            Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Cyan)),
                Span::raw(text.clone()),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Text::from(visible)), input_area);

    let (cursor_row, cursor_column) =
        input_cursor_position(app.input(), app.cursor_byte_position(), area.width);
    if cursor_row >= first_visible && cursor_row - first_visible < usize::from(input_area.height) {
        let cursor_x = area
            .x
            .saturating_add(2)
            .saturating_add(cursor_column as u16)
            .min(area.x.saturating_add(area.width.saturating_sub(1)));
        frame.set_cursor_position(Position::new(
            cursor_x,
            input_area.y + (cursor_row - first_visible) as u16,
        ));
    }
}

fn draw_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.is_empty() {
        return;
    }
    let left = "  Enter send · Shift+Enter newline · PgUp/PgDn scroll · Esc quit";
    let right = truncate_to_width(&status_text(app.status()), usize::from(area.width) / 2);
    let available_left = usize::from(area.width)
        .saturating_sub(display_width(&right))
        .saturating_sub(1);
    let left = truncate_to_width(left, available_left);
    let gap = usize::from(area.width)
        .saturating_sub(display_width(&left))
        .saturating_sub(display_width(&right));
    let line = Line::from(vec![
        Span::styled(left, Style::default().dim()),
        Span::raw(" ".repeat(gap)),
        Span::styled(right, status_style(app.status())),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn prefixed_line(mut line: Line<'static>, prefix: &str) -> Line<'static> {
    let mut spans = vec![Span::raw(prefix.to_owned())];
    spans.append(&mut line.spans);
    Line::from(spans).style(line.style)
}

fn wrapped_input_lines(input: &str, width: u16) -> Vec<String> {
    let content_width = usize::from(width).saturating_sub(4).max(1);
    let mut lines = vec![String::new()];
    let mut current_width = 0;
    for character in input.chars() {
        if character == '\n' {
            lines.push(String::new());
            current_width = 0;
            continue;
        }
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if current_width > 0 && current_width + character_width > content_width {
            lines.push(String::new());
            current_width = 0;
        }
        lines
            .last_mut()
            .expect("input always has a line")
            .push(character);
        current_width += character_width;
    }
    lines
}

fn input_cursor_position(input: &str, cursor: usize, width: u16) -> (usize, usize) {
    let content_width = usize::from(width).saturating_sub(4).max(1);
    let mut row = 0;
    let mut column = 0;
    for (index, character) in input.char_indices() {
        if index >= cursor {
            break;
        }
        if character == '\n' {
            row += 1;
            column = 0;
            continue;
        }
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if column > 0 && column + character_width > content_width {
            row += 1;
            column = 0;
        }
        column += character_width;
    }
    (row, column)
}

fn wrap_text_line(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut current_width = 0;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if current_width > 0 && current_width + character_width > width {
            lines.push(String::new());
            current_width = 0;
        }
        lines
            .last_mut()
            .expect("wrapped text always has a line")
            .push(character);
        current_width += character_width;
    }
    lines
}

fn status_text(status: &Status) -> String {
    match status {
        Status::Ready => "READY".to_owned(),
        Status::Sending => "SENDING".to_owned(),
        Status::Thinking => format!("{} THINKING", spinner_frame()),
        Status::Tool(name) => format!("{} {}", spinner_frame(), name.to_uppercase()),
        Status::Error(message) => format!("ERROR {}", truncate_to_width(message, 24)),
    }
}

fn status_style(status: &Status) -> Style {
    match status {
        Status::Ready => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        Status::Sending | Status::Thinking | Status::Tool(_) => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        Status::Error(_) => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    }
}

fn spinner_frame() -> &'static str {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    SPINNER_FRAMES[(millis / 160) as usize % SPINNER_FRAMES.len()]
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

fn truncate_to_width(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_owned();
    }
    if width <= 1 {
        return "…".chars().take(width).collect();
    }
    let mut result = String::new();
    let mut used = 0;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width + 1 > width {
            break;
        }
        result.push(character);
        used += character_width;
    }
    result.push('…');
    result
}

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

fn wrapped_line_count(lines: &[Line<'static>], width: u16) -> usize {
    let width = usize::from(width.max(1));
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum()
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::draw;
    use crate::app::App;
    use crate::runtime::RuntimeEvent;

    fn terminal_text(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn renders_dense_transcript_with_tool_output_and_status() {
        let mut app = App::new("minha-conversa".to_owned());
        app.insert_character('t');
        app.insert_character('e');
        app.insert_character('s');
        app.insert_character('t');
        assert_eq!(app.submit_input().as_deref(), Some("test"));
        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-1".to_owned(),
            content: "Vou executar isso.\n\n**Tudo pronto.**".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_name: "process.exec".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::ToolCompleted {
            tool_name: "process.exec".to_owned(),
            output: Some(r#"{\"message\":\"Teste funcionando!\"}"#.to_owned()),
        });
        app.handle_runtime_event(RuntimeEvent::TurnCompleted);

        let mut terminal = Terminal::new(TestBackend::new(80, 18)).expect("terminal");
        terminal.draw(|frame| draw(frame, &app)).expect("draw");
        let rendered = terminal_text(&terminal);

        assert!(rendered.contains("ATLAS"));
        assert!(rendered.contains("Você"));
        assert!(rendered.contains("process.exec"));
        assert!(rendered.contains("Teste funcionando!"));
        assert!(!rendered.contains(r#"{\"message\""#));
        assert!(rendered.contains("READY"));
    }

    #[test]
    fn wraps_input_and_keeps_cursor_on_the_visible_line() {
        let mut app = App::new("conversation".to_owned());
        for character in "a very long input that should wrap".chars() {
            app.insert_character(character);
        }
        let mut terminal = Terminal::new(TestBackend::new(24, 12)).expect("terminal");
        terminal.draw(|frame| draw(frame, &app)).expect("draw");
        assert!(
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .any(|cell| cell.symbol() == "a")
        );
    }
}
