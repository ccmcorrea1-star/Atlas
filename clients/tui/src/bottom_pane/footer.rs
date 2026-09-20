//! Renderizacao do footer adaptada do painel inferior da TUI do Codex.
//!
//! O footer e uma view pura do estado do composer. Ele aplica a mesma ordem de
//! fallback por largura do Codex sem deixar o contexto cobrir a dica em terminais estreitos.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use ratatui::style::Modifier;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::ui_consts::error_style;
use crate::ui_consts::secondary_style;
use crate::ui_consts::warning_style;

const SHORTCUT_HEIGHT: u16 = 11;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FooterMode {
    HistorySearch,
    QuitShortcutReminder,
    ShortcutOverlay,
    Connecting,
    EscHint,
    ComposerEmpty,
    ComposerHasDraft,
}

#[derive(Debug)]
pub(crate) struct FooterProps {
    mode: FooterMode,
    left: String,
    session: Option<String>,
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
        } else if !app.runtime_connected() {
            FooterMode::Connecting
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
            FooterMode::Connecting => "Connecting…".to_owned(),
            FooterMode::ComposerEmpty | FooterMode::ComposerHasDraft => "? shortcuts".to_owned(),
            FooterMode::ShortcutOverlay => String::new(),
        };
        let session = session_info(app);
        let context = app
            .context_usage()
            .map(|usage| format_context(usage.used_tokens, usage.context_window));
        Self {
            mode,
            left,
            session,
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

fn session_info(app: &App) -> Option<String> {
    match (app.session_model(), app.session_provider()) {
        (Some(model), Some(provider)) => Some(format!("{model} · {provider}")),
        (Some(model), None) | (None, Some(model)) => Some(model.to_owned()),
        (None, None) => None,
    }
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

    let left = props.left;
    let left_width = UnicodeWidthStr::width(left.as_str());
    let available_right = usize::from(area.width).saturating_sub(left_width + 1);
    let right = metadata_line(props.session, props.context, available_right);
    let right_width = right.as_ref().map_or(0, Line::width);
    let mut line = Line::from(Span::styled(left, secondary_style()));
    if let Some(right) = right {
        let padding = usize::from(area.width)
            .saturating_sub(left_width + right_width)
            .max(1);
        line.push_span(Span::raw(" ".repeat(padding)));
        line.extend(right.spans);
    }
    line.render(area, buffer);
}

fn metadata_line(
    session: Option<String>,
    context: Option<String>,
    available_width: usize,
) -> Option<Line<'static>> {
    let separator_width = UnicodeWidthStr::width(" · ");
    match (session, context) {
        (Some(session), Some(context))
            if UnicodeWidthStr::width(session.as_str())
                .saturating_add(separator_width)
                .saturating_add(UnicodeWidthStr::width(context.as_str()))
                <= available_width =>
        {
            Some(Line::from(vec![
                Span::styled(session, secondary_style()),
                Span::styled(" · ", secondary_style()),
                Span::styled(context, secondary_style()),
            ]))
        }
        (Some(session), Some(context)) => {
            if UnicodeWidthStr::width(context.as_str()) <= available_width {
                Some(Line::from(Span::styled(context, secondary_style())))
            } else if UnicodeWidthStr::width(session.as_str()) <= available_width {
                Some(Line::from(Span::styled(session, secondary_style())))
            } else {
                None
            }
        }
        (Some(session), None) if UnicodeWidthStr::width(session.as_str()) <= available_width => {
            Some(Line::from(Span::styled(session, secondary_style())))
        }
        (None, Some(context)) if UnicodeWidthStr::width(context.as_str()) <= available_width => {
            Some(Line::from(Span::styled(context, secondary_style())))
        }
        _ => None,
    }
}

#[allow(dead_code)]
pub(crate) fn render_status_line(app: &App, area: Rect, buffer: &mut Buffer) {
    let Some(line) = status_line(app, area.width) else {
        return;
    };
    line.render(area, buffer);
}

#[allow(dead_code)]
fn status_line(app: &App, width: u16) -> Option<Line<'static>> {
    match app.status() {
        crate::app::Status::Ready => None,
        crate::app::Status::Thinking | crate::app::Status::Executing => {
            (width > 0).then(|| Line::from(Span::styled("esc to interrupt", secondary_style())))
        }
        crate::app::Status::Error(message) => Some(Line::from(Span::styled(
            format!("! {message}"),
            error_style(),
        ))),
    }
}

