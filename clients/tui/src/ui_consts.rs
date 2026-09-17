//! Constantes de layout compartilhadas com a TUI do Codex.

/// Colunas reservadas pela margem esquerda e pelo prefixo do composer.
pub(crate) const LIVE_PREFIX_COLS: u16 = 2;
pub(crate) const FOOTER_INDENT_COLS: usize = LIVE_PREFIX_COLS as usize;
pub(crate) const TRANSCRIPT_HINT: &str = "ctrl + t to view transcript";
