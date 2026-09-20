//! Tokens visuais e constantes de layout compartilhados pela TUI.

use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;

pub(crate) const COLOR_SURFACE: Color = Color::Rgb(20, 20, 20);
pub(crate) const COLOR_SURFACE_ELEVATED: Color = Color::Rgb(32, 33, 37);
pub(crate) const COLOR_SURFACE_USER: Color = COLOR_SURFACE_ELEVATED;
pub(crate) const COLOR_SURFACE_DIFF: Color = COLOR_SURFACE_ELEVATED;

pub(crate) const COLOR_TEXT_PRIMARY: Color = Color::Rgb(233, 246, 225);
pub(crate) const COLOR_TEXT_SECONDARY: Color = Color::Rgb(130, 125, 125);
pub(crate) const COLOR_COMPOSER_SECONDARY: Color = COLOR_TEXT_SECONDARY;
pub(crate) const COLOR_ACTION: Color = Color::Rgb(60, 83, 206);
pub(crate) const COLOR_SUCCESS: Color = COLOR_ACTION;
pub(crate) const COLOR_RUNNING: Color = COLOR_ACTION;
pub(crate) const COLOR_ERROR: Color = Color::Rgb(190, 23, 59);
pub(crate) const COLOR_THINKING: Color = Color::Rgb(121, 125, 222);
pub(crate) const COLOR_THOUGHT: Color = COLOR_TEXT_SECONDARY;
pub(crate) const COLOR_THOUGHT_BODY: Color = COLOR_TEXT_SECONDARY;
pub(crate) const COLOR_DIFF_ADDED: Color = Color::Rgb(115, 190, 104);
pub(crate) const COLOR_DIFF_REMOVED: Color = COLOR_ERROR;
pub(crate) const COLOR_DIFF_ADDED_BG: Color = Color::Rgb(28, 61, 35);
pub(crate) const COLOR_DIFF_REMOVED_BG: Color = Color::Rgb(72, 29, 38);

pub(crate) fn primary_style() -> Style {
    Style::default().fg(COLOR_TEXT_PRIMARY)
}

pub(crate) fn secondary_style() -> Style {
    Style::default().fg(COLOR_TEXT_SECONDARY)
}

pub(crate) fn composer_secondary_style() -> Style {
    Style::default().fg(COLOR_COMPOSER_SECONDARY)
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

pub(crate) fn thinking_style() -> Style {
    Style::default().fg(COLOR_THINKING)
}

pub(crate) fn thought_style() -> Style {
    Style::default().fg(COLOR_THOUGHT)
}

pub(crate) fn thought_body_style() -> Style {
    Style::default()
        .fg(COLOR_THOUGHT_BODY)
        .add_modifier(Modifier::DIM)
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
pub(crate) const BOTTOM_PANE_TRANSCRIPT_GAP: u16 = 1;
pub(crate) const BOTTOM_PANE_FOOTER_GAP: u16 = 1;
pub(crate) const BOTTOM_PANE_BOTTOM_PADDING: u16 = 0;
pub(crate) const TRANSCRIPT_HINT: &str = "ctrl + t to view transcript";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_atlas_palette_tokens() {
        assert_eq!(COLOR_SURFACE, Color::Rgb(20, 20, 20));
        assert_eq!(COLOR_SURFACE_ELEVATED, Color::Rgb(32, 33, 37));
        assert_eq!(COLOR_TEXT_PRIMARY, Color::Rgb(233, 246, 225));
        assert_eq!(COLOR_TEXT_SECONDARY, Color::Rgb(130, 125, 125));
        assert_eq!(COLOR_ACTION, Color::Rgb(60, 83, 206));
        assert_eq!(COLOR_THINKING, Color::Rgb(121, 125, 222));
        assert_eq!(COLOR_ERROR, Color::Rgb(190, 23, 59));
        assert_eq!(COLOR_DIFF_ADDED, Color::Rgb(115, 190, 104));
        assert_eq!(COLOR_DIFF_REMOVED, COLOR_ERROR);
        assert_eq!(COLOR_DIFF_ADDED_BG, Color::Rgb(28, 61, 35));
        assert_eq!(COLOR_DIFF_REMOVED_BG, Color::Rgb(72, 29, 38));
        assert_eq!(COLOR_SUCCESS, COLOR_ACTION);
    }

    #[test]
    fn dims_only_the_expanded_thought_body() {
        assert!(!secondary_style().add_modifier.contains(Modifier::DIM));
        assert!(
            !composer_secondary_style()
                .add_modifier
                .contains(Modifier::DIM)
        );
        assert!(thought_body_style().add_modifier.contains(Modifier::DIM));
    }
}
