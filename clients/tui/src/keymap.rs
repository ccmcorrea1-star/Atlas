//! Resolucao de acoes de teclas para a TUI no estilo Codex.
//!
//! Os handlers consomem acoes em vez de espalhar detalhes do terminal pelo
//! estado das views. Os atalhos padrao acompanham os atalhos principais do Codex.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    Cancel,
    CloseOverlay,
    ConfirmQuit,
    DeclineQuit,
    ClearDraft,
    OpenExternalEditor,
    OpenHistorySearch,
    OpenTranscript,
    OpenShortcuts,
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
    JumpTop,
    JumpBottom,
}

/// Label curto usado por overlays para manter os hints sincronizados com as
/// teclas aceitas por [`resolve`].
pub(crate) fn hint(action: Action) -> &'static str {
    match action {
        Action::Cancel => "esc",
        Action::CloseOverlay => "q",
        Action::PageUp => "pgup",
        Action::PageDown => "pgdn",
        Action::JumpTop => "ctrl-home",
        Action::JumpBottom => "ctrl-end",
        Action::ScrollUp => "↑",
        Action::ScrollDown => "↓",
        _ => "",
    }
}

pub(crate) fn resolve(key: KeyEvent) -> Option<Action> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let none = key.modifiers == KeyModifiers::NONE;
    match key.code {
        KeyCode::Esc if none => Some(Action::Cancel),
        KeyCode::Char('q') if none => Some(Action::CloseOverlay),
        KeyCode::Char('n') if none => Some(Action::DeclineQuit),
        KeyCode::Char('y') | KeyCode::Enter if none => Some(Action::ConfirmQuit),
        KeyCode::Char('c') if ctrl => Some(Action::ClearDraft),
        KeyCode::Char('g') if ctrl => Some(Action::OpenExternalEditor),
        KeyCode::Char('r') if ctrl => Some(Action::OpenHistorySearch),
        KeyCode::Char('t') if ctrl => Some(Action::OpenTranscript),
        KeyCode::Char('?') if none => Some(Action::OpenShortcuts),
        KeyCode::Up if none => Some(Action::ScrollUp),
        KeyCode::Down if none => Some(Action::ScrollDown),
        KeyCode::PageUp if none => Some(Action::PageUp),
        KeyCode::PageDown if none => Some(Action::PageDown),
        KeyCode::Home if ctrl => Some(Action::JumpTop),
        KeyCode::End if ctrl => Some(Action::JumpBottom),
        _ => None,
    }
}
