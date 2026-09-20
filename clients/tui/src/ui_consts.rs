//! Tokens visuais e constantes de layout compartilhados pela TUI.

use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;

pub(crate) const COLOR_SURFACE: Color = Color::Rgb(9, 11, 11);
pub(crate) const COLOR_SURFACE_ELEVATED: Color = Color::Rgb(27, 48, 53);
pub(crate) const COLOR_SURFACE_USER: Color = Color::Rgb(11, 18, 20);
pub(crate) const COLOR_SURFACE_DIFF: Color = Color::Rgb(11, 18, 20);
pub(crate) const COLOR_SURFACE_DIFF_ADDED: Color = Color::Rgb(24, 58, 38);
pub(crate) const COLOR_SURFACE_DIFF_REMOVED: Color = Color::Rgb(62, 34, 38);

pub(crate) const COLOR_TEXT_PRIMARY: Color = Color::Rgb(235, 237, 223);
pub(crate) const COLOR_TEXT_SECONDARY: Color = Color::Rgb(85, 124, 133);
pub(crate) const COLOR_COMPOSER_SECONDARY: Color = Color::Rgb(116, 154, 160);
pub(crate) const COLOR_ACTION: Color = Color::Rgb(119, 176, 184);
pub(crate) const COLOR_SUCCESS: Color = Color::Green;
pub(crate) const COLOR_RUNNING: Color = Color::Rgb(119, 176, 184);
pub(crate) const COLOR_WARNING: Color = Color::Rgb(167, 219, 223);
pub(crate) const COLOR_ERROR: Color = Color::Red;
pub(crate) const COLOR_THINKING: Color = Color::Rgb(167, 219, 223);
pub(crate) const COLOR_THOUGHT: Color = Color::Rgb(85, 124, 133);
pub(crate) const COLOR_THOUGHT_BODY: Color = Color::Rgb(49, 74, 80);

pub(crate) fn primary_style() -> Style {
    Style::default().fg(COLOR_TEXT_PRIMARY)
}

pub(crate) fn secondary_style() -> Style {
    Style::default()
        .fg(COLOR_TEXT_SECONDARY)
        .add_modifier(Modifier::DIM)
}

pub(crate) fn composer_secondary_style() -> Style {
    Style::default()
        .fg(COLOR_COMPOSER_SECONDARY)
        .add_modifier(Modifier::DIM)
}

pub(crate) fn action_style() -> Style {
    Style::default().fg(COLOR_ACTION)
}

pub(crate) fn running_style() -> Style {
    Style::default()
        .fg(COLOR_RUNNING)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn success_style() -> Style {
    Style::default()
        .fg(COLOR_SUCCESS)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn warning_style() -> Style {
    Style::default().fg(COLOR_WARNING)
}

pub(crate) fn thinking_style() -> Style {
    Style::default().fg(COLOR_THINKING)
}

pub(crate) fn thought_style() -> Style {
    Style::default().fg(COLOR_THOUGHT)
}

pub(crate) fn thought_body_style() -> Style {
    Style::default().fg(COLOR_THOUGHT_BODY)
}

pub(crate) fn error_style() -> Style {
    Style::default().fg(COLOR_ERROR)
}

pub(crate) fn surface_style() -> Style {
    Style::default().bg(COLOR_SURFACE)
}

pub(crate) fn elevated_surface_style() -> Style {
    Style::default().bg(COLOR_SURFACE_ELEVATED)
}

pub(crate) fn user_surface_style() -> Style {
    primary_style().bg(COLOR_SURFACE_USER)
}

/// Inset compartilhado pela coluna principal da conversa.
pub(crate) const CONVERSATION_HORIZONTAL_INSET: u16 = 2;
/// Inset horizontal compartilhado pelo composer e pelo footer do painel inferior.
pub(crate) const BOTTOM_PANE_HORIZONTAL_INSET: u16 = CONVERSATION_HORIZONTAL_INSET;
pub(crate) const BOTTOM_PANE_TRANSCRIPT_GAP: u16 = 1;
pub(crate) const BOTTOM_PANE_FOOTER_GAP: u16 = 1;
pub(crate) const BOTTOM_PANE_BOTTOM_PADDING: u16 = 1;
pub(crate) const TRANSCRIPT_HINT: &str = "ctrl + t to view transcript";

/// Area interna do painel inferior, com o mesmo padding nos dois lados.
///
/// O composer e o footer desenham dentro dela para que nenhum texto encoste nas
/// bordas do painel nem no terminal.
pub(crate) fn bottom_pane_inner_area(area: Rect) -> Rect {
    let inset = BOTTOM_PANE_HORIZONTAL_INSET.min(area.width / 2);
    Rect::new(
        area.x.saturating_add(inset),
        area.y,
        area.width.saturating_sub(inset.saturating_mul(2)),
        area.height,
    )
}
