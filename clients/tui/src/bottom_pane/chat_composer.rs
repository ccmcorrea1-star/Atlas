use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::app::App;
use crate::bottom_pane::footer;
use crate::bottom_pane::paste_burst::PasteBurst;
use crate::bottom_pane::textarea::TextArea;
use crate::ui_consts::LIVE_PREFIX_COLS;

const PROMPT: &str = "›";
const COMPOSER_TOP: u16 = 1;
const SHORTCUT_HEIGHT: u16 = 11;

/// State owned by the canonical bottom-pane composer.
///
/// Runtime and transcript coordination stays in `App`; editing modes and
/// draft-local interaction state live here so new bottom-pane views can reuse
/// the same composer seam.
#[derive(Debug)]
pub(crate) struct ChatComposer {
    pub(crate) textarea: TextArea,
    pub(crate) history_search_open: bool,
    pub(crate) history_search_query: String,
    pub(crate) history_search_draft: String,
    pub(crate) history_search_matches: Vec<usize>,
    pub(crate) history_search_match: Option<usize>,
    pub(crate) history_entries: Vec<String>,
    pub(crate) history_index: Option<usize>,
    pub(crate) slash_popup_suppressed: bool,
    pub(crate) file_popup_suppressed: bool,
    pub(crate) completion_selection: usize,
    pub(crate) esc_backtrack_hint: bool,
    pub(crate) paste_burst: PasteBurst,
}

impl ChatComposer {
    pub(crate) fn new() -> Self {
        Self {
            textarea: TextArea::new(),
            history_search_open: false,
            history_search_query: String::new(),
            history_search_draft: String::new(),
            history_search_matches: Vec::new(),
            history_search_match: None,
            history_entries: Vec::new(),
            history_index: None,
            slash_popup_suppressed: false,
            file_popup_suppressed: false,
            completion_selection: 0,
            esc_backtrack_hint: false,
            paste_burst: PasteBurst::default(),
        }
    }
}

pub(crate) fn desired_height(app: &App, width: u16) -> u16 {
    if app.shortcuts_open() {
        return SHORTCUT_HEIGHT;
    }
    let input_width = usize::from(width.saturating_sub(LIVE_PREFIX_COLS + 1)).max(1);
    let input_rows = app
        .textarea()
        .desired_height(u16::try_from(input_width).unwrap_or(u16::MAX))
        .max(1);
    let popup_height = completion_popup_height(app);
    (COMPOSER_TOP + input_rows + popup_height + footer::desired_height(app, width) + 1).max(4)
}

pub(crate) fn render(app: &App, area: Rect, buffer: &mut ratatui::buffer::Buffer) {
    if area.is_empty() || area.height <= COMPOSER_TOP {
        return;
    }
    if app.shortcuts_open() {
        footer::render(app, area, buffer);
        return;
    }

    render_status(app, area, buffer);

    let popup_height = completion_popup_height(app);
    let input_area = Rect {
        x: area.x + LIVE_PREFIX_COLS,
        y: area.y + COMPOSER_TOP,
        width: area.width.saturating_sub(LIVE_PREFIX_COLS + 1),
        height: area.height.saturating_sub(
            COMPOSER_TOP + popup_height + footer::desired_height(app, area.width) + 1,
        ),
    };
    let prompt_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    buffer.set_span(
        input_area.x.saturating_sub(LIVE_PREFIX_COLS),
        input_area.y,
        &Span::styled(PROMPT, prompt_style),
        1,
    );

    let lines = if app.input().is_empty() {
        vec![Line::from(Span::styled(
            "Ask Codex to do anything",
            Style::default().dim(),
        ))]
    } else {
        Vec::new()
    };
    if lines.is_empty() {
        app.textarea().render(
            input_area,
            buffer,
            app.textarea().state_for_viewport(input_area),
        );
    } else {
        Paragraph::new(lines).render(input_area, buffer);
    }

    if popup_height > 0 {
        let popup_area = Rect {
            x: area.x + LIVE_PREFIX_COLS,
            y: input_area.y + input_area.height,
            width: area.width.saturating_sub(LIVE_PREFIX_COLS),
            height: popup_height,
        };
        render_completion_popup(app, popup_area, buffer);
    }

    let footer_area = Rect {
        x: area.x,
        y: area.y
            + area
                .height
                .saturating_sub(footer::desired_height(app, area.width)),
        width: area.width,
        height: footer::desired_height(app, area.width),
    };
    footer::render(app, footer_area, buffer);
}

fn completion_popup_height(app: &App) -> u16 {
    u16::try_from(app.completion_popup_items().len()).unwrap_or(u16::MAX)
}

fn render_completion_popup(app: &App, area: Rect, buffer: &mut ratatui::buffer::Buffer) {
    let selected = app.completion_popup_selected_row();
    for (row, (label, description)) in app.completion_popup_items().into_iter().enumerate() {
        let Ok(y) = u16::try_from(row) else {
            break;
        };
        if y >= area.height {
            break;
        }
        let is_selected = selected == Some(row);
        let marker = if is_selected { "› " } else { "  " };
        let label_style = if is_selected {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::Yellow)
        };
        let description_style = if is_selected {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };
        Line::from(vec![
            Span::styled(format!("{marker}{label}"), label_style),
            Span::styled(format!("  {description}"), description_style),
        ])
        .render(
            Rect {
                x: area.x,
                y: area.y + y,
                width: area.width,
                height: 1,
            },
            buffer,
        );
    }
}

