use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use serde_json::Value;
use std::time::Duration;

const ANSI_ESCAPE: char = '\x1b';
const MAX_OUTPUT_BYTES: usize = 128 * 1024;
const OUTPUT_TRUNCATION_MARKER: &str = "\n[output truncated]";

/// Presentation helpers are independent from the Runtime protocol.
/// The transcript layout follows the cell-oriented approach used by the Codex CLI TUI.
/// See `clients/tui/NOTICE` and `clients/tui/LICENSE-APACHE` for attribution.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ToolOutput {
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) duration: Option<Duration>,
    pub(crate) success: Option<bool>,
}

impl ToolOutput {
    pub(crate) fn display_text(&self) -> String {
        [self.stdout.as_str(), self.stderr.as_str()]
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Extracts human-readable streams from the serialized result returned by a capability.
///
/// Capabilities currently return a JSON envelope, but that envelope is an execution detail and
/// must not leak into the transcript. Nested strings are decoded because the model SDK can add a
/// second serialization layer around tool output.
pub(crate) fn parse_tool_output(raw: &str) -> ToolOutput {
    let value = decode_output_value(raw, 0);
    let mut output = ToolOutput::default();
    collect_tool_output(&value, &mut output);
    output.stdout = clean_output(&output.stdout);
    output.stderr = clean_output(&output.stderr);
    output
}

fn decode_output_value(raw: &str, depth: usize) -> Value {
    let raw = sanitize_terminal_text(raw.trim());
    if let Ok(value) = serde_json::from_str::<Value>(&raw) {
        if let Value::String(text) = &value
            && depth < 3
            && let Some(nested) = decode_nested_string(text, depth + 1)
        {
            return nested;
        }
        return value;
    }

    if depth < 3 && raw.contains('\\') {
        let quoted = format!("\"{raw}\"");
        if let Ok(Value::String(decoded)) = serde_json::from_str(&quoted) {
            if let Some(nested) = decode_nested_string(&decoded, depth + 1) {
                return nested;
            }
            return Value::String(decoded);
        }
    }

    Value::String(raw)
}

fn decode_nested_string(value: &str, depth: usize) -> Option<Value> {
    if depth > 3 {
        return None;
    }
    let trimmed = value.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') || trimmed.starts_with('"') {
        serde_json::from_str(trimmed).ok()
    } else {
        None
    }
}

fn collect_tool_output(value: &Value, output: &mut ToolOutput) {
    match value {
        Value::String(text) => {
            if output.stdout.is_empty() {
                output.stdout = text.clone();
            }
        }
        Value::Object(object) => {
            let mut has_display_output = false;
            if let Some(stdout) = object.get("stdout").and_then(display_value_text) {
                output.stdout = stdout;
                has_display_output = true;
            }
            if let Some(stderr) = object.get("stderr").and_then(display_value_text) {
                output.stderr = stderr;
                has_display_output = true;
            }
            if output.stdout.is_empty() && output.stderr.is_empty() {
                for key in ["text", "message", "content", "output"] {
                    if let Some(value) = object.get(key) {
                        let text = display_value_text(value).unwrap_or_default();
                        if !text.is_empty() {
                            output.stdout = text;
                            has_display_output = true;
                            break;
                        }
                    }
                }
            }
            if output.stderr.is_empty()
                && let Some(error) = object.get("error").and_then(display_value_text)
                && !error.is_empty()
            {
                output.stderr = error;
                has_display_output = true;
            }
            if !has_display_output {
                output.stdout = human_value_lines(value, 0).join("\n");
            }
            output.duration = object
                .get("duration_ms")
                .and_then(Value::as_u64)
                .map(Duration::from_millis);
            output.success = object
                .get("status")
                .and_then(Value::as_str)
                .map(|status| status == "success")
                .or_else(|| {
                    object
                        .get("exit_code")
                        .and_then(Value::as_i64)
                        .map(|code| code == 0)
                });
        }
        Value::Array(values) => {
            let text = values
                .iter()
                .filter_map(value_text)
                .collect::<Vec<_>>()
                .join("\n");
            if !text.is_empty() {
                output.stdout = text;
            }
        }
        Value::Number(number) => output.stdout = number.to_string(),
        Value::Bool(value) => output.stdout = value.to_string(),
        Value::Null => {}
    }
}

fn display_value_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => match decode_output_value(text, 1) {
            Value::String(decoded) => Some(decoded),
            decoded => value_text(&decoded),
        },
        value => value_text(value),
    }
}

fn human_value_lines(value: &Value, indent: usize) -> Vec<String> {
    let prefix = " ".repeat(indent);
    match value {
        Value::Object(object) => object
            .iter()
            .flat_map(|(key, value)| match value {
                Value::Object(_) | Value::Array(_) => {
                    let mut lines = vec![format!("{prefix}{key}:")];
                    lines.extend(human_value_lines(value, indent + 2));
                    lines
                }
                _ => vec![format!(
                    "{prefix}{key}: {}",
                    value_text(value).unwrap_or_default()
                )],
            })
            .collect(),
        Value::Array(values) => values
            .iter()
            .flat_map(|value| match value {
                Value::Object(_) | Value::Array(_) => human_value_lines(value, indent),
                _ => vec![format!("{prefix}{}", value_text(value).unwrap_or_default())],
            })
            .collect(),
        _ => value_text(value).map_or_else(Vec::new, |text| vec![format!("{prefix}{text}")]),
    }
}

