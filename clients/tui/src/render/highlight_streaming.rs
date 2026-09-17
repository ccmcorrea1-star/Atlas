//! Estado Syntect reutilizável para destacar linhas novas de uma fence aberta.

use super::highlight::MAX_HIGHLIGHT_LINE_BYTES;
use super::highlight::find_syntax;
use super::highlight::highlighted_line_spans;
use super::highlight::syntax_set;
use super::highlight::theme;
use ratatui::text::Line;
use syntect::easy::HighlightLines;
use syntect::highlighting::{HighlightState, Style as SyntectStyle};
use syntect::parsing::ParseState;
use syntect::util::LinesWithEndings;

const MAX_HIGHLIGHT_BYTES: usize = 512 * 1024;
const MAX_HIGHLIGHT_LINES: usize = 10_000;

#[derive(Debug)]
pub(crate) struct StreamingCodeHighlighter {
    state: Option<HighlightedState>,
}

#[derive(Debug)]
struct HighlightedState {
    bytes: usize,
    lines: usize,
    syntax: (HighlightState, ParseState),
}

impl StreamingCodeHighlighter {
    pub(crate) fn new(code: &str, language: &str) -> Option<Self> {
        let syntax = find_syntax(language).filter(|_| within_limits(code, 0))?;
        let mut highlighter = HighlightLines::new(syntax, theme());
        for line in LinesWithEndings::from(code) {
            highlighter.highlight_line(line, syntax_set()).ok()?;
        }
        Some(Self {
            state: Some(HighlightedState {
                bytes: code.len(),
                lines: code.lines().count(),
                syntax: highlighter.state(),
            }),
        })
    }

    pub(crate) fn append(mut self, appended: &str) -> Option<(Self, Vec<Line<'static>>)> {
        if appended.is_empty() || !appended.ends_with('\n') {
            return None;
        }
        let mut state = self.state.take()?;
        let bytes = state.bytes.checked_add(appended.len())?;
        let lines = state.lines.checked_add(appended.lines().count())?;
        if !within_limits(appended, bytes.saturating_sub(appended.len()))
            || lines > MAX_HIGHLIGHT_LINES
        {
            return None;
        }
        let (highlight_state, parse_state) = state.syntax;
        let mut highlighter = HighlightLines::from_state(theme(), highlight_state, parse_state);
        let mut rendered = Vec::new();
        for line in LinesWithEndings::from(appended) {
            let ranges: Vec<(SyntectStyle, &str)> =
                highlighter.highlight_line(line, syntax_set()).ok()?;
            rendered.push(Line::from(highlighted_line_spans(ranges)));
        }
        state.bytes = bytes;
        state.lines = lines;
        state.syntax = highlighter.state();
        self.state = Some(state);
        Some((self, rendered))
    }
}

fn within_limits(text: &str, prior_bytes: usize) -> bool {
    prior_bytes.saturating_add(text.len()) <= MAX_HIGHLIGHT_BYTES
        && text.lines().count() <= MAX_HIGHLIGHT_LINES
        && text
            .lines()
            .all(|line| line.len() <= MAX_HIGHLIGHT_LINE_BYTES)
}

#[cfg(test)]
mod tests {
    use super::StreamingCodeHighlighter;

    fn plain(lines: &[ratatui::text::Line<'static>]) -> String {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn appends_highlighting_from_the_previous_parser_state() {
        let highlighter =
            StreamingCodeHighlighter::new("fn main() {\n", "rust").expect("rust syntax");
        let (highlighter, first) = highlighter
            .append("    println!(\"ok\");\n")
            .expect("complete appended line");
        assert_eq!(plain(&first), "    println!(\"ok\");");
        assert!(
            first
                .iter()
                .flat_map(|line| line.spans.iter())
                .any(|span| { span.style.fg.is_some() })
        );
        assert!(highlighter.append("}").is_none());
    }

    #[test]
    fn highlighter_state_is_safe_for_history_cell_caches() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<StreamingCodeHighlighter>();
    }

    #[test]
    fn unknown_language_does_not_create_incremental_state() {
        assert!(StreamingCodeHighlighter::new("text\n", "not-a-language").is_none());
    }
}
