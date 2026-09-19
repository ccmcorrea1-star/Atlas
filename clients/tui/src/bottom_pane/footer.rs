//! Renderizacao do footer adaptada do painel inferior da TUI do Codex.
//!
//! O footer e uma view pura do estado do composer. Ele aplica a mesma ordem de
//! fallback por largura do Codex sem deixar o contexto cobrir a dica em terminais estreitos.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::env;

use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::ui_consts::FOOTER_INDENT_COLS;

const SHORTCUT_HEIGHT: u16 = 11;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FooterMode {
    HistorySearch,
    QuitShortcutReminder,
    ShortcutOverlay,
    EscHint,
    ComposerEmpty,
    ComposerHasDraft,
}

#[derive(Debug)]
pub(crate) struct FooterProps {
    mode: FooterMode,
    left: String,
    context: Option<String>,
}

impl FooterProps {
    pub(crate) fn from_app(app: &App) -> Self {
        let mode = if app.bottom_pane().shortcuts_open() {
            FooterMode::ShortcutOverlay
        } else if app.history_search_open() {
            FooterMode::HistorySearch
        } else if app.quit_confirmation() {
            FooterMode::QuitShortcutReminder
        } else if app.esc_backtrack_hint() {
            FooterMode::EscHint
        } else if app.input().is_empty() {
            FooterMode::ComposerEmpty
        } else {
            FooterMode::ComposerHasDraft
        };
        let left = match mode {
            FooterMode::QuitShortcutReminder => "ctrl + c again to quit".to_owned(),
            FooterMode::EscHint => "esc again to edit previous message".to_owned(),
            FooterMode::HistorySearch => {
                let query = app.history_search_query();
                if app.history_search_has_match() {
                    format!("reverse-i-search: {query}")
                } else {
                    format!("reverse-i-search: {query} (no match)")
                }
            }
            FooterMode::ComposerHasDraft if app.turn_active() => "tab to queue message".to_owned(),
            FooterMode::ComposerEmpty => "? for shortcuts".to_owned(),
            FooterMode::ShortcutOverlay | FooterMode::ComposerHasDraft => String::new(),
        };
        let context = app
            .context_usage()
            .map(|usage| format_context(usage.used_tokens, usage.context_window));
        Self {
            mode,
            left,
            context,
        }
    }
}

pub(crate) fn desired_height(app: &App, _width: u16) -> u16 {
    if app.bottom_pane().shortcuts_open() {
        SHORTCUT_HEIGHT
    } else {
        1
    }
}

fn session_info(app: &App) -> String {
    let model = app.session_model().unwrap_or("unavailable");
    let provider = app.session_provider().unwrap_or("unavailable");
    let directory = env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "unavailable".to_owned());
    format!("model: {model}  provider: {provider}  directory: {directory}")
}

pub(crate) fn render(app: &App, area: Rect, buffer: &mut Buffer) {
    if area.is_empty() {
        return;
    }
    let props = FooterProps::from_app(app);
    if props.mode == FooterMode::ShortcutOverlay {
        render_shortcut_overlay(app, area, buffer);
        return;
    }

    let context_width = props.context.as_deref().map_or(0, UnicodeWidthStr::width);
    let mut left = props.left.clone();
    let mut show_context = props.context.is_some();
    if !fits(area.width, &left, context_width) {
        if props.mode == FooterMode::ComposerHasDraft && left == "tab to queue message" {
            left = "tab to queue".to_owned();
        }
        show_context = fits(area.width, &left, context_width);
        if !show_context && props.mode == FooterMode::ComposerEmpty {
            show_context = false;
        }
        if !fits(area.width, &left, 0) {
            left.clear();
        }
    }

    let info = session_info(app);
    let left_width = FOOTER_INDENT_COLS + UnicodeWidthStr::width(left.as_str());
    let info_width = UnicodeWidthStr::width(info.as_str());
    let show_info = left_width + info_width + context_width + 4 <= usize::from(area.width);
    let left_span = Span::styled(
        format!("{}{}", " ".repeat(FOOTER_INDENT_COLS), left),
        Style::default().dim(),
    );
    let mut line = Line::from(left_span);
    let mut line_width = left_width;
    if show_info {
        let padding = usize::from(area.width)
            .saturating_sub(
                left_width + info_width + if show_context { context_width + 2 } else { 0 },
            )
            .max(2);
        line.push_span(Span::raw(" ".repeat(padding)));
        line.push_span(Span::styled(info, Style::default().dim()));
        line_width += padding + info_width;
    }
    if show_context && let Some(context) = props.context {
        let padding = usize::from(area.width)
            .saturating_sub(line_width + context.width())
            .max(1);
        line.push_span(Span::raw(" ".repeat(padding)));
        line.push_span(Span::styled(context, Style::default().dim()));
    }
    line.render(area, buffer);
}