pub(crate) fn cursor_position_for(app: &App, area: Rect) -> Option<(u16, u16)> {
    if app.shortcuts_open() || area.is_empty() {
        return None;
    }
    let popup_height = completion_popup_height(app);
    let input_area = Rect {
        x: area.x + LIVE_PREFIX_COLS,
        y: area.y + COMPOSER_TOP,
        width: area.width.saturating_sub(LIVE_PREFIX_COLS + 1),
        height: area.height.saturating_sub(
            COMPOSER_TOP + popup_height + footer::desired_height(app, area.width) + 1,
        ),
    };
    if input_area.is_empty() {
        return None;
    }
    app.textarea()
        .cursor_pos_with_state(input_area, app.textarea().state_for_viewport(input_area))
}

fn render_status(app: &App, area: Rect, buffer: &mut ratatui::buffer::Buffer) {
    let (label, style) = match app.status() {
        crate::app::Status::Ready => return,
        crate::app::Status::Thinking | crate::app::Status::Executing => (
            format!("• Working ({}s • esc to interrupt)", app.working_seconds()),
            Style::default(),
        ),
        crate::app::Status::Error(message) => {
            (format!("! {message}"), Style::default().fg(Color::Red))
        }
    };
    Line::from(Span::styled(label, style.dim())).render(
        Rect {
            x: area.x + LIVE_PREFIX_COLS,
            y: area.y,
            width: area.width.saturating_sub(LIVE_PREFIX_COLS),
            height: 1,
        },
        buffer,
    );
}

#[cfg(test)]
mod tests {
    use super::cursor_position_for;
    use super::render;
    use crate::app::App;
    use crate::runtime::ContextUsage;
    use crate::runtime::RuntimeEvent;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    fn rows(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render(app, frame.area(), frame.buffer_mut()))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn with_context(app: &mut App) {
        app.handle_runtime_event(RuntimeEvent::ContextUpdated {
            context: ContextUsage {
                used_tokens: 0,
                context_window: 256_000,
            },
        });
    }

    #[test]
    fn matches_codex_composer_surfaces() {
        let mut empty = App::new("snapshot".to_owned());
        with_context(&mut empty);
        insta::assert_snapshot!("composer_empty", rows(&empty, 100, 14));

        let mut draft = App::new("snapshot".to_owned());
        for character in "short".chars() {
            draft.insert_character(character);
        }
        with_context(&mut draft);
        insta::assert_snapshot!("composer_draft", rows(&draft, 100, 14));

        let mut shortcuts = App::new("snapshot".to_owned());
        shortcuts.open_shortcuts();
        insta::assert_snapshot!(
            "composer_shortcuts",
            rows(&shortcuts, 100, super::desired_height(&shortcuts, 100))
        );
    }

    #[test]
    fn renders_slash_command_popup_above_footer() {
        let mut app = App::new("slash-popup-render".to_owned());
        app.insert_text("/");
        let output = rows(&app, 100, 14);
        assert!(output.contains("/help"));
        assert!(output.contains("show keyboard shortcuts"));
        assert!(output.contains("/quit"));
    }

    #[test]
    fn renders_the_selected_completion_marker_on_the_active_row() {
        let mut app = App::new("slash-popup-selection-render".to_owned());
        app.insert_text("/");
        assert!(app.handle_global_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        )));

        let output = rows(&app, 100, 14);
        let popup_rows = output
            .lines()
            .filter(|line| line.contains("/help") || line.contains("/quit"))
            .collect::<Vec<_>>();
        assert_eq!(popup_rows.len(), 2);
        assert!(!popup_rows[0].contains("› /help"));
        assert!(popup_rows[1].contains("› /quit"));
    }

    #[test]
    fn applies_focus_color_to_the_selected_completion_row() {
        let mut app = App::new("slash-popup-selection-style".to_owned());
        app.insert_text("/");
        assert!(app.handle_global_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        )));

        let mut terminal = Terminal::new(TestBackend::new(100, 14)).unwrap();
        terminal
            .draw(|frame| render(&app, frame.area(), frame.buffer_mut()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(
            buffer
                .content
                .iter()
                .any(|cell| cell.symbol() == "/" && cell.style().fg == Some(Color::Cyan))
        );
    }

    #[test]
    fn keeps_prompt_inside_the_composer_area() {
        let app = App::new("snapshot".to_owned());
        let mut terminal = Terminal::new(TestBackend::new(10, 4)).unwrap();
        terminal
            .draw(|frame| render(&app, frame.area(), frame.buffer_mut()))
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(0, 1)].symbol(), "›");
    }

    #[test]
    fn scrolls_multiline_composer_to_keep_the_cursor_visible() {
        let mut app = App::new("composer-scroll".to_owned());
        app.insert_text("first\nsecond\nthird");
        let area = ratatui::layout::Rect::new(0, 0, 40, 5);
        let cursor = cursor_position_for(&app, area).expect("cursor should remain visible");
        assert_eq!(cursor.1, 2);

        let output = rows(&app, area.width, area.height);
        assert!(output.contains("second"));
        assert!(output.contains("third"));
        assert!(!output.contains("first"));
    }

    #[test]
    fn shortcut_overlay_switches_tab_hint_while_turn_is_active() {
        let mut app = App::new("shortcut-queue".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        app.open_shortcuts();
        let output = rows(&app, 100, super::desired_height(&app, 100));
        assert!(output.contains("tab to queue message"));
        assert!(!output.contains("tab to submit message"));
    }

    #[test]
    fn uses_codex_working_status_for_active_execution() {
        let mut app = App::new("status".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);

        assert!(rows(&app, 100, 14).contains("• Working ("));
    }
}
