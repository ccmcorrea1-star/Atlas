use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Status};
use crate::presentation::sanitize_terminal_text;

pub(crate) struct StatusBar;

impl StatusBar {
    pub(crate) fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
        if area.is_empty() {
            return;
        }

        frame.render_widget(Paragraph::new(Self::line(app, area.width)), area);
    }

    fn line(app: &App, width: u16) -> Line<'static> {
        if app.quit_confirmation() {
            return Line::from(vec![
                Span::styled("! ", Style::default().fg(Color::Yellow).bold()),
                Span::styled(
                    "Work is still running. Quit client? [y/N]",
                    Style::default().fg(Color::Yellow),
                ),
            ]);
        }

        let status = match app.status() {
            Status::Error(message) => Line::from(vec![
                Span::styled("! ", Style::default().fg(Color::Red).bold()),
                Span::styled(
                    sanitize_terminal_text(message),
                    Style::default().fg(Color::Red),
                ),
                Span::raw("  "),
                Span::styled("Ctrl+P", Style::default().fg(Color::Cyan).bold()),
                Span::styled(" shortcuts", Style::default().dim()),
            ]),
            Status::Ready => Line::from(vec![
                Span::styled("Ctrl+P", Style::default().fg(Color::Cyan).bold()),
                Span::styled(" shortcuts", Style::default().dim()),
            ]),
            Status::Sending | Status::Thinking => Line::from(vec![
                Span::styled(
                    spinner(app.animation_tick()),
                    Style::default().fg(Color::Cyan).bold(),
                ),
                Span::raw(" "),
                Span::styled("Working", Style::default().bold()),
                Span::raw("  "),
                Span::styled("Ctrl+P", Style::default().fg(Color::Cyan).bold()),
                Span::styled(" shortcuts", Style::default().dim()),
            ]),
            Status::Tool(tool) => Line::from(vec![
                Span::styled(
                    spinner(app.animation_tick()),
                    Style::default().fg(Color::Cyan).bold(),
                ),
                Span::raw(" "),
                Span::styled("Working", Style::default().bold()),
                Span::raw(" "),
                Span::styled(
                    sanitize_terminal_text(tool),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw("  "),
                Span::styled("Ctrl+P", Style::default().fg(Color::Cyan).bold()),
                Span::styled(" shortcuts", Style::default().dim()),
            ]),
        };
        let mut left = Line::from(Span::styled("─ ", Style::default().fg(Color::DarkGray)));
        left.spans.extend(status.spans);
        let Some(context) = app.context_usage() else {
            return truncate_line(left, usize::from(width));
        };

        let right = format_context(context.used_tokens, context.context_window);
        let right_width = right.width();
        let total_width = usize::from(width);
        if right_width >= total_width {
            return truncate_line(
                Line::from(Span::styled(right, Style::default().dim())),
                total_width,
            );
        }

        let available_left = total_width.saturating_sub(right_width + 1);
        let mut spans = truncate_line(left, available_left).spans;
        let rendered_left_width = spans.iter().map(|span| span.content.width()).sum::<usize>();
        spans.push(Span::raw(
            " ".repeat(total_width - right_width - rendered_left_width),
        ));
        spans.push(Span::styled(right, Style::default().dim()));
        Line::from(spans)
    }
}

fn spinner(tick: u64) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[(tick as usize) % FRAMES.len()]
}

