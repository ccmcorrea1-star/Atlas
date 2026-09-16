//! Footer rendering adapted from the Codex TUI bottom-pane footer.
//!
//! The footer is deliberately a pure view of composer state. It applies the
//! same width-based fallback order as Codex instead of letting the context
//! indicator overwrite the left-side hint on narrow terminals.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
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

    let left_span = Span::styled(
        format!("{}{}", " ".repeat(FOOTER_INDENT_COLS), left),
        Style::default().dim(),
    );
    let mut line = Line::from(left_span);
    if show_context && let Some(context) = props.context {
        let used = FOOTER_INDENT_COLS + UnicodeWidthStr::width(left.as_str());
        let padding = usize::from(area.width)
            .saturating_sub(used + context.width())
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
        crate::app::Status::Thinking | crate::app::Status::Executing => Some(
            Line::from(Span::styled(
                format!("• Working ({}s • esc to interrupt)", app.working_seconds()),
                Style::default(),
            ))
            .dim(),
        ),
        crate::app::Status::Error(message) => Some(
            Line::from(Span::styled(
                format!("! {message}"),
                Style::default().fg(Color::Red),
            ))
            .dim(),
        ),
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
            &Span::styled("Ask Codex to do anything", Style::default().dim()),
            area.width.saturating_sub(FOOTER_INDENT_COLS as u16),
        );
    }
}

fn format_context(used: u64, window: u64) -> String {
    let remaining = window.saturating_sub(used).saturating_mul(100) / window.max(1);
    format!("{remaining}% context left")
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

    #[test]
    fn renders_working_status_line_from_runtime_turn_state() {
        let mut app = App::new("footer-status".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        let area = Rect::new(0, 0, 80, 1);
        let mut buffer = Buffer::empty(area);

        super::render_status_line(&app, area, &mut buffer);
        let output = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(output.contains("Working ("));
        assert!(output.contains("esc to interrupt"));
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
        assert!(!output.contains("Working ("));
    }
}