#[cfg(test)]
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

fn render_shortcut_overlay(app: &App, area: Rect, buffer: &mut Buffer) {
    let queue_hint = if app.turn_active() {
        "tab to queue message"
    } else {
        "tab to submit message"
    };
    let rows = [
        ("/ for commands", ""),
        ("shift + enter for newline", queue_hint),
        ("@ for file paths", ""),
        (
            "ctrl + g to edit in external editor",
            "esc again to edit previous message",
        ),
        ("ctrl + r search history", "ctrl + c to exit"),
        ("ctrl + t to view transcript", ""),
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
            Span::styled(left, secondary_style()),
            Span::raw(" ".repeat(left_width.saturating_sub(left.width()))),
            Span::styled(right, secondary_style()),
        ])
        .render(Rect::new(area.x, y, area.width, 1), buffer);
    }
    if area.height > 1 {
        buffer.set_span(
            area.x,
            area.y + 1,
            &Span::styled("›", warning_style().add_modifier(Modifier::BOLD)),
            1,
        );
        buffer.set_span(
            area.x + 2,
            area.y + 1,
            &Span::styled("Ask Atlas to do anything", secondary_style()),
            area.width.saturating_sub(2),
        );
    }
}

fn format_context(used: u64, window: u64) -> String {
    let remaining = window.saturating_sub(used).saturating_mul(100) / window.max(1);
    format!(
        "{}/{} · {remaining}% left",
        format_token_count(used),
        format_token_count(window)
    )
}

fn format_token_count(tokens: u64) -> String {
    if tokens < 1_000 {
        return tokens.to_string();
    }
    if tokens < 1_000_000 {
        return format_compact_count(tokens, 1_000, 'k');
    }
    format_compact_count(tokens, 1_000_000, 'm')
}

fn format_compact_count(tokens: u64, unit: u64, suffix: char) -> String {
    let whole = tokens / unit;
    let remainder = tokens % unit;
    if remainder == 0 || whole >= 100 {
        return format!("{whole}{suffix}");
    }
    let tenths = (remainder.saturating_mul(10) + unit / 2) / unit;
    if tenths >= 10 {
        format!("{}{}", whole + 1, suffix)
    } else {
        format!("{whole}.{tenths}{suffix}")
    }
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
    fn starts_ready_for_the_first_turn() {
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
        assert!(output.contains("esc to interrupt"));
        assert!(!output.contains("Thinking"));
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

        assert_eq!(status_output(&app).trim(), "esc to interrupt");

        app.handle_runtime_event(RuntimeEvent::ToolCompleted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
            output: None,
        });
        assert!(status_output(&app).contains("esc to interrupt"));
        assert!(!status_output(&app).contains("Thinking"));
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

        assert_eq!(status_output(&app).trim(), "esc to interrupt");
    }

    #[test]
    fn snapshots_active_footer_hierarchy() {
        let mut app = App::new("footer-snapshot".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        app.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
            target: Some("src/main.rs".to_owned()),
        });

        insta::assert_snapshot!("footer_activity", status_output(&app));
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
    fn formats_context_as_remaining_percentage() {
        assert_eq!(super::format_context(100, 156_000), "100/156k · 99% left");
        assert_eq!(
            super::format_context(12_000, 200_000),
            "12k/200k · 94% left"
        );
        assert_eq!(
            super::format_context(1_234, 1_000_000),
            "1.2k/1m · 99% left"
        );
    }

    #[test]
    fn renders_session_info_with_model_and_provider_only() {
        let mut app = App::new("footer-session-info".to_owned());
        app.handle_runtime_event(RuntimeEvent::SessionUpdated {
            model: "gpt-5.6-luna".to_owned(),
            provider: "opencode-go".to_owned(),
        });
        let output = super::session_info(&app);
        assert_eq!(output.as_deref(), Some("gpt-5.6-luna · opencode-go"));
        assert!(!output.as_deref().unwrap().contains("directory:"));
    }

    #[test]
    fn ready_footer_omits_missing_session_metadata() {
        let app = App::new("footer-ready".to_owned());
        let props = super::FooterProps::from_app(&app);
        assert_eq!(props.mode, FooterMode::ComposerEmpty);
        assert_eq!(props.left, "? shortcuts");
        assert!(props.session.is_none());
    }
}
