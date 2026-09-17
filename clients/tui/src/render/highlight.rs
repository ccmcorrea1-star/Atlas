//! Syntax highlighting portado do renderer do Codex para blocos e comandos.

use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use syntect::easy::HighlightLines;
use syntect::highlighting::Color as SyntectColor;
use syntect::highlighting::FontStyle;
use syntect::highlighting::Theme;
use syntect::parsing::SyntaxReference;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use two_face::theme::EmbeddedThemeName;

const MAX_HIGHLIGHT_BYTES: usize = 512 * 1024;
const MAX_HIGHLIGHT_LINES: usize = 10_000;
pub(crate) const MAX_HIGHLIGHT_LINE_BYTES: usize = 4 * 1024;

fn syntax_set() -> &'static SyntaxSet {
    use std::sync::OnceLock;
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

fn theme() -> Theme {
    two_face::theme::extra()
        .get(EmbeddedThemeName::CatppuccinMocha)
        .clone()
}

fn find_syntax(lang: &str) -> Option<&'static SyntaxReference> {
    let normalized = lang.to_ascii_lowercase();
    let alias = match normalized.as_str() {
        "csharp" | "c-sharp" => "c#",
        "golang" => "go",
        "python3" => "python",
        "shell" | "sh" => "bash",
        _ => lang,
    };
    syntax_set()
        .find_syntax_by_token(alias)
        .or_else(|| syntax_set().find_syntax_by_name(alias))
        .or_else(|| {
            syntax_set()
                .syntaxes()
                .iter()
                .find(|syntax| syntax.name.eq_ignore_ascii_case(alias))
        })
}

fn convert_color(color: SyntectColor) -> Option<Color> {
    match color.a {
        0 => Some(match color.r {
            0 => Color::Black,
            1 => Color::Red,
            2 => Color::Green,
            3 => Color::Yellow,
            4 => Color::Blue,
            5 => Color::Magenta,
            6 => Color::Cyan,
            7 => Color::Gray,
            index => Color::Indexed(index),
        }),
        1 => None,
        _ => Some(Color::Rgb(color.r, color.g, color.b)),
    }
}

fn convert_style(style: syntect::highlighting::Style) -> Style {
    let mut result = Style::default();
    if let Some(color) = convert_color(style.foreground) {
        result = result.fg(color);
    }
    if style.font_style.contains(FontStyle::BOLD) {
        result = result.add_modifier(Modifier::BOLD);
    }
    result
}

fn highlight_spans(code: &str, lang: &str) -> Option<Vec<Vec<Span<'static>>>> {
    if code.is_empty()
        || code.len() > MAX_HIGHLIGHT_BYTES
        || code.lines().count() > MAX_HIGHLIGHT_LINES
        || code
            .lines()
            .any(|line| line.len() > MAX_HIGHLIGHT_LINE_BYTES)
    {
        return None;
    }
    let syntax = find_syntax(lang)?;
    let theme = theme();
    let mut highlighter = HighlightLines::new(syntax, &theme);
    let mut lines = Vec::new();
    for source in LinesWithEndings::from(code) {
        let ranges = highlighter.highlight_line(source, syntax_set()).ok()?;
        let spans = ranges
            .into_iter()
            .filter_map(|(style, text)| {
                let text = text.trim_end_matches(['\r', '\n']);
                (!text.is_empty()).then(|| Span::styled(text.to_owned(), convert_style(style)))
            })
            .collect::<Vec<_>>();
        lines.push(if spans.is_empty() {
            vec![Span::raw(String::new())]
        } else {
            spans
        });
    }
    Some(lines)
}

pub(crate) fn highlight_code_to_lines(code: &str, lang: &str) -> Vec<Line<'static>> {
    highlight_spans(code, lang)
        .map(|lines| lines.into_iter().map(Line::from).collect())
        .unwrap_or_else(|| {
            let mut lines = code
                .lines()
                .map(|line| Line::from(line.to_owned()))
                .collect::<Vec<_>>();
            if lines.is_empty() {
                lines.push(Line::from(String::new()));
            }
            lines
        })
}

pub(crate) fn highlight_bash_to_lines(script: &str) -> Vec<Line<'static>> {
    highlight_code_to_lines(script, "bash")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[Line<'static>]) -> String {
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
    fn highlights_bash_without_changing_source() {
        let source = "echo hello && printf '%s' world";
        let lines = highlight_bash_to_lines(source);
        assert_eq!(plain(&lines), source);
        assert!(lines[0].spans.iter().any(|span| span.style.fg.is_some()));
    }

    #[test]
    fn falls_back_for_unknown_language_and_large_input() {
        let source = "plain text";
        assert_eq!(plain(&highlight_code_to_lines(source, "unknown")), source);
        assert_eq!(
            plain(&highlight_code_to_lines(
                &"x".repeat(MAX_HIGHLIGHT_BYTES + 1),
                "rust"
            )),
            "x".repeat(MAX_HIGHLIGHT_BYTES + 1)
        );
    }
}
