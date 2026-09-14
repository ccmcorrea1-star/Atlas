//! Word wrapping adapted from the Codex TUI's width-aware helpers.
//!
//! See `clients/tui/NOTICE` and `clients/tui/LICENSE-APACHE` for attribution.

use std::borrow::Cow;
use std::ops::Range;

use ratatui::text::{Line, Span};
use textwrap::{Options, WordSeparator, WordSplitter, WrapAlgorithm};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(crate) fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let ranges = wrap_ranges(&text, width);
    if ranges.is_empty() {
        return vec![Line::default().style(line.style)];
    }

    ranges
        .into_iter()
        .map(|range| styled_range(&line, &text, range).style(line.style))
        .collect()
}

pub(crate) fn wrap_lines(
    lines: impl IntoIterator<Item = Line<'static>>,
    width: usize,
) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .flat_map(|line| wrap_line(line, width))
        .collect()
}

pub(crate) fn wrap_text(input: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for logical_line in input.split('\n') {
        if logical_line.is_empty() {
            lines.push(String::new());
            continue;
        }
        lines.extend(wrap_plain(logical_line, width));
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

pub(crate) fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

pub(crate) fn wrap_command_with_widths(
    text: &str,
    first_width: usize,
    continuation_width: usize,
) -> Vec<String> {
    wrap_plain_with_widths_using(text, first_width, continuation_width, true)
}

pub(crate) fn wrap_plain_no_hyphenation(text: &str, width: usize) -> Vec<String> {
    wrap_plain_using(text, width, true)
}

fn wrap_plain_with_widths_using(
    text: &str,
    first_width: usize,
    continuation_width: usize,
    no_hyphenation: bool,
) -> Vec<String> {
    let first_width = first_width.max(1);
    let continuation_width = continuation_width.max(1);
    let mut lines = wrap_plain_using(text, first_width, no_hyphenation);
    if lines.len() <= 1 {
        return lines;
    }

    let first = lines.remove(0);
    let first_end = text.find(&first).map_or(0, |start| start + first.len());
    let remainder = text[first_end..].trim_start();
    let mut output = vec![first];
    output.extend(wrap_plain_using(
        remainder,
        continuation_width,
        no_hyphenation,
    ));
    output
}

fn wrap_ranges(text: &str, width: usize) -> Vec<Range<usize>> {
    if text.is_empty() {
        return std::iter::once(0..0).collect();
    }

    let options = wrapping_options(text, width, false);
    let mut ranges = Vec::new();
    let mut cursor = 0;
    for wrapped in textwrap::wrap(text, &options) {
        let wrapped = wrapped.as_ref();
        let start = text[cursor..]
            .find(wrapped)
            .map_or(cursor, |offset| cursor + offset);
        let end = start + wrapped.len();
        if end > start {
            ranges.push(start..end);
            cursor = end;
        }
        while text[cursor..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            let next = text[cursor..]
                .chars()
                .next()
                .map_or(cursor + 1, |character| cursor + character.len_utf8());
            cursor = next;
        }
    }

    if ranges.is_empty() {
        std::iter::once(0..text.len()).collect()
    } else {
        ranges
    }
}

fn wrap_plain(text: &str, width: usize) -> Vec<String> {
    wrap_plain_using(text, width, false)
}

fn wrap_plain_using(text: &str, width: usize, no_hyphenation: bool) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let options = wrapping_options(text, width, no_hyphenation);
    textwrap::wrap(text, &options)
        .into_iter()
        .map(Cow::into_owned)
        .collect()
}

fn wrapping_options(text: &str, width: usize, no_hyphenation: bool) -> Options<'static> {
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
        || (token.split('/').next().is_some_and(is_domain) && token.contains('/'))
}

fn is_domain(domain: &str) -> bool {
    let Some((host, tld)) = domain.rsplit_once('.') else {
        return false;
    };
    host.split('.').all(|label| {
        !label.is_empty()
            && label
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
    }) && (2..=63).contains(&tld.len())
        && tld.chars().all(|character| character.is_ascii_alphabetic())
}

fn styled_range(line: &Line<'static>, text: &str, range: Range<usize>) -> Line<'static> {
    let mut spans = Vec::new();
    let mut offset = 0;
    for span in &line.spans {
        let span_start = offset;
        let span_end = offset + span.content.len();
        let start = range.start.max(span_start);
        let end = range.end.min(span_end);
        if start < end {
            spans.push(Span::styled(text[start..end].to_owned(), span.style));
        }
        offset = span_end;
    }
    Line::from(spans)
}

pub(crate) fn cursor_position(input: &str, cursor: usize, width: usize) -> (usize, usize) {
    let cursor = cursor.min(input.len());
    let mut parts = input[..cursor].split('\n').peekable();
    let mut row = 0;
    while let Some(part) = parts.next() {
        let wrapped = wrap_text(part, width.max(1));
        if parts.peek().is_some() {
            row += wrapped.len();
        } else {
            row += wrapped.len().saturating_sub(1);
            let column = display_width(wrapped.last().map_or("", String::as_str));
            return (row, column.min(width.saturating_sub(1)));
        }
    }
    (0, 0)
}

#[allow(dead_code)]
fn grapheme_width(text: &str) -> usize {
    text.graphemes(true)
        .map(|grapheme| {
            grapheme
                .chars()
                .map(|character| UnicodeWidthChar::width(character).unwrap_or(0))
                .sum::<usize>()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{wrap_plain_no_hyphenation, wrap_text};

    #[test]
    fn wraps_prose_at_word_boundaries() {
        assert_eq!(wrap_text("one two three", 7), ["one two", "three"]);
    }

    #[test]
    fn breaks_long_commands_without_inserting_hyphens() {
        assert_eq!(wrap_plain_no_hyphenation("abcdefgh", 4), ["abcd", "efgh"]);
    }

    #[test]
    fn keeps_url_tokens_intact_when_possible() {
        assert_eq!(
            wrap_plain_no_hyphenation("https://example.com/a/b", 8),
            ["https://example.com/a/b"]
        );
    }
}
