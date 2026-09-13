use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use serde_json::Value;

const ANSI_ESCAPE: char = '\x1b';

/// Presentation helpers are intentionally independent from the Runtime protocol.
/// The transcript layout follows the cell-oriented approach used by mature Rust TUIs,
/// adapted here to Atlas messages and events. The component cues were informed by
/// studying the Codex CLI TUI (Apache-2.0), without copying its source or architecture.

pub(crate) fn normalize_tool_output(output: &str) -> String {
    let output = sanitize_terminal_text(output.trim());
    if output.is_empty() {
        return String::new();
    }

    normalize_jsonish(&output, 0).unwrap_or(output)
}

fn normalize_jsonish(value: &str, depth: usize) -> Option<String> {
    if let Ok(parsed) = serde_json::from_str::<Value>(value) {
        return match parsed {
            Value::String(text) if depth < 2 => normalize_jsonish(&text, depth + 1).or(Some(text)),
            Value::String(text) => Some(text),
            parsed => serde_json::to_string_pretty(&parsed).ok(),
        };
    }

    if depth < 2 && value.contains('\\') {
        let quoted = format!("\"{value}\"");
        if let Ok(Value::String(decoded)) = serde_json::from_str(&quoted) {
            return normalize_jsonish(&decoded, depth + 1).or(Some(decoded));
        }
    }

    None
}

pub(crate) fn sanitize_terminal_text(text: &str) -> String {
    let mut sanitized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character == ANSI_ESCAPE {
            if chars.next_if_eq(&'[').is_some() {
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }
        if character == '\t' {
            sanitized.push_str("  ");
        } else if !character.is_control() || character == '\n' {
            sanitized.push(character);
        }
    }
    sanitized
}

pub(crate) fn render_markdown(input: &str) -> Vec<Line<'static>> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);

    MarkdownRenderer::new(Parser::new_ext(input, options)).render()
}

struct MarkdownRenderer<'a> {
    events: Parser<'a>,
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    styles: Vec<Style>,
    list_stack: Vec<Option<u64>>,
    item_prefix: Option<String>,
    quote_depth: usize,
    in_code_block: bool,
    in_table: bool,
    table_cell_index: usize,
    needs_blank: bool,
}

impl<'a> MarkdownRenderer<'a> {
    fn new(events: Parser<'a>) -> Self {
        Self {
            events,
            lines: Vec::new(),
            current: Vec::new(),
            styles: vec![Style::default()],
            list_stack: Vec::new(),
            item_prefix: None,
            quote_depth: 0,
            in_code_block: false,
            in_table: false,
            table_cell_index: 0,
            needs_blank: false,
        }
    }