fn value_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(values) => {
            let text = values
                .iter()
                .filter_map(value_text)
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
        Value::Object(object) => ["text", "message", "content", "output", "stdout", "stderr"]
            .into_iter()
            .find_map(|key| object.get(key).and_then(value_text)),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null => None,
    }
}

fn clean_output(output: &str) -> String {
    truncate_text(
        sanitize_terminal_text(output.trim()).trim_end(),
        MAX_OUTPUT_BYTES,
    )
}

fn truncate_text(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }

    let mut end = max_bytes
        .saturating_sub(OUTPUT_TRUNCATION_MARKER.len())
        .min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &text[..end], OUTPUT_TRUNCATION_MARKER)
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

pub(crate) fn format_process_command(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(display_argument)
        .map(|argument| shell_quote(&argument))
        .collect::<Vec<_>>()
        .join(" ")
}

fn display_argument(argument: &str) -> String {
    sanitize_terminal_text(argument).replace('\n', "\\n")
}

fn shell_quote(argument: &str) -> String {
    if argument
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-._/:=".contains(character))
    {
        argument.to_owned()
    } else {
        format!("'{}'", argument.replace('\'', "'\\''"))
    }
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
    link_destinations: Vec<String>,
    image_destinations: Vec<String>,
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
            link_destinations: Vec::new(),
            image_destinations: Vec::new(),
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
            Event::Code(code) => self.push_span(
                sanitize_terminal_text(&code),
                Style::default().fg(Color::Cyan),
            ),
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
                let label = match kind {
                    CodeBlockKind::Fenced(language) if !language.is_empty() => {
                        sanitize_terminal_text(&language)
                    }
                    _ => "code".to_owned(),
                };
                self.ensure_prefix();
                self.current.push(Span::styled(
                    format!("┌─ {label}"),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));
                self.flush_line();
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
            Tag::Link { dest_url, .. } => {
                self.link_destinations
                    .push(sanitize_terminal_text(&dest_url));
                self.styles.push(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::UNDERLINED),
                );
            }
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
            | Tag::MetadataBlock(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition => {}
            Tag::Image { dest_url, .. } => {
                self.image_destinations
                    .push(sanitize_terminal_text(&dest_url));
                self.push_span("![".to_owned(), Style::default().fg(Color::DarkGray));
            }
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
                self.lines
                    .push(Line::from(Span::styled("│ └─", Style::default().dim())));
                self.needs_blank = true;
            }
            TagEnd::List(_) => {
                self.flush_line();
                self.list_stack.pop();
                self.item_prefix = None;
                self.needs_blank = true;
            }
            TagEnd::Item => self.flush_line(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.styles.pop();
            }
            TagEnd::Link => {
                if let Some(destination) = self.link_destinations.pop()
                    && !destination.is_empty()
                {
                    self.push_span(
                        format!(" ({destination})"),
                        Style::default().fg(Color::DarkGray),
                    );
                }
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
            | TagEnd::MetadataBlock(_)
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition => {}
            TagEnd::Image => {
                let destination = self.image_destinations.pop().unwrap_or_default();
                self.push_span(
                    format!("]({destination})"),
                    Style::default().fg(Color::DarkGray),
                );
            }
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
                "│ ".repeat(self.quote_depth),
                Style::default().fg(Color::Green).dim(),
            ));
        }
        if self.in_code_block {
            self.current
                .push(Span::styled("│ ", Style::default().dim()));
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
    use super::{
        format_process_command, parse_tool_output, render_markdown, sanitize_terminal_text,
    };

    #[test]
    fn formats_quoted_and_structured_tool_output_for_humans() {
        assert_eq!(
            parse_tool_output(r#"\"ready\\nnow\""#).display_text(),
            "ready\nnow"
        );
        assert_eq!(
            parse_tool_output(r#"{\"ok\":true,\"items\":[1,2]}"#).display_text(),
            "items:\n  1\n  2\nok: true"
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
    fn quotes_process_arguments_only_for_display() {
        assert_eq!(
            format_process_command("node", &["script.js".to_owned(), "hello world".to_owned()]),
            "node script.js 'hello world'"
        );
    }

    #[test]
    fn separates_nested_process_streams_without_rendering_the_envelope() {
        let output = parse_tool_output(
            r#"{\"stdout\":\"ready\\nnow\",\"stderr\":\"warn\",\"status\":\"failure\",\"exit_code\":1}"#,
        );

        assert_eq!(output.stdout, "ready\nnow");
        assert_eq!(output.stderr, "warn");
        assert_eq!(output.success, Some(false));
        assert_eq!(output.display_text(), "ready\nnow\nwarn");
    }

    #[test]
    fn does_not_duplicate_an_error_only_tool_output() {
        let output = parse_tool_output(r#"{\"error\":\"failed\"}"#);

        assert_eq!(output.stdout, "");
        assert_eq!(output.stderr, "failed");
        assert_eq!(output.display_text(), "failed");
    }

    #[test]
    fn sanitizes_dynamic_command_and_inline_code_text() {
        let command = format_process_command("printf", &["\x1b[2J\nunsafe".to_owned()]);
        assert!(!command.contains('\x1b'));
        assert!(command.contains("\\nunsafe"));

        let lines = render_markdown("inline `\x1b[2Junsafe`");
        let text = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(!text.contains('\x1b'));
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
    fn preserves_link_and_image_destinations_in_terminal_markdown() {
        let lines = render_markdown(
            "[docs](https://example.com/docs) ![logo](https://example.com/logo.png)",
        );
        let text = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(text.contains("docs (https://example.com/docs)"));
        assert!(text.contains("![logo](https://example.com/logo.png)"));
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
