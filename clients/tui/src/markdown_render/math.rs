//! Recognize math before Markdown consumes TeX escapes, retaining exact source offsets.
//!
//! Only the rendering copy is masked. Unsupported expressions are restored verbatim; code,
//! links, and HTML stay under the ordinary Markdown renderer.

use pulldown_cmark::Event;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;
use std::borrow::Cow;
use std::ops::Range;

#[path = "math/render.rs"]
pub(super) mod render;
pub(super) use render::render as render_formula;

const MAX_MATH_BYTES: usize = 4096;

pub(super) struct MathMarkdown<'a> {
    pub(super) markdown: Cow<'a, str>,
    pub(super) pending_start: Option<usize>,
    pub(super) display_ranges: Vec<Range<usize>>,
    replacements: Vec<(Range<usize>, String)>,
}

impl<'a> MathMarkdown<'a> {
    pub(super) fn new(input: &'a str, options: Options, width: Option<usize>) -> Self {
        let mut result = Self {
            markdown: Cow::Borrowed(input),
            pending_start: None,
            display_ranges: Vec::new(),
            replacements: Vec::new(),
        };
        if !input.contains('$') && !input.contains("\\(") && !input.contains("\\[") {
            return result;
        }

        let parser = Parser::new_ext(input, options);
        let mut protected: Vec<_> = parser
            .reference_definitions()
            .iter()
            .map(|(_, definition)| definition.span.clone())
            .collect();
        let mut containers = Vec::new();
        protected.extend(parser.into_offset_iter().filter_map(|(event, range)| {
            if matches!(event, Event::Start(Tag::List(_) | Tag::BlockQuote(_))) {
                containers.push(range.clone());
            }
            matches!(
                event,
                Event::Code(_)
                    | Event::Html(_)
                    | Event::InlineHtml(_)
                    | Event::Start(Tag::CodeBlock(_) | Tag::Link { .. } | Tag::Image { .. })
            )
            .then_some(range)
        }));
        protected.sort_unstable_by_key(|range| range.start);
        let mut protected = protected.iter().peekable();
        containers.sort_unstable_by_key(|range| range.start);
        let mut containers = containers.into_iter().peekable();
        let mut offset = 0;
        let mut scanned = 0;
        let mut line_start = 0;
        let mut line_has_text = false;

        while offset < input.len() {
            if let Some(index) = input[scanned..offset].rfind('\n') {
                line_start = scanned + index + 1;
                line_has_text = false;
            }
            line_has_text |= !input[scanned.max(line_start)..offset].trim().is_empty();
            scanned = offset;
            while protected.peek().is_some_and(|range| range.end <= offset) {
                protected.next();
            }
            while containers.peek().is_some_and(|range| range.end <= offset) {
                containers.next();
            }
            if let Some(range) = protected.peek()
                && range.contains(&offset)
            {
                offset = range.end;
                continue;
            }

            let rest = &input[offset..];
            let (open, close, display) = if rest.starts_with("$$") {
                ("$$", "$$", true)
            } else if rest.starts_with("\\[") {
                ("\\[", "\\]", true)
            } else if rest.starts_with("\\(") {
                ("\\(", "\\)", false)
            } else if rest.starts_with('$') {
                ("$", "$", false)
            } else {
                let Some(character) = rest.chars().next() else {
                    break;
                };
                offset += character.len_utf8();
                continue;
            };

            let start = offset;
            offset += open.len();
            if escaped(input, start) {
                continue;
            }
            let body = &input[offset..];
            if open == "$"
                && (body.starts_with(char::is_whitespace) || body.starts_with(['(', '{']))
            {
                continue;
            }

            let limit = body
                .char_indices()
                .map(|(index, _)| index)
                .find(|index| *index >= MAX_MATH_BYTES)
                .unwrap_or(body.len());
            let rejected_display = display && line_has_text;
            let search = if display && !rejected_display {
                body
            } else {
                &body[..limit]
            };
            let mut multiline_close = false;
            let mut closing_protected = protected.clone();
            let mut rejected_close = false;
            let end = search.match_indices(&close[..1]).find_map(|(index, _)| {
                let end = offset + index;
                if !search[index..].starts_with(close) || escaped(input, end) {
                    return None;
                }
                if display {
                    while closing_protected
                        .peek()
                        .is_some_and(|range| range.end <= end)
                    {
                        closing_protected.next();
                    }
                    if closing_protected
                        .peek()
                        .is_some_and(|range| range.start < end + close.len())
                    {
                        return None;
                    }
                    if !rejected_display
                        && input[end + close.len()..]
                            .chars()
                            .take_while(|character| *character != '\n')
                            .any(|character| !character.is_whitespace())
                    {
                        if multiline_close || input[offset..end].contains('\n') {
                            multiline_close = true;
                            return None;
                        }
                        rejected_close = true;
                    }
                }
                Some(end)
            });

            if display && (!rejected_display || end.is_some() || body.len() < MAX_MATH_BYTES) {
                result
                    .display_ranges
                    .push(start..end.map_or(input.len(), |end| end + close.len()));
            }
            if rejected_display || rejected_close {
                if let Some(end) = end
                    && !input[offset..end].trim().is_empty()
                {
                    if rejected_display
                        && open == "$$"
                        && input[offset..end]
                            .rsplit_once('\n')
                            .is_some_and(|(_, prefix)| prefix.trim().is_empty())
                        && input[end + close.len()..]
                            .chars()
                            .take_while(|character| *character != '\n')
                            .any(|character| !character.is_whitespace())
                    {
                        continue;
                    }
                    offset = end + close.len();
                } else if end.is_none() && open == "\\[" && body.len() < MAX_MATH_BYTES {
                    break;
                }
                continue;
            }

            let Some(end) = end else {
                if display {
                    if body.len() < MAX_MATH_BYTES {
                        result.pending_start.get_or_insert(line_start);
                    }
                    let span = start..input.len();
                    result
                        .markdown
                        .to_mut()
                        .replace_range(span.clone(), &"$".repeat(span.len()));
                    result.replacements.push((span, input[start..].to_owned()));
                }
                break;
            };

            let span = start..end + close.len();
            let formula = &input[offset..end];
            if display {
                offset = span.end;
            }
            if protected.peek().is_some_and(|range| range.start < span.end) {
                continue;
            }
            if !display && formula.contains('\n') {
                continue;
            }
            if open == "$" {
                let next = input[span.end..].chars().next();
                if formula.ends_with(char::is_whitespace) || next.is_some_and(char::is_alphanumeric)
                {
                    offset = span.end;
                    continue;
                }
                if formula.starts_with(|character: char| character.is_ascii_digit())
                    && !formula.contains(['\\', '^', '_', '=', '+', '-', '*', '/', '<', '>'])
                    || formula.len() > 1
                        && formula
                            .chars()
                            .all(|character| character.is_ascii_uppercase())
                {
                    offset = span.end;
                    continue;
                }
            }
            let rendered = if formula.len() < MAX_MATH_BYTES {
                render::render(formula, display)
            } else {
                None
            }
            .filter(|text| {
                !text.contains('\n')
                    || !containers
                        .peek()
                        .is_some_and(|range| range.contains(&start))
                        && width.is_none_or(|width| {
                            text.lines().all(|line| {
                                crate::width::display_width(line) <= width.saturating_sub(4)
                            })
                        })
            })
            .unwrap_or_else(|| input[span.clone()].to_owned());

            result
                .markdown
                .to_mut()
                .replace_range(span.clone(), &"$".repeat(span.len()));
            offset = span.end;
            result.replacements.push((span, rendered));
        }
        result
    }