fn truncate_line(line: Line<'static>, width: usize) -> Line<'static> {
    if line.width() <= width {
        return line;
    }

    let mut remaining = width;
    let mut spans = Vec::new();
    for span in line.spans {
        if remaining == 0 {
            break;
        }
        let mut end = span.content.len();
        while end > 0 && UnicodeWidthStr::width(&span.content[..end]) > remaining {
            end -= 1;
            while end > 0 && !span.content.is_char_boundary(end) {
                end -= 1;
            }
        }
        if end == 0 {
            continue;
        }
        let content = span.content[..end].to_owned();
        remaining = remaining.saturating_sub(content.width());
        spans.push(Span::styled(content, span.style));
    }
    Line::from(spans)
}

pub(crate) fn format_context(used_tokens: u64, context_window: u64) -> String {
    let percent = used_tokens
        .saturating_mul(100)
        .checked_div(context_window.max(1))
        .unwrap_or_default();
    format!(
        "{} / {} ({}%)",
        compact_tokens(used_tokens),
        compact_tokens(context_window),
        percent
    )
}

fn compact_tokens(tokens: u64) -> String {
    if tokens < 1_000 {
        return tokens.to_string();
    }
    if tokens < 10_000 {
        return format_decimal(tokens, 1, 1_000, 'K');
    }
    if tokens < 1_000_000 {
        return format_decimal(tokens, 0, 1_000, 'K');
    }
    format_decimal(tokens, 1, 1_000_000, 'M')
}

fn format_decimal(tokens: u64, decimals: u32, divisor: u64, suffix: char) -> String {
    let value = tokens as f64 / divisor as f64;
    let formatted = if decimals == 0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };
    format!(
        "{}{}",
        formatted.trim_end_matches('0').trim_end_matches('.'),
        suffix
    )
}

pub(crate) struct ShortcutsOverlay;

impl ShortcutsOverlay {
    pub(crate) fn draw(frame: &mut Frame<'_>, area: Rect) {
        if area.is_empty() {
            return;
        }
        if area.width < 20 || area.height < 8 {
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new("Esc close | Ctrl+C quit").style(Style::default().fg(Color::Cyan)),
                area,
            );
            return;
        }
        let width = area.width.saturating_sub(4).min(58);
        let height = area.height.saturating_sub(4).min(13);
        let popup = Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        };
        frame.render_widget(Clear, popup);
        let content = Text::from(vec![
            Line::from(vec![
                Span::styled("Ctrl+P", Style::default().fg(Color::Cyan).bold()),
                Span::raw(" Open shortcuts"),
            ]),
            Line::from(vec![
                Span::styled("Esc", Style::default().fg(Color::Cyan).bold()),
                Span::raw("     Close overlay"),
            ]),
            Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Cyan).bold()),
                Span::raw("   Send message"),
            ]),
            Line::from(vec![
                Span::styled("Shift+Enter", Style::default().fg(Color::Cyan).bold()),
                Span::raw(" Insert newline"),
            ]),
            Line::from(vec![
                Span::styled("Up/Down", Style::default().fg(Color::Cyan).bold()),
                Span::raw("  Move cursor"),
            ]),
            Line::from(vec![
                Span::styled("PageUp/Down", Style::default().fg(Color::Cyan).bold()),
                Span::raw(" Scroll transcript"),
            ]),
            Line::from(vec![
                Span::styled("Ctrl+Home/End", Style::default().fg(Color::Cyan).bold()),
                Span::raw(" Jump transcript"),
            ]),
            Line::from(vec![
                Span::styled("Ctrl+C", Style::default().fg(Color::Cyan).bold()),
                Span::raw("    Quit"),
            ]),
        ]);
        frame.render_widget(
            Paragraph::new(content).block(
                Block::default()
                    .title(" Shortcuts ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray))
                    .title_style(Style::default().fg(Color::Cyan).bold()),
            ),
            popup,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::StatusBar;
    use crate::app::App;
    use crate::runtime::{ContextUsage, RuntimeEvent};

    #[test]
    fn truncates_long_errors_without_overflowing_the_footer() {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::Error {
            message: "x".repeat(200),
        });

        let line = StatusBar::line(&app, 24);

        assert!(line.width() <= 24);
    }

    #[test]
    fn keeps_context_visible_when_the_status_prefix_is_too_long() {
        let mut app = App::new("conversation".to_owned());
        app.set_context_usage(ContextUsage {
            used_tokens: 6_600,
            context_window: 256_000,
        });
        app.handle_runtime_event(RuntimeEvent::Error {
            message: "connection failed with a long explanation".to_owned(),
        });

        let line = StatusBar::line(&app, 32);
        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(line.width() <= 32);
        assert!(text.contains("6.6K / 256K (2%)"));
    }
}
