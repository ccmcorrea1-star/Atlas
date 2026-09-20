//! Familias de frames animados da TUI.

use crate::icons;

pub(crate) const REASONING_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
pub(crate) const WEB_FRAMES: [&str; 4] = [icons::WEB_SEARCH, "◉", "●", "◉"];
pub(crate) const TOOL_FRAMES: [&str; 4] = ["░", "▒", "▓", "▒"];

pub(crate) fn reasoning_frame(frame: usize) -> &'static str {
    REASONING_FRAMES[frame % REASONING_FRAMES.len()]
}

pub(crate) fn tool_frame(tool_name: &str, frame: usize) -> &'static str {
    let frames = if tool_name.starts_with("web.") || tool_name == "filesystem.search" {
        &WEB_FRAMES
    } else {
        &TOOL_FRAMES
    };
    frames[frame % frames.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_the_reasoning_spinner() {
        assert_eq!(reasoning_frame(0), "⠋");
        assert_eq!(reasoning_frame(10), "⠋");
        assert_eq!(reasoning_frame(9), "⠏");
    }

    #[test]
    fn uses_the_network_circle_family_for_web_tools() {
        assert_eq!(tool_frame("web.search", 0), "◎");
        assert_eq!(tool_frame("web.search", 1), "◉");
        assert_eq!(tool_frame("web.search", 2), "●");
        assert_eq!(tool_frame("filesystem.search", 3), "◉");
    }

    #[test]
    fn uses_the_intensity_family_for_general_tools() {
        assert_eq!(tool_frame("filesystem.read", 0), "░");
        assert_eq!(tool_frame("filesystem.read", 1), "▒");
        assert_eq!(tool_frame("filesystem.patch", 2), "▓");
        assert_eq!(tool_frame("filesystem.patch", 3), "▒");
    }
}