pub(crate) fn render_status_line(app: &App, area: Rect, buffer: &mut Buffer) {
    let Some(line) = status_line(app) else {
        return;
    };
    let line_area = Rect::new(
        area.x.saturating_add(FOOTER_INDENT_COLS as u16),
        area.y,
        area.width.saturating_sub(FOOTER_INDENT_COLS as u16),
        area.height,
    );
    line.render(line_area, buffer);
}

fn status_line(app: &App) -> Option<Line<'static>> {
    match app.status() {
        crate::app::Status::Ready => None,
        crate::app::Status::Thinking | crate::app::Status::Executing => {
            let activity = app
                .current_activity()
                .unwrap_or_else(|| "Thinking".to_owned());
            Some(
                Line::from(Span::styled(
                    format!(
                        "• {activity} ({} • esc to interrupt)",
                        format_duration(app.working_seconds())
                    ),
                    Style::default(),
                ))
                .dim(),
            )
        }
        crate::app::Status::Error(message) => Some(
            Line::from(Span::styled(
                format!("! {message}"),
                Style::default().fg(Color::Red),
            ))
            .dim(),
        ),
    }
}

/// Formata a duração do turno em unidades compactas: `42s`, `1m 13s`, `1h 4m`.
fn format_duration(total_seconds: u64) -> String {
    let seconds = total_seconds % 60;
    let minutes = (total_seconds / 60) % 60;
    let hours = total_seconds / 3600;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

fn fits(width: u16, left: &str, right_width: usize) -> bool {
    FOOTER_INDENT_COLS + UnicodeWidthStr::width(left) + usize::from(right_width > 0) + right_width
        <= usize::from(width)
}

fn render_shortcut_overlay(app: &App, area: Rect, buffer: &mut Buffer) {
    let queue_hint = if app.turn_active() {
        "tab to queue message"
    } else {
        "tab to submit message"
    };
    let rows = [
        ("  / for commands", ""),
        ("  shift + enter for newline", queue_hint),
        ("  @ for file paths", ""),
        (
            "  ctrl + g to edit in external editor",
            "esc again to edit previous message",
        ),
        ("  ctrl + r search history", "ctrl + c to exit"),
        ("  ctrl + t to view transcript", ""),
        ("", ""),
        ("", ""),
        ("", ""),
    ];
    let first_row = area.y + area.height.saturating_sub(rows.len() as u16);
    for (index, (left, right)) in rows.into_iter().enumerate() {
        let y = first_row + u16::try_from(index).unwrap_or(u16::MAX);
        if y >= area.bottom() {
            break;
        }
        let left_width = usize::from(area.width) / 2;
        Line::from(vec![
            Span::styled(left, Style::default().dim()),
            Span::raw(" ".repeat(left_width.saturating_sub(left.width()))),
            Span::styled(right, Style::default().dim()),
        ])
        .render(Rect::new(area.x, y, area.width, 1), buffer);
    }
    if area.height > 1 {
        buffer.set_span(
            area.x,
            area.y + 1,
            &Span::styled("›", Style::default().fg(Color::Yellow).bold()),
            1,
        );
        buffer.set_span(
            area.x + FOOTER_INDENT_COLS as u16,
            area.y + 1,
            &Span::styled("Ask Atlas to do anything", Style::default().dim()),
            area.width.saturating_sub(FOOTER_INDENT_COLS as u16),
        );
    }
}

fn format_context(used: u64, window: u64) -> String {
    let remaining = window.saturating_sub(used).saturating_mul(100) / window.max(1);
    format!(
        "{}/{} ({}% left)",
        format_tokens_compact(used),
        format_tokens_compact(window),
        remaining
    )
}

fn format_tokens_compact(value: u64) -> String {
    if value < 1_000 {
        return value.to_string();
    }

    let value_f64 = value as f64;
    let (scaled, suffix) = if value >= 1_000_000_000 {
        (value_f64 / 1_000_000_000.0, "b")
    } else if value >= 1_000_000 {
        (value_f64 / 1_000_000.0, "m")
    } else {
        (value_f64 / 1_000.0, "k")
    };
    let decimals = if scaled < 10.0 {
        2
    } else if scaled < 100.0 {
        1
    } else {
        0
    };
    let mut formatted = format!("{scaled:.decimals$}");
    if formatted.contains('.') {
        while formatted.ends_with('0') {
            formatted.pop();
        }
        if formatted.ends_with('.') {
            formatted.pop();
        }
    }
    format!("{formatted}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::FooterMode;
    use crate::app::App;
    use crate::runtime::RuntimeEvent;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    #[test]
    fn shortcut_overlay_height_matches_composer_layout() {
        let mut app = App::new("footer-height".to_owned());
        app.open_shortcuts();
        assert_eq!(super::desired_height(&app, 80), 11);
    }

    #[test]
    fn shortcut_overlay_omits_unsupported_shell_mode() {
        let mut app = App::new("footer-shell".to_owned());
        app.open_shortcuts();
        let props = super::FooterProps::from_app(&app);
        assert!(!props.left.contains("shell"));
    }

    #[test]
    fn selects_codex_footer_modes_from_composer_state() {
        let app = App::new("footer".to_owned());
        assert_eq!(
            super::FooterProps::from_app(&app).mode,
            FooterMode::ComposerEmpty
        );
    }

    fn status_output(app: &App) -> String {
        let area = Rect::new(0, 0, 80, 1);
        let mut buffer = Buffer::empty(area);
        super::render_status_line(app, area, &mut buffer);
        buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    #[test]
    fn renders_thinking_status_line_from_runtime_turn_state() {
        let mut app = App::new("footer-status".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);

        let output = status_output(&app);
        assert!(output.contains("• Thinking ("));
        assert!(output.contains("esc to interrupt"));
        assert!(!output.contains("Working"));
    }

    #[test]
    fn renders_tool_activity_in_the_status_line() {
        let mut app = App::new("footer-activity".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        app.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
            target: Some("tsconfig.json".to_owned()),
        });

        assert!(status_output(&app).contains("• Reading tsconfig.json ("));

        app.handle_runtime_event(RuntimeEvent::ToolCompleted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
            output: None,
        });
        assert!(status_output(&app).contains("• Thinking ("));
    }

    #[test]
    fn renders_execution_activity_in_the_status_line() {
        let mut app = App::new("footer-exec-activity".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        app.handle_runtime_event(RuntimeEvent::ExecutionStarted {
            execution_id: "exec-1".to_owned(),
            capability: "shell.exec".to_owned(),
            program: "sh".to_owned(),
            args: vec!["-c".to_owned(), "npm test".to_owned()],
            cwd: None,
            target: None,
        });

        assert!(status_output(&app).contains("• Running npm test ("));
    }

    #[test]
    fn formats_turn_duration_in_compact_units() {
        assert_eq!(super::format_duration(0), "0s");
        assert_eq!(super::format_duration(42), "42s");
        assert_eq!(super::format_duration(73), "1m 13s");
        assert_eq!(super::format_duration(3_840), "1h 4m");
    }

    #[test]
    fn renders_error_status_line_without_working_hint() {
        let mut app = App::new("footer-error".to_owned());
        app.handle_runtime_event(RuntimeEvent::Error {
            message: "runtime unavailable".to_owned(),
        });
        let area = Rect::new(0, 0, 80, 1);
        let mut buffer = Buffer::empty(area);

        super::render_status_line(&app, area, &mut buffer);
        let output = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(output.contains("! runtime unavailable"));
        assert!(!output.contains("Thinking ("));
    }

    #[test]
    fn formats_context_as_used_over_window_with_remaining_percentage() {
        assert_eq!(super::format_context(100, 156_000), "100/156k (99% left)");
        assert_eq!(
            super::format_context(1_234, 1_000_000),
            "1.23k/1m (99% left)"
        );
    }

    #[test]
    fn renders_session_info_with_model_provider_and_directory() {
        let mut app = App::new("footer-session-info".to_owned());
        app.handle_runtime_event(RuntimeEvent::SessionUpdated {
            model: "gpt-5.6-luna".to_owned(),
            provider: "opencode-go".to_owned(),
        });
        let output = super::session_info(&app);
        assert!(output.contains("model: gpt-5.6-luna"));
        assert!(output.contains("provider: opencode-go"));
        assert!(output.contains("directory:"));
    }
}
