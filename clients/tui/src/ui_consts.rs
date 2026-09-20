//! Tokens visuais e constantes de layout compartilhados pela TUI.

use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;

pub(crate) const COLOR_SURFACE: Color = Color::Rgb(18, 21, 26);
pub(crate) const COLOR_SURFACE_ELEVATED: Color = Color::Rgb(29, 34, 41);
pub(crate) const COLOR_SURFACE_USER: Color = Color::Rgb(40, 48, 59);
pub(crate) const COLOR_SURFACE_DIFF: Color = Color::Rgb(25, 29, 35);
pub(crate) const COLOR_SURFACE_DIFF_ADDED: Color = Color::Rgb(24, 58, 38);
pub(crate) const COLOR_SURFACE_DIFF_REMOVED: Color = Color::Rgb(62, 34, 38);

pub(crate) const COLOR_TEXT_PRIMARY: Color = Color::White;
pub(crate) const COLOR_TEXT_SECONDARY: Color = Color::Gray;
pub(crate) const COLOR_ACTION: Color = Color::Cyan;
pub(crate) const COLOR_SUCCESS: Color = Color::Green;
pub(crate) const COLOR_RUNNING: Color = Color::Cyan;
pub(crate) const COLOR_WARNING: Color = Color::Yellow;
pub(crate) const COLOR_ERROR: Color = Color::Red;

pub(crate) fn primary_style() -> Style {
    Style::default().fg(COLOR_TEXT_PRIMARY)
}

pub(crate) fn secondary_style() -> Style {
    Style::default()
        .fg(COLOR_TEXT_SECONDARY)
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

/// Colunas reservadas pela margem esquerda e pelo prefixo do composer.
pub(crate) const LIVE_PREFIX_COLS: u16 = 2;
pub(crate) const FOOTER_INDENT_COLS: usize = LIVE_PREFIX_COLS as usize;
pub(crate) const TRANSCRIPT_HINT: &str = "ctrl + t to view transcript";