    pub(super) fn events<'s>(
        &'s self,
        events: impl Iterator<Item = (Event<'s>, Range<usize>)>,
    ) -> impl Iterator<Item = Event<'s>> {
        let mut replacements = self.replacements.iter().peekable();
        events.flat_map(move |(event, range)| {
            while replacements
                .peek()
                .is_some_and(|(span, _)| span.end <= range.start)
            {
                replacements.next();
            }
            let Event::Text(text) = event else {
                return std::iter::once(event).collect::<Vec<_>>();
            };
            if replacements
                .peek()
                .is_none_or(|(span, _)| span.start >= range.end)
            {
                return std::iter::once(Event::Text(text)).collect();
            }

            let mut output = Vec::new();
            let mut offset = range.start;
            while let Some((span, replacement)) =
                replacements.next_if(|(span, _)| span.end <= range.end)
            {
                if offset < span.start {
                    output.push(Event::Text(self.markdown[offset..span.start].into()));
                }
                output.push(Event::Text(replacement.as_str().into()));
                offset = span.end;
            }
            if offset < range.end {
                output.push(Event::Text(self.markdown[offset..range.end].into()));
            }
            output
        })
    }
}

fn escaped(input: &str, offset: usize) -> bool {
    input[..offset]
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'\\')
        .count()
        % 2
        == 1
}
