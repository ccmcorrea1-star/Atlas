use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::app::App;
use crate::bottom_pane::footer;
use crate::bottom_pane::paste_burst::PasteBurst;
use crate::bottom_pane::selection_popup::SelectionPopupState;
use crate::bottom_pane::textarea::TextArea;
use crate::icons;
use crate::ui_consts::BOTTOM_PANE_BOTTOM_PADDING;
use crate::ui_consts::BOTTOM_PANE_FOOTER_GAP;
use crate::ui_consts::BOTTOM_PANE_TRANSCRIPT_GAP;
use crate::ui_consts::action_style;
use crate::ui_consts::composer_secondary_style;
use crate::ui_consts::elevated_surface_style;
use crate::ui_consts::primary_style;
use crate::ui_consts::surface_style;
use crate::wrapping::display_width;
use crate::wrapping::wrap_text;

const COMPOSER_INPUT_INSET: u16 = 2;
const COMPOSER_PROMPT_OFFSET: u16 = 2;
const COMPOSER_VERTICAL_PADDING: u16 = 1;
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
    pub(crate) history_navigation_draft: Option<String>,
    pub(crate) slash_popup_suppressed: bool,
    pub(crate) file_popup_suppressed: bool,
    pub(crate) completion_popup: SelectionPopupState,
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
            history_navigation_draft: None,
            slash_popup_suppressed: false,
            file_popup_suppressed: false,
            completion_popup: SelectionPopupState::default(),
            esc_backtrack_hint: false,
            paste_burst: PasteBurst::default(),
        }
    }
}

pub(crate) fn desired_height(app: &App, width: u16) -> u16 {
    if app.bottom_pane().shortcuts_open() {
        return SHORTCUT_HEIGHT;
    }
    let input_rows = input_rows(app, width);
    let popup_height = completion_popup_height(app, width);
    COMPOSER_VERTICAL_PADDING
        .saturating_mul(2)
        .saturating_add(input_rows)
        .saturating_add(popup_height)
        .saturating_add(BOTTOM_PANE_TRANSCRIPT_GAP)
        .saturating_add(BOTTOM_PANE_FOOTER_GAP)
        .saturating_add(footer::desired_height(app, width))
        .saturating_add(BOTTOM_PANE_BOTTOM_PADDING)
        .max(3)
}