    fn render(mut self) -> Vec<Line<'static>> {
        while let Some(event) = self.events.next() {
            self.handle(event);
        }
        self.flush_line();
        while self.lines.last().is_some_and(|line| line.spans.is_empty()) {
            self.lines.pop();
        }
        self.lines
    }

    fn handle(&mut self, event: Event<'a>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.push_text(&sanitize_terminal_text(&text)),
            Event::Code(code) => {
                self.push_span(code.into_string(), Style::default().fg(Color::Cyan))
            }
            Event::SoftBreak | Event::HardBreak => self.flush_line(),
            Event::Rule => {
                self.flush_line();
                self.lines
                    .push(Line::from(Span::styled("────", Style::default().dim())));
                self.needs_blank = true;
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                self.push_text(&sanitize_terminal_text(&html));
            }
            Event::InlineMath(math) | Event::DisplayMath(math) => {
                self.push_text(&sanitize_terminal_text(&math));
            }
            Event::FootnoteReference(_) | Event::TaskListMarker(_) => {}
        }
    }

    fn start(&mut self, tag: Tag<'a>) {
        match tag {
            Tag::Paragraph => {
                if self.needs_blank && self.list_stack.is_empty() && !self.lines.is_empty() {
                    self.lines.push(Line::default());
                }
                self.needs_blank = false;
            }
            Tag::Heading { level, .. } => {
                self.flush_line();
                if !self.lines.is_empty() {
                    self.lines.push(Line::default());
                }
                let style = heading_style(level);
                self.current.push(Span::styled(
                    format!("{} ", "#".repeat(level as usize)),
                    style,
                ));
                self.styles.push(style);
            }
            Tag::BlockQuote(_) => {
                self.flush_line();
                self.quote_depth += 1;
            }
            Tag::CodeBlock(kind) => {
                self.flush_line();
                if !self.lines.is_empty() {
                    self.lines.push(Line::default());
                }
                self.in_code_block = true;
                if let CodeBlockKind::Fenced(language) = kind
                    && !language.is_empty()
                {
                    self.current.push(Span::styled(
                        format!("  · {}", sanitize_terminal_text(&language)),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ));
                    self.flush_line();
                }
            }
            Tag::List(start) => self.list_stack.push(start),
            Tag::Item => {
                self.flush_line();
                let indent = "  ".repeat(self.list_stack.len().saturating_sub(1));
                let marker = match self.list_stack.last_mut() {
                    Some(Some(index)) => {
                        let marker = format!("{index}. ");
                        *index += 1;
                        marker
                    }
                    Some(None) => "- ".to_owned(),
                    None => "- ".to_owned(),
                };
                self.item_prefix = Some(format!("{indent}{marker}"));
            }
            Tag::Emphasis => self
                .styles
                .push(Style::default().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self
                .styles
                .push(Style::default().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self
                .styles
                .push(Style::default().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { .. } => self.styles.push(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::UNDERLINED),
            ),
            Tag::Table(_) => {
                self.flush_line();
                if !self.lines.is_empty() {
                    self.lines.push(Line::default());
                }
                self.in_table = true;
            }
            Tag::TableHead => {
                self.styles
                    .push(Style::default().add_modifier(Modifier::BOLD));
            }
            Tag::TableRow => {
                self.flush_line();
                self.table_cell_index = 0;
            }
            Tag::TableCell => {
                self.ensure_prefix();
                if self.table_cell_index > 0 {
                    self.current
                        .push(Span::styled("  │ ", Style::default().fg(Color::DarkGray)));
                }
                self.table_cell_index += 1;
            }
            Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
            | Tag::Image { .. }
            | Tag::MetadataBlock(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_line();
                self.needs_blank = true;
            }
            TagEnd::Heading(_) => {
                self.flush_line();
                self.styles.pop();
                self.needs_blank = true;
            }
            TagEnd::BlockQuote(_) => {
                self.flush_line();
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.needs_blank = true;
            }
            TagEnd::CodeBlock => {
                self.flush_line();
                self.in_code_block = false;
                self.needs_blank = true;
            }
            TagEnd::List(_) => {
                self.flush_line();
                self.list_stack.pop();
                self.item_prefix = None;
                self.needs_blank = true;
            }
            TagEnd::Item => self.flush_line(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.styles.pop();
            }
            TagEnd::Table => {
                self.flush_line();
                self.in_table = false;
                self.needs_blank = true;
            }
            TagEnd::TableHead => {
                self.styles.pop();
            }
            TagEnd::TableRow => self.flush_line(),
            TagEnd::TableCell => {}
            TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
            | TagEnd::Image
            | TagEnd::MetadataBlock(_)
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition => {}
        }
    }

    fn push_text(&mut self, text: &str) {
        for (index, part) in text.split('\n').enumerate() {
            if index > 0 {
                self.flush_line();
            }
            if !part.is_empty() {
                self.push_span(part.to_owned(), self.style());
            }
        }
    }

    fn push_span(&mut self, text: String, style: Style) {
        self.ensure_prefix();
        self.current.push(Span::styled(text, style));
    }

    fn ensure_prefix(&mut self) {
        if !self.current.is_empty() {
            return;
        }
        if self.quote_depth > 0 {
            self.current.push(Span::styled(
                format!("{}", "│ ".repeat(self.quote_depth)),
                Style::default().fg(Color::Green).dim(),
            ));
        }
        if self.in_code_block {
            self.current
                .push(Span::styled("  │ ", Style::default().dim()));
        } else if !self.in_table
            && let Some(prefix) = self.item_prefix.as_deref()
        {
            self.current.push(Span::styled(
                prefix.to_owned(),
                Style::default().fg(Color::Cyan),
            ));
        }
    }

    fn style(&self) -> Style {
        self.styles.last().copied().unwrap_or_default()
    }

    fn flush_line(&mut self) {
        if self.current.is_empty() {
            return;
        }
        self.lines
            .push(Line::from(std::mem::take(&mut self.current)));
    }
}

fn heading_style(level: HeadingLevel) -> Style {
    let modifier = match level {
        HeadingLevel::H1 | HeadingLevel::H2 => Modifier::BOLD,
        _ => Modifier::BOLD,
    };
    Style::default().add_modifier(modifier)
}

#[cfg(test)]
mod tests {
    use super::{normalize_tool_output, render_markdown, sanitize_terminal_text};

    #[test]
    fn formats_quoted_and_structured_tool_output_for_humans() {
        assert_eq!(normalize_tool_output(r#"\"ready\\nnow\""#), "ready\nnow");
        assert_eq!(
            normalize_tool_output(r#"{\"ok\":true,\"items\":[1,2]}"#),
            "{\n  \"items\": [\n    1,\n    2\n  ],\n  \"ok\": true\n}"
        );
    }

    #[test]
    fn removes_terminal_control_sequences_without_losing_lines() {
        assert_eq!(
            sanitize_terminal_text("\x1b[31mred\x1b[0m\nnext\tline"),
            "red\nnext  line"
        );
    }

    #[test]
    fn renders_markdown_as_styled_logical_lines() {
        let lines = render_markdown("## Result\n\n**done** with `atlas`\n\n- one\n- two");
        let text = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(text.contains("## Result"));
        assert!(text.contains("done"));
        assert!(text.contains("- one"));
        assert!(text.contains("- two"));
    }

    #[test]
    fn keeps_table_cells_scannable_without_a_box_around_the_transcript() {
        let lines = render_markdown("| key | value |\n| --- | --- |\n| mode | ready |");
        let text = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(text.contains("key"));
        assert!(text.contains("value"));
        assert!(text.contains("│"));
    }
}
