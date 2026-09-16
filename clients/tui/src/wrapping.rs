//! Width-aware wrapping adapted from the Codex TUI.
//!
//! URL tokens are deliberately not split at punctuation. This keeps terminal
//! links readable while ordinary prose still uses word boundaries.

use std::borrow::Cow;
use std::ops::Range;

use ratatui::text::Line;
use ratatui::text::Span;
use textwrap::Options;
use textwrap::WordSeparator;
use textwrap::WordSplitter;
use textwrap::WrapAlgorithm;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

pub(crate) fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

pub(crate) fn wrap_text(input: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for logical_line in input.split('\n') {
        if logical_line.is_empty() {
            lines.push(String::new());
        } else {
            lines.extend(wrap_plain(logical_line, width));
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

pub(crate) fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    let text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    wrap_ranges(&text, width.max(1))
        .into_iter()
        .map(|range| {
            let mut spans = Vec::new();
            let mut offset = 0;
            for span in &line.spans {
                let span_start = offset;
                let span_end = offset + span.content.len();
                let start = range.start.max(span_start);
                let end = range.end.min(span_end);
                if start < end && text.is_char_boundary(start) && text.is_char_boundary(end) {
                    spans.push(Span::styled(text[start..end].to_owned(), span.style));
                }
                offset = span_end;
            }
            Line::from(spans).style(line.style)
        })
        .collect()
}

pub(crate) fn wrap_plain_no_hyphenation(text: &str, width: usize) -> Vec<String> {
    let options = wrapping_options(width, true, text);
    textwrap::wrap(text, &options)
        .into_iter()
        .map(Cow::into_owned)
        .collect()
}

pub(crate) fn cursor_position(input: &str, cursor: usize, width: usize) -> (usize, usize) {
    let cursor = cursor.min(input.len());
    let mut row = 0;
    let mut parts = input[..cursor].split('\n').peekable();
    while let Some(part) = parts.next() {
        let wrapped = wrap_text(part, width.max(1));
        if parts.peek().is_some() {
            row += wrapped.len();
        } else {
            row += wrapped.len().saturating_sub(1);
            return (
                row,
                display_width(wrapped.last().map_or("", String::as_str))
                    .min(width.saturating_sub(1)),
            );
        }
    }
    (0, 0)
}

fn wrap_ranges(text: &str, width: usize) -> Vec<Range<usize>> {
    if text.is_empty() {
        return std::iter::once(0..0).collect();
    }
    let mut ranges = Vec::new();
    let mut cursor = 0;
    for wrapped in wrap_plain(text, width) {
        let start = text[cursor..]
            .find(wrapped.as_str())
            .map_or(cursor, |offset| cursor + offset);
        let end = start + wrapped.len();
        if end > start {
            ranges.push(start..end);
            cursor = end;
        }
        while let Some(character) = text[cursor..].chars().next()
            && character.is_whitespace()
        {
            cursor += character.len_utf8();
        }
    }
    if ranges.is_empty() {
        std::iter::once(0..text.len()).collect()
    } else {
        ranges
    }
}

fn wrap_plain(text: &str, width: usize) -> Vec<String> {
    let options = wrapping_options(width, false, text);
    textwrap::wrap(text, &options)
        .into_iter()
        .map(Cow::into_owned)
        .collect()
}

fn wrapping_options(width: usize, no_hyphenation: bool, text: &str) -> Options<'static> {
    let has_url = text.split_ascii_whitespace().any(is_url_like);
    let mut options = Options::new(width.max(1)).wrap_algorithm(WrapAlgorithm::FirstFit);
    if has_url {
        options = options
            .word_separator(WordSeparator::AsciiSpace)
            .word_splitter(WordSplitter::NoHyphenation)
            .break_words(false);
    } else if no_hyphenation {
        options = options.word_splitter(WordSplitter::NoHyphenation);
    }
    options
}

fn is_url_like(token: &str) -> bool {
    let token = token.trim_matches(|character: char| {
        matches!(
            character,
            '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | '.'
        )
    });
    token.starts_with("http://")
        || token.starts_with("https://")
        || token.starts_with("www.")
        || (token.contains('/') && token.split('/').next().is_some_and(is_domain))
}

fn is_domain(domain: &str) -> bool {
    let Some((host, tld)) = domain.rsplit_once('.') else {
        return false;
    };
    !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
        && (2..=63).contains(&tld.len())
        && tld.chars().all(|character| character.is_ascii_alphabetic())
}

pub(crate) fn position_at_display_column(text: &str, offset: usize, target: usize) -> usize {
    let mut width = 0;
    for (index, grapheme) in text.grapheme_indices(true) {
        let grapheme_width = grapheme
            .chars()
            .map(|character| UnicodeWidthChar::width(character).unwrap_or(0))
            .sum::<usize>();
        if width + grapheme_width > target {
            return offset + index;
        }
        width += grapheme_width;
    }
    offset + text.len()
}

#[cfg(test)]
mod tests {
    use super::{wrap_plain_no_hyphenation, wrap_text};

    #[test]
    fn wraps_prose_at_word_boundaries() {
        assert_eq!(wrap_text("one two three", 7), ["one two", "three"]);
    }

    #[test]
    fn keeps_url_tokens_intact_when_possible() {
        assert_eq!(
            wrap_plain_no_hyphenation("https://example.com/a/b", 8),
            ["https://example.com/a/b"]
        );
    }
}