pub(crate) fn render(app: &App, area: Rect, buffer: &mut ratatui::buffer::Buffer) {
    if area.is_empty() {
        return;
    }
    buffer.set_style(area, surface_style());
    if app.bottom_pane().shortcuts_open() {
        footer::render(app, area, buffer);
        return;
    }

    let (input_area, popup_area, footer_area, composer_frame) = layout_areas(app, area);
    buffer.set_style(composer_frame, elevated_surface_style());
    let prompt_style = action_style().add_modifier(Modifier::BOLD);
    if !input_area.is_empty() {
        buffer.set_span(
            input_area.x.saturating_sub(COMPOSER_PROMPT_OFFSET),
            input_area.y,
            &Span::styled(icons::PROMPT, prompt_style),
            1,
        );
    }

    let lines = if app.input().is_empty() {
        vec![Line::from(Span::styled(
            "Ask Atlas to do anything",
            composer_secondary_style(),
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

    if !popup_area.is_empty() {
        render_completion_popup(app, popup_area, buffer);
    }

    footer::render(app, footer_area, buffer);
}

/// Largura util do input: a identidade e o contexto ficam exclusivamente no footer.
fn input_width(width: u16) -> u16 {
    width.saturating_sub(COMPOSER_INPUT_INSET)
}

fn input_rows(app: &App, width: u16) -> u16 {
    let input_width = usize::from(input_width(width)).max(1);
    app.textarea()
        .desired_height(u16::try_from(input_width).unwrap_or(u16::MAX))
        .max(1)
}

fn layout_areas(app: &App, area: Rect) -> (Rect, Rect, Rect, Rect) {
    let popup_height = completion_popup_height(app, area.width);
    let requested_footer_height = if area.height < 4 {
        1
    } else {
        footer::desired_height(app, area.width)
    };
    let frame_height = COMPOSER_VERTICAL_PADDING
        .saturating_mul(2)
        .saturating_add(input_rows(app, area.width))
        .saturating_add(popup_height);
    let can_show_footer = area.height >= frame_height.saturating_add(requested_footer_height);
    let full_spacing = can_show_footer
        && area.height
            >= BOTTOM_PANE_TRANSCRIPT_GAP
                .saturating_add(frame_height)
                .saturating_add(BOTTOM_PANE_FOOTER_GAP)
                .saturating_add(requested_footer_height)
                .saturating_add(BOTTOM_PANE_BOTTOM_PADDING);
    let footer_height = if can_show_footer {
        requested_footer_height
    } else {
        0
    };
    let transcript_gap = if full_spacing {
        BOTTOM_PANE_TRANSCRIPT_GAP
    } else {
        0
    };
    let footer_gap = if full_spacing {
        BOTTOM_PANE_FOOTER_GAP
    } else {
        0
    };
    let bottom_padding = if full_spacing {
        BOTTOM_PANE_BOTTOM_PADDING
    } else {
        0
    };
    let footer_y = area
        .bottom()
        .saturating_sub(bottom_padding)
        .saturating_sub(footer_height);
    let composer_bottom = footer_y.saturating_sub(footer_gap);
    let composer_y = area.y.saturating_add(transcript_gap);
    let composer_frame = Rect::new(
        area.x,
        composer_y,
        area.width,
        composer_bottom.saturating_sub(composer_y),
    );
    let popup_y = composer_frame.bottom().saturating_sub(popup_height);
    let input_bottom_padding = if composer_frame.height
        >= COMPOSER_VERTICAL_PADDING
            .saturating_add(1)
            .saturating_add(popup_height)
            .saturating_add(COMPOSER_VERTICAL_PADDING)
    {
        COMPOSER_VERTICAL_PADDING
    } else {
        0
    };
    let input_area = Rect::new(
        composer_frame.x + COMPOSER_INPUT_INSET,
        composer_frame.y + COMPOSER_VERTICAL_PADDING,
        input_width(composer_frame.width),
        popup_y
            .saturating_sub(composer_frame.y)
            .saturating_sub(COMPOSER_VERTICAL_PADDING)
            .saturating_sub(input_bottom_padding),
    );
    let popup_area = Rect::new(area.x, popup_y, area.width, popup_height);
    let footer_area = Rect::new(area.x, footer_y, area.width, footer_height);
    (input_area, popup_area, footer_area, composer_frame)
}

fn completion_popup_height(app: &App, width: u16) -> u16 {
    let items = app.completion_popup_items();
    let label_width = items
        .iter()
        .map(|(label, _)| display_width(label) + 2)
        .max()
        .unwrap_or(2);
    let description_width = usize::from(width).saturating_sub(label_width + 2).max(1);
    let rows = items
        .iter()
        .map(|(_, description)| wrap_text(description, description_width).len().max(1))
        .sum::<usize>();
    u16::try_from(rows.min(crate::app::MAX_COMPLETION_ROWS)).unwrap_or(u16::MAX)
}

fn render_completion_popup(app: &App, area: Rect, buffer: &mut ratatui::buffer::Buffer) {
    let selected = app.completion_popup_selected_row();
    let items = app.completion_popup_items();
    buffer.set_style(area, elevated_surface_style());
    let label_width = items
        .iter()
        .map(|(label, _)| display_width(label) + 2)
        .max()
        .unwrap_or(2);
    let description_width = usize::from(area.width)
        .saturating_sub(label_width + 2)
        .max(1);
    let mut row = 0usize;
    for (item_index, (label, description)) in items.into_iter().enumerate() {
        let is_selected = selected == Some(item_index);
        let marker = if is_selected { "› " } else { "  " };
        let label_style = if is_selected {
            action_style()
        } else {
            composer_secondary_style()
        };
        let description_style = if is_selected {
            action_style()
        } else {
            primary_style()
        };
        for (line_index, description_line) in wrap_text(&description, description_width)
            .into_iter()
            .enumerate()
        {
            let Ok(y) = u16::try_from(row) else {
                return;
            };
            if y >= area.height {
                return;
            }
            let label_span = if line_index == 0 {
                Span::styled(
                    format!("{marker}{label:<width$}", width = label_width - 2),
                    label_style,
                )
            } else {
                Span::raw(" ".repeat(label_width))
            };
            Line::from(vec![
                label_span,
                Span::styled(format!("  {description_line}"), description_style),
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
            row += 1;
        }
    }
}

pub(crate) fn cursor_position_for(app: &App, area: Rect) -> Option<(u16, u16)> {
    if app.bottom_pane().shortcuts_open() || area.is_empty() {
        return None;
    }
    let (input_area, _, _, _) = layout_areas(app, area);
    if input_area.is_empty() {
        return None;
    }
    app.textarea()
        .cursor_pos_with_state(input_area, app.textarea().state_for_viewport(input_area))
}

#[cfg(test)]
mod tests {
    use super::cursor_position_for;
    use super::render;
    use crate::app::App;
    use crate::runtime::ContextUsage;
    use crate::runtime::RuntimeEvent;
    use crate::ui_consts::COLOR_ACTION;
    use crate::ui_consts::COLOR_SURFACE;
    use crate::ui_consts::COLOR_SURFACE_ELEVATED;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

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
                used_tokens: 2_600,
                context_window: 256_000,
            },
        });
    }

    fn with_session(app: &mut App) {
        app.handle_runtime_event(RuntimeEvent::SessionUpdated {
            model: "gpt-5.6-luna".to_owned(),
            provider: "opencode-go".to_owned(),
        });
    }

    #[test]
    fn matches_codex_composer_surfaces() {
        let mut empty = App::new("snapshot".to_owned());
        empty.set_display_paths("/home/kyle/projetos/Atlas", "/home/kyle");
        with_context(&mut empty);
        insta::assert_snapshot!("composer_empty", rows(&empty, 100, 14));

        let mut draft = App::new("snapshot".to_owned());
        draft.set_display_paths("/home/kyle/projetos/Atlas", "/home/kyle");
        for character in "short".chars() {
            draft.insert_character(character);
        }
        with_session(&mut draft);
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
    fn snapshots_bottom_pane_padding_on_a_normal_terminal() {
        let mut app = App::new("composer-padding".to_owned());
        app.set_display_paths("/home/kyle/projetos/Atlas", "/home/kyle");
        with_session(&mut app);
        with_context(&mut app);
        let height = super::desired_height(&app, 100);
        insta::assert_snapshot!("composer_padding", rows(&app, 100, height));
    }

    #[test]
    fn snapshots_bottom_pane_padding_on_a_narrow_terminal() {
        let mut app = App::new("composer-narrow".to_owned());
        app.set_display_paths("/home/kyle/projetos/Atlas", "/home/kyle");
        with_session(&mut app);
        with_context(&mut app);
        let height = super::desired_height(&app, 50);
        let output = rows(&app, 50, height);
        assert!(output.contains("? shortcuts"));
        assert!(!output.contains("~/projetos/Atlas"));
        insta::assert_snapshot!("composer_narrow", output);
    }

    #[test]
    fn keeps_the_input_and_the_identity_off_the_pane_borders() {
        let mut app = App::new("composer-borders".to_owned());
        app.set_display_paths("/home/kyle/projetos/Atlas", "/home/kyle");
        with_session(&mut app);
        with_context(&mut app);

        let wide = rows(&app, 100, super::desired_height(&app, 100));
        let wide_input = input_row(&wide);
        assert!(wide_input.contains("Ask Atlas to do anything"));

        let narrow = rows(&app, 60, super::desired_height(&app, 60));
        let narrow_input = input_row(&narrow);
        assert!(narrow_input.contains("Ask Atlas to do anything"));

        let tiny = rows(&app, 30, super::desired_height(&app, 30));
        let tiny_input = input_row(&tiny);
        assert!(tiny_input.contains("Ask Atlas to do anything"));
        assert!(!tiny_input.contains("gpt-5.6-luna"));

        let workspace_row = wide
            .lines()
            .find(|line| line.contains("? shortcuts"))
            .expect("workspace row");
        assert!(workspace_row.starts_with("? shortcuts"));
        assert!(workspace_row.ends_with("2.6k/256k · 98% left"));
    }

    fn input_row(output: &str) -> &str {
        output
            .lines()
            .find(|line| line.contains("Ask Atlas to do anything"))
            .expect("input row")
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
                .any(|cell| cell.symbol() == "/" && cell.style().fg == Some(COLOR_ACTION))
        );
    }

    #[test]
    fn snapshots_reverse_history_search_after_paste() {
        let mut app = App::new("history-search-snapshot".to_owned());
        app.set_display_paths("/home/kyle/projetos/Atlas", "/home/kyle");
        app.insert_text("git status");
        assert_eq!(app.submit_input().as_deref(), Some("git status"));
        app.insert_text("draft");
        assert!(app.handle_global_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('r'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
        app.handle_paste("git");
        insta::assert_snapshot!("composer_history_search_pasted_query", rows(&app, 100, 14));
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
    fn keeps_the_composer_input_line_free_of_session_identity() {
        let mut app = App::new("composer-identity".to_owned());
        app.set_display_paths("/home/kyle/projetos/Atlas", "/home/kyle");
        with_session(&mut app);
        with_context(&mut app);

        let output = rows(&app, 100, super::desired_height(&app, 100));
        let input = input_row(&output);
        assert!(!input.contains("gpt-5.6-luna"));
        assert!(!input.contains("opencode-go"));
        assert!(output.contains("2.6k/256k · 98% left"));
    }

    #[test]
    fn scrolls_multiline_composer_to_keep_the_cursor_visible() {
        let mut app = App::new("composer-scroll".to_owned());
        app.insert_text("first\nsecond\nthird");
        let area = ratatui::layout::Rect::new(0, 0, 40, 4);
        let cursor = cursor_position_for(&app, area).expect("cursor should remain visible");
        assert!(cursor.1 < area.bottom());

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

        assert!(!rows(&app, 100, 14).contains("Thinking"));
        assert!(!rows(&app, 100, 14).contains("Running npm test"));
    }

    #[test]
    fn centers_the_composer_frame_above_the_separate_footer() {
        let app = App::new("composer-surface".to_owned());
        let height = super::desired_height(&app, 100);
        let mut terminal = Terminal::new(TestBackend::new(100, height)).unwrap();
        terminal
            .draw(|frame| render(&app, frame.area(), frame.buffer_mut()))
            .unwrap();
        let (_, _, footer_area, composer_frame) =
            super::layout_areas(&app, terminal.backend().buffer().area);

        assert_eq!(composer_frame.x, 0);
        assert_eq!(composer_frame.right(), 100);
        assert_eq!(footer_area.x, 0);
        assert!(composer_frame.bottom() < footer_area.y);
        assert_eq!(
            footer_area.bottom(),
            terminal.backend().buffer().area.bottom()
        );
        assert_eq!(
            terminal.backend().buffer()[(20, composer_frame.y)].bg,
            COLOR_SURFACE_ELEVATED
        );
        assert_eq!(
            terminal.backend().buffer()[(20, footer_area.y)].bg,
            COLOR_SURFACE
        );
    }
}
