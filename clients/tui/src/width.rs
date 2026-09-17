//! Terminal display-width helpers and guards for fixed prefix columns.
//!
//! Several rendering paths reserve a fixed number of columns for bullets,
//! gutters, or labels before laying out content.  When the terminal is very
//! narrow, those reserved columns can consume the entire width, leaving zero
//! or negative space for content.
//!
//! The display-width helpers match Ratatui's terminal-cell semantics while retaining `usize`
//! precision for long lines. The guards centralise subtraction and enforce a strict-positive
//! contract: they return `Some(n)` where `n > 0`, or `None` when no usable
//! content width remains.  Callers treat `None` as "render prefix-only
//! fallback" rather than attempting wrapped rendering at zero width, which
//! would produce empty or unstable output.

use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

/// Returns the display width Ratatui uses for terminal text without its `u16` limit.
pub(crate) fn display_width(text: &str) -> usize {
    let mut width = 0;
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let end = if bytes[index] == b'\x1b' {
            ansi_sequence_end(text, index)
        } else {
            index + text[index..].chars().next().map_or(1, char::len_utf8)
        };
        if bytes[index] == b'\x1b' {
            width += visible_segment_width(&text[start..index]);
            index = end;
            start = end;
        } else {
            index = end;
        }
    }
    width + visible_segment_width(&text[start..])
}

fn visible_segment_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
        + text
            .chars()
            .filter(|ch| matches!(ch, '\u{FF9E}' | '\u{FF9F}'))
            .count()
}

fn ansi_sequence_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let Some(&control) = bytes.get(start + 1) else {
        return bytes.len();
    };
    if control == b'[' {
        return (start + 2..bytes.len())
            .find(|index| (b'@'..=b'~').contains(&bytes[*index]))
            .map_or(bytes.len(), |index| index + 1);
    }
    if control == b']' {
        for index in start + 2..bytes.len() {
            if bytes[index] == 0x07 {
                return index + 1;
            }
            if bytes[index] == b'\x1b' && bytes.get(index + 1) == Some(&b'\\') {
                return index + 2;
            }
        }
    }
    (start + 2).min(bytes.len())
}

/// Returns a scalar's terminal width, treating halfwidth sound marks as visible cells.
#[allow(dead_code)]
pub(crate) fn char_width(ch: char) -> usize {
    if matches!(ch, '\u{FF9E}' | '\u{FF9F}') {
        1
    } else {
        UnicodeWidthChar::width(ch).unwrap_or(0)
    }
}

/// Returns usable content width after reserving fixed columns.
///
/// Guarantees a strict positive width (`Some(n)` where `n > 0`) or `None` when
/// the reserved columns consume the full width.
///
/// Treat `None` as "render prefix-only fallback". Coercing it to `0` and still
/// attempting wrapped rendering often produces empty or unstable output at very
/// narrow terminal widths.
#[allow(dead_code)]
pub(crate) fn usable_content_width(total_width: usize, reserved_cols: usize) -> Option<usize> {
    total_width
        .checked_sub(reserved_cols)
        .filter(|remaining| *remaining > 0)
}

/// `u16` convenience wrapper around [`usable_content_width`].
///
/// This keeps width math at callsites that receive terminal dimensions as
/// `u16` while preserving the same `None` contract for exhausted width.
#[allow(dead_code)]
pub(crate) fn usable_content_width_u16(total_width: u16, reserved_cols: u16) -> Option<usize> {
    usable_content_width(usize::from(total_width), usize::from(reserved_cols))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::line_truncation::line_width;
    use pretty_assertions::assert_eq;
    use ratatui::text::Line;

    #[test]
    fn display_width_matches_ratatui_halfwidth_sound_marks_without_overflow() {
        assert_eq!(display_width("ｶﾞﾊﾟ"), 4);
        assert_eq!(display_width("ｶﾞﾞ"), 3);
        assert_eq!(display_width("界ﾞ"), 3);
        assert_eq!(char_width('\u{FF9E}'), 1);
        assert_eq!(char_width('\u{FF9F}'), 1);

        let text = "a".repeat(65_536);
        assert_eq!(display_width(&text), 65_536);
        assert_eq!(line_width(&Line::from(text)), 65_536);
    }

    #[test]
    fn usable_content_width_returns_none_when_reserved_exhausts_width() {
        assert_eq!(
            usable_content_width(/*total_width*/ 0, /*reserved_cols*/ 0),
            None
        );
        assert_eq!(
            usable_content_width(/*total_width*/ 2, /*reserved_cols*/ 2),
            None
        );
        assert_eq!(
            usable_content_width(/*total_width*/ 3, /*reserved_cols*/ 4),
            None
        );
        assert_eq!(
            usable_content_width(/*total_width*/ 5, /*reserved_cols*/ 4),
            Some(1)
        );
    }

    #[test]
    fn usable_content_width_u16_matches_usize_variant() {
        assert_eq!(
            usable_content_width_u16(/*total_width*/ 2, /*reserved_cols*/ 2),
            None
        );
        assert_eq!(
            usable_content_width_u16(/*total_width*/ 5, /*reserved_cols*/ 4),
            Some(1)
        );
    }

    #[test]
    fn display_width_ignores_terminal_control_sequences() {
        assert_eq!(
            display_width("\x1b]8;;https://example.test\x07Atlas\x1b]8;;\x07"),
            5
        );
        assert_eq!(display_width("\x1b[31mred\x1b[0m"), 3);
    }
}
