use std::time::Duration;

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

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let input_rows = wrapped_input_lines(app.input(), frame.area().width)
        .len()
        .clamp(1, MAX_INPUT_ROWS);
    let areas = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length((input_rows + 2) as u16),
    ])
    .split(frame.area());

    draw_history(frame, app, areas[0]);
    draw_composer(frame, app, areas[1]);
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
    if app.messages().is_empty() {
        return vec![
            Line::from(Span::styled(
                "  ATLAS",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "  terminal client",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                format!("  conversation {}", app.conversation_id()),
                Style::default().fg(Color::DarkGray),
            )),
        ];
    }

    let mut lines = Vec::new();
    for (index, message) in app.messages().iter().enumerate() {
        if index > 0 && message.role != MessageRole::Tool {
            lines.push(Line::default());
        }
        if let Some(tool) = message.tool.as_ref() {
            lines.extend(tool_lines(tool, width));
        } else {
            lines.extend(message_lines(message, width));
        }
    }
    lines
}

fn message_lines(message: &Message, width: u16) -> Vec<Line<'static>> {
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
    let prefix = match message.role {
        MessageRole::User => "›",
        MessageRole::Atlas => "•",
        MessageRole::System => "!",
        MessageRole::Tool => "•",
    };
    let mut body = render_markdown(&message.content);
    let mut lines = Vec::new();
    if let Some(first) = body.first().cloned() {
        let first_lines = wrap_line_to_width(first, usize::from(width).saturating_sub(4).max(1));
        for (index, line) in first_lines.into_iter().enumerate() {
            let prefix = if index == 0 {
                format!("{HISTORY_INDENT}{prefix} ")
            } else {
                "    ".to_owned()
            };
            lines.push(prefixed_styled_line(line, &prefix, label_style));
        }
        body.remove(0);
    } else {
        lines.push(Line::from(vec![
            Span::raw(HISTORY_INDENT),
            Span::styled(prefix, label_style),
        ]));
    }
    for line in body {
        lines.extend(
            wrap_line_to_width(line, usize::from(width).saturating_sub(3).max(1))
                .into_iter()
                .map(|line| prefixed_line(line, "   ")),
        );
    }
    lines
}

fn tool_lines(tool: &ToolCall, width: u16) -> Vec<Line<'static>> {
    let mut lines = vec![tool_header(tool, width)];
    append_tool_stream(
        &mut lines,
        "out",
        tool.output.as_deref().unwrap_or_default(),
        width,
        Style::default().dim(),
    );
    append_tool_stream(
        &mut lines,
        "err",
        tool.stderr.as_deref().unwrap_or_default(),
        width,
        Style::default().fg(Color::Yellow).dim(),
    );
    lines
}

fn tool_header(tool: &ToolCall, width: u16) -> Line<'static> {
    let (right, right_style) = if tool.completed {
        (
            format!(
                "{} {}",
                if tool.success { "✓" } else { "✗" },
                tool.duration
                    .map(format_duration)
                    .unwrap_or_else(|| "done".to_owned())
            ),
            if tool.success {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            },
        )
    } else {
        (
            "… running".to_owned(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
    };
    let right_width = display_width(&right);
    let available_left = usize::from(width)
        .saturating_sub(right_width)
        .saturating_sub(1);
    let left = truncate_to_width(&format!("{HISTORY_INDENT}• {}", tool.name), available_left);
    let gap = usize::from(width)
        .saturating_sub(display_width(&left))
        .saturating_sub(right_width);
    Line::from(vec![
        Span::styled(left, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" ".repeat(gap)),
        Span::styled(right, right_style),
    ])
}

fn append_tool_stream(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    output: &str,
    width: u16,
    style: Style,
) {
    if output.is_empty() {
        return;
    }
    let output_lines = tool_preview_lines(output);
    let content_width = usize::from(width).saturating_sub(8).max(1);
    let mut first_output = true;
    for output in output_lines {
        for segment in wrap_text_line(&output, content_width) {
            let prefix = if first_output {
                first_output = false;
                format!("  └ {label} ")
            } else {
                "      ".to_owned()
            };
            lines.push(Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::DarkGray)),
                Span::styled(segment, style),
            ]));
        }
    }
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
        height: area.height.saturating_sub(2),
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
                "› "
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
    if area.height < 3 {
        return;
    }
    let left = "  Enter send · Shift+Enter newline · PgUp/PgDn scroll · Esc quit";
    let right = status_text(app.status());
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
    frame.render_widget(
        Paragraph::new(line),
        Rect {
            y: area.y + area.height - 1,
            height: 1,
            ..area
        },
    );
}

