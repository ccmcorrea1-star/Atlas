//! Simbolos Unicode usados pela identidade visual da TUI.

pub(crate) const PROMPT: &str = "›";
pub(crate) const WEB_SEARCH: &str = "◎";
pub(crate) const SHELL: &str = ">_";
pub(crate) const CODE: &str = "<>";
pub(crate) const SUCCESS: &str = "✓";
pub(crate) const ERROR: &str = "✗";
pub(crate) const CANCELLED: &str = "•";

#[allow(dead_code)]
pub(crate) fn tool_icon(tool_name: &str) -> &'static str {
    if tool_name.starts_with("web.") || tool_name == "filesystem.search" {
        WEB_SEARCH
    } else if tool_name == "shell.exec" {
        SHELL
    } else {
        CODE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_unicode_tool_icons_without_nerd_font_symbols() {
        assert_eq!(tool_icon("web.search"), "◎");
        assert_eq!(tool_icon("filesystem.search"), "◎");
        assert_eq!(tool_icon("shell.exec"), ">_");
        assert_eq!(tool_icon("lsp.diagnostics"), "<>");
        assert_eq!(SUCCESS, "✓");
        assert_eq!(ERROR, "✗");
        assert_eq!(CANCELLED, "•");
    }
}