fn prefixed_line(mut line: Line<'static>, prefix: &str) -> Line<'static> {
    let mut spans = vec![Span::raw(prefix.to_owned())];
    spans.append(&mut line.spans);
    Line::from(spans).style(line.style)
}

fn prefixed_styled_line(mut line: Line<'static>, prefix: &str, style: Style) -> Line<'static> {
    let mut spans = vec![Span::styled(prefix.to_owned(), style)];
    spans.append(&mut line.spans);
    Line::from(spans).style(line.style)
}

fn wrap_line_to_width(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::default().style(line.style)];
    let mut current_width = 0;
    for span in line.spans {
        for character in span.content.chars() {
            let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
            if current_width > 0 && current_width + character_width > width {
                lines.push(Line::default().style(line.style));
                current_width = 0;
            }
            lines
                .last_mut()
                .expect("wrapped line always has a current line")
                .spans
                .push(Span::styled(character.to_string(), span.style));
            current_width += character_width;
        }
    }
    lines
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
        Status::Thinking => "THINKING".to_owned(),
        Status::Tool(name) => format!("… {}", name.to_uppercase()),
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
            tool_id: "tool-1".to_owned(),
            tool_name: "process.exec".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::ToolCompleted {
            tool_id: "tool-1".to_owned(),
            tool_name: "process.exec".to_owned(),
            output: Some(
                r#"{\"stdout\":\"Teste funcionando!\",\"stderr\":\"warning\",\"exit_code\":0,\"duration_ms\":12}"#
                    .to_owned(),
            ),
        });
        app.handle_runtime_event(RuntimeEvent::TurnCompleted);

        let mut terminal = Terminal::new(TestBackend::new(80, 18)).expect("terminal");
        terminal.draw(|frame| draw(frame, &app)).expect("draw");
        let rendered = terminal_text(&terminal);

        assert!(rendered.contains("› test"));
        assert!(rendered.contains("process.exec"));
        assert!(rendered.contains("Teste funcionando!"));
        assert!(rendered.contains("warning"));
        assert!(!rendered.contains(r#"{\"message\""#));
        assert!(rendered.contains("READY"));
    }

    #[test]
    fn snapshots_transcript_at_80_columns() {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-1".to_owned(),
            content: "Resposta curta.".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "tool-1".to_owned(),
            tool_name: "process.exec".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::ToolCompleted {
            tool_id: "tool-1".to_owned(),
            tool_name: "process.exec".to_owned(),
            output: Some(
                r#"{\"stdout\":\"hello\",\"stderr\":\"warning\",\"exit_code\":0,\"duration_ms\":12}"#
                    .to_owned(),
            ),
        });
        app.handle_runtime_event(RuntimeEvent::TurnCompleted);

        let mut terminal = Terminal::new(TestBackend::new(80, 14)).expect("terminal");
        terminal.draw(|frame| draw(frame, &app)).expect("draw");

        assert_eq!(
            terminal_rows(&terminal),
            vec![
                "  • Resposta curta.".to_owned(),
                "  • process.exec                                                          ✓ 12ms"
                    .to_owned(),
                "  └ out hello".to_owned(),
                "  └ err warning".to_owned(),
                "".to_owned(),
                "".to_owned(),
                "".to_owned(),
                "".to_owned(),
                "".to_owned(),
                "".to_owned(),
                "".to_owned(),
                "────────────────────────────────────────────────────────────────────────────────"
                    .to_owned(),
                "›".to_owned(),
                "  Enter send · Shift+Enter newline · PgUp/PgDn scroll · Esc quit           READY"
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn snapshots_wrapping_at_40_columns() {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-1".to_owned(),
            content: "Texto suficientemente longo para demonstrar o wrapping do transcript."
                .to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::TurnCompleted);

        let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("terminal");
        terminal.draw(|frame| draw(frame, &app)).expect("draw");

        assert_eq!(
            terminal_rows(&terminal),
            vec![
                "  • Texto suficientemente longo para dem".to_owned(),
                "    onstrar o wrapping do transcript.".to_owned(),
                "".to_owned(),
                "".to_owned(),
                "".to_owned(),
                "".to_owned(),
                "".to_owned(),
                "".to_owned(),
                "".to_owned(),
                "────────────────────────────────────────".to_owned(),
                "›".to_owned(),
                "  Enter send · Shift+Enter newlin… READY".to_owned(),
            ]
        );
    }

    #[test]
    fn renders_atlas_welcome_before_the_first_turn() {
        let app = App::new("conversation".to_owned());
        let mut terminal = Terminal::new(TestBackend::new(40, 10)).expect("terminal");
        terminal.draw(|frame| draw(frame, &app)).expect("draw");
        let rendered = terminal_text(&terminal);

        assert!(rendered.contains("ATLAS"));
        assert!(rendered.contains("terminal client"));
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
