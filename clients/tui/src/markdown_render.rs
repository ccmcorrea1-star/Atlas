//! Renderer de Markdown adaptado do pipeline de eventos da TUI do Codex.
//!
//! O renderer de eventos fica em `markdown_render`, mantendo a separacao do
//! Codex entre normalizacao da fonte e apresentacao no ratatui.

use std::borrow::Cow;

use pulldown_cmark::CodeBlockKind;
use pulldown_cmark::Event;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;
use pulldown_cmark::TagEnd;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use unicode_segmentation::UnicodeSegmentation;

use crate::ui_consts::action_style;
use crate::ui_consts::primary_style;
use crate::ui_consts::secondary_style;

#[path = "markdown_render/math.rs"]
mod math;

const ESC: char = '\x1b';
const ANSI_MARKER: &str = "␛";
// Mantem a metadata do link no mesmo grapheme sem expor controles ao ratatui.
const HYPERLINK_START: char = '\u{e0100}';
const HYPERLINK_DATA_START: u32 = 0xe0101;
const HYPERLINK_DATA_END: u32 = 0xe0110;
const HYPERLINK_END: char = '\u{e0111}';
const HYPERLINK_CONTINUE: char = '\u{e0112}';

#[derive(Debug)]
pub(crate) enum TerminalHyperlinkPart<'a> {
    Start {
        destination: String,
        visible: &'a str,
    },
    Continue {
        visible: &'a str,
    },
}

pub(crate) fn sanitize_terminal_text(text: &str) -> String {
    ansi_spans(text, Style::default())
        .into_iter()
        .map(|span| span.content.into_owned())
        .collect()
}

pub(crate) fn ansi_spans(text: &str, base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut style = base;
    let mut chunk = String::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if let Some(end) = hyperlink_metadata_end(text, index) {
            index = end;
            continue;
        }
        if (bytes[index] == ESC as u8 || text[index..].starts_with(ANSI_MARKER))
            && let Some((end, parameters, is_sgr)) = ansi_sequence(text, index)
        {
            push_chunk(&mut spans, &mut chunk, style);
            if is_sgr {
                apply_sgr(&mut style, base, parameters);
            }
            index = end;
            continue;
        }
        let character = text[index..].chars().next().unwrap_or_default();
        if character == '\t' {
            chunk.push_str("  ");
        } else if !character.is_control() || character == '\n' {
            chunk.push(character);
        }
        index += character.len_utf8();
    }
    push_chunk(&mut spans, &mut chunk, style);
    spans
}

pub(crate) fn render_ansi_line(text: &str, style: Style) -> Line<'static> {
    Line::from(ansi_spans(text, style))
}

pub(crate) fn format_process_command(program: &str, args: &[String]) -> String {
    if is_shell_wrapper(program, args) {
        return sanitize_terminal_text(&args[1]).replace('\n', "\\n");
    }
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(|argument| sanitize_terminal_text(argument).replace('\n', "\\n"))
        .map(shell_quote)
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_shell_wrapper(program: &str, args: &[String]) -> bool {
    let shell = program.rsplit(['/', '\\']).next().unwrap_or(program);
    matches!(shell.to_ascii_lowercase().as_str(), "bash" | "sh" | "zsh")
        && matches!(args.first().map(String::as_str), Some("-lc" | "-c"))
        && args.len() >= 2
}

pub(crate) fn unwrap_markdown_fences<'a>(input: &'a str) -> Cow<'a, str> {
    if !input.contains("```") && !input.contains("~~~") {
        return Cow::Borrowed(input);
    }

    let mut output = String::with_capacity(input.len());
    let mut lines = input.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let Some((marker, marker_len, info)) = fence_start(trimmed) else {
            output.push_str(line);
            continue;
        };
        let mut body = String::new();
        let mut closing = None;
        for candidate in lines.by_ref() {
            let candidate_trimmed = candidate.trim_end_matches(['\r', '\n']);
            if fence_end(candidate_trimmed, marker, marker_len) {
                closing = Some(candidate);
                break;
            }
            body.push_str(candidate);
        }
        let is_table = (info.eq_ignore_ascii_case("md") || info.eq_ignore_ascii_case("markdown"))
            && contains_table(&body);
        if is_table {
            output.push_str(&body);
        } else {
            output.push_str(line);
            output.push_str(&body);
            if let Some(closing) = closing {
                output.push_str(closing);
            }
        }
        if closing.is_none() {
            break;
        }
    }
    Cow::Owned(output)
}

fn fence_start(line: &str) -> Option<(char, usize, &str)> {
    let leading = line.bytes().take_while(|byte| *byte == b' ').count();
    if leading > 3 {
        return None;
    }
    let line = &line[leading..];
    let marker = if line.starts_with("```") {
        '`'
    } else if line.starts_with("~~~") {
        '~'
    } else {
        return None;
    };
    let marker_len = line
        .bytes()
        .take_while(|byte| *byte == marker as u8)
        .count();
    Some((marker, marker_len, line[marker_len..].trim()))
}

fn fence_end(line: &str, marker: char, marker_len: usize) -> bool {
    let line = line.trim();
    line.chars().all(|character| character == marker) && line.chars().count() >= marker_len
}

fn contains_table(body: &str) -> bool {
    let mut previous_header = false;
    for line in body.lines() {
        let line = crate::table_detect::strip_blockquote_prefix(line.trim());
        let is_header = crate::table_detect::is_table_header_line(line);
        if previous_header && crate::table_detect::is_table_delimiter_line(line) {
            return true;
        }
        previous_header = !line.is_empty() && is_header;
    }
    false
}

fn push_chunk(spans: &mut Vec<Span<'static>>, chunk: &mut String, style: Style) {
    if !chunk.is_empty() {
        spans.push(Span::styled(std::mem::take(chunk), style));
    }
}

pub(crate) fn encode_terminal_hyperlink(destination: &str, text: &str) -> String {
    let visible = if text.is_empty() { destination } else { text };
    let mut encoded = String::with_capacity(visible.len() + destination.len() * 2 + 8);
    for (index, grapheme) in visible.graphemes(true).enumerate() {
        encoded.push_str(grapheme);
        if index == 0 {
            encoded.push(HYPERLINK_START);
            for byte in destination.as_bytes() {
                encoded.push(
                    char::from_u32(HYPERLINK_DATA_START + u32::from(byte >> 4))
                        .expect("hyperlink data marker is a valid variation selector"),
                );
                encoded.push(
                    char::from_u32(HYPERLINK_DATA_START + u32::from(byte & 0x0f))
                        .expect("hyperlink data marker is a valid variation selector"),
                );
            }
            encoded.push(HYPERLINK_END);
        } else {
            encoded.push(HYPERLINK_CONTINUE);
        }
    }
    encoded
}

pub(crate) fn terminal_hyperlink_parts(symbol: &str) -> Option<TerminalHyperlinkPart<'_>> {
    if let Some(marker_index) = symbol.find(HYPERLINK_START) {
        let visible = &symbol[..marker_index];
        let marker_end = hyperlink_metadata_end(symbol, marker_index)?;
        if marker_end != symbol.len() {
            return None;
        }

        let metadata = &symbol[marker_index..marker_end];
        let mut chars = metadata.chars();
        chars.next();
        let mut bytes = Vec::new();
        loop {
            let high = chars.next()?;
            if high == HYPERLINK_END {
                break;
            }
            let low = chars.next()?;
            if !is_hyperlink_data(high) || !is_hyperlink_data(low) {
                return None;
            }
            bytes.push(
                ((high as u32 - HYPERLINK_DATA_START) << 4 | (low as u32 - HYPERLINK_DATA_START))
                    as u8,
            );
        }
        if chars.next().is_some() || bytes.is_empty() {
            return None;
        }

        return Some(TerminalHyperlinkPart::Start {
            destination: String::from_utf8(bytes).ok()?,
            visible,
        });
    }

    symbol
        .strip_suffix(HYPERLINK_CONTINUE)
        .map(|visible| TerminalHyperlinkPart::Continue { visible })
}

fn hyperlink_metadata_end(text: &str, start: usize) -> Option<usize> {
    let mut chars = text[start..].char_indices();
    let (_, marker) = chars.next()?;
    if marker == HYPERLINK_CONTINUE {
        return Some(start + marker.len_utf8());
    }
    if marker != HYPERLINK_START {
        return None;
    }

    let mut expect_low = false;
    for (offset, character) in chars {
        if character == HYPERLINK_END && !expect_low {
            return Some(start + offset + character.len_utf8());
        }
        if !is_hyperlink_data(character) {
            return None;
        }
        expect_low = !expect_low;
    }
    None
}

fn is_hyperlink_data(character: char) -> bool {
    (HYPERLINK_DATA_START..=HYPERLINK_DATA_END).contains(&(character as u32))
}

fn ansi_sequence(text: &str, start: usize) -> Option<(usize, &str, bool)> {
    let bytes = text.as_bytes();
    let marker_length = if bytes.get(start) == Some(&(ESC as u8)) {
        ESC.len_utf8()
    } else if text[start..].starts_with(ANSI_MARKER) {
        ANSI_MARKER.len()
    } else {
        return None;
    };
    let control_index = start + marker_length;
    if bytes.get(control_index) == Some(&b'[') {
        let final_index = (control_index + 1..bytes.len())
            .find(|index| (b'@'..=b'~').contains(&bytes[*index]))?;
        return Some((
            final_index + 1,
            &text[control_index + 1..final_index],
            bytes[final_index] == b'm',
        ));
    }
    if bytes.get(control_index) == Some(&b']') {
        for index in control_index + 1..bytes.len() {
            if bytes[index] == 0x07 {
                return Some((index + 1, "", false));
            }
            if bytes[index] == ESC as u8 && bytes.get(index + 1) == Some(&b'\\') {
                return Some((index + 2, "", false));
            }
            if bytes.get(index..index + ANSI_MARKER.len()) == Some(ANSI_MARKER.as_bytes())
                && bytes.get(index + ANSI_MARKER.len()) == Some(&b'\\')
            {
                return Some((index + ANSI_MARKER.len() + 1, "", false));
            }
        }
        return Some((bytes.len(), "", false));
    }
    Some(((control_index + 1).min(bytes.len()), "", false))
}

fn apply_sgr(style: &mut Style, base: Style, parameters: &str) {
    let values = if parameters.is_empty() {
        vec![0]
    } else {
        parameters
            .split(';')
            .map(|value| value.parse::<u16>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let mut index = 0;
    while index < values.len() {
        match values[index] {
            0 => *style = base,
            1 => *style = style.add_modifier(Modifier::BOLD),
            2 => *style = style.add_modifier(Modifier::DIM),
            3 => *style = style.add_modifier(Modifier::ITALIC),
            4 => *style = style.add_modifier(Modifier::UNDERLINED),
            7 => *style = style.add_modifier(Modifier::REVERSED),
            9 => *style = style.add_modifier(Modifier::CROSSED_OUT),
            22 => *style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => *style = style.remove_modifier(Modifier::ITALIC),
            24 => *style = style.remove_modifier(Modifier::UNDERLINED),
            27 => *style = style.remove_modifier(Modifier::REVERSED),
            29 => *style = style.remove_modifier(Modifier::CROSSED_OUT),
            30..=37 => *style = style.fg(ansi_color(values[index] - 30, false)),
            39 => *style = style.fg(Color::Reset),
            40..=47 => *style = style.bg(ansi_color(values[index] - 40, false)),
            49 => *style = style.bg(Color::Reset),
            90..=97 => *style = style.fg(ansi_color(values[index] - 90, true)),
            100..=107 => *style = style.bg(ansi_color(values[index] - 100, true)),
            38 | 48 if values.get(index + 1) == Some(&5) => {
                if let Some(value) = values.get(index + 2) {
                    let color = Color::Indexed((*value).min(255) as u8);
                    if values[index] == 38 {
                        *style = style.fg(color);
                    } else {
                        *style = style.bg(color);
                    }
                }
                index += 2;
            }
            38 | 48 if values.get(index + 1) == Some(&2) => {
                if let (Some(red), Some(green), Some(blue)) = (
                    values.get(index + 2),
                    values.get(index + 3),
                    values.get(index + 4),
                ) {
                    let color = Color::Rgb(
                        (*red).min(255) as u8,
                        (*green).min(255) as u8,
                        (*blue).min(255) as u8,
                    );
                    if values[index] == 38 {
                        *style = style.fg(color);
                    } else {
                        *style = style.bg(color);
                    }
                }
                index += 4;
            }
            _ => {}
        }
        index += 1;
    }
}

fn ansi_color(value: u16, bright: bool) -> Color {
    let colors = if bright {
        [
            Color::Gray,
            Color::LightRed,
            Color::LightGreen,
            Color::LightYellow,
            Color::LightBlue,
            Color::LightMagenta,
            Color::LightCyan,
            Color::White,
        ]
    } else {
        [
            Color::Black,
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Magenta,
            Color::Cyan,
            Color::Gray,
        ]
    };
    colors[usize::from(value.min(7))]
}

fn shell_quote(argument: String) -> String {
    if argument
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-._/:=".contains(character))
    {
        argument
    } else {
        format!("'{}'", argument.replace('\'', "'\\''"))
    }
}

mod writer {
    use super::*;

    pub(crate) fn render_markdown_text_with_width(
        input: &str,
        width: Option<usize>,
    ) -> Text<'static> {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TABLES);
        let math = math::MathMarkdown::new(input, options, width);
        let events = math.events(Parser::new_ext(&math.markdown, options).into_offset_iter());
        let mut writer = Writer::new(events, width);
        writer.run();
        Text::from(writer.lines)
    }

    struct TableState {
        header: Option<Vec<String>>,
        rows: Vec<Vec<String>>,
        current_row: Vec<String>,
        current_cell: String,
        in_header: bool,
    }

    struct Writer<'a, I>
    where
        I: Iterator<Item = Event<'a>>,
    {
        events: I,
        lines: Vec<Line<'static>>,
        current: Vec<Span<'static>>,
        styles: Vec<Style>,
        list_stack: Vec<Option<u64>>,
        item_prefix: Option<String>,
        quote_depth: usize,
        link_destinations: Vec<Option<String>>,
        in_code_block: bool,
        code_lang: Option<String>,
        code_buffer: String,
        needs_blank: bool,
        width: Option<usize>,
        table: Option<TableState>,
    }

    impl<'a, I> Writer<'a, I>
    where
        I: Iterator<Item = Event<'a>>,
    {
        fn new(events: I, width: Option<usize>) -> Self {
            Self {
                events,
                lines: Vec::new(),
                current: Vec::new(),
                styles: vec![Style::default()],
                list_stack: Vec::new(),
                item_prefix: None,
                quote_depth: 0,
                link_destinations: Vec::new(),
                in_code_block: false,
                code_lang: None,
                code_buffer: String::new(),
                needs_blank: false,
                width,
                table: None,
            }
        }

        fn run(&mut self) {
            while let Some(event) = self.events.next() {
                self.handle(event);
            }
            self.flush_open_code_block();
            self.flush_line();
            while self.lines.last().is_some_and(|line| line.spans.is_empty()) {
                self.lines.pop();
            }
        }

        fn flush_open_code_block(&mut self) {
            if !self.in_code_block {
                return;
            }
            let code = self
                .code_buffer
                .strip_suffix('\n')
                .unwrap_or(&self.code_buffer);
            let highlighted = self.render_code_block(code, self.code_lang.as_deref());
            let prefix = format!(
                "{}{}",
                "> ".repeat(self.quote_depth),
                "  ".repeat(self.list_stack.len())
            );
            for mut line in highlighted {
                let mut spans = vec![Span::styled(prefix.clone(), Style::default())];
                spans.append(&mut line.spans);
                self.lines.push(Line::from(spans).style(line.style));
            }
            self.in_code_block = false;
        }

        fn render_code_block(&self, code: &str, language: Option<&str>) -> Vec<Line<'static>> {
            language.map_or_else(
                || crate::render::highlight::highlight_code_to_lines(code, "text"),
                |language| {
                    if language.eq_ignore_ascii_case("mermaid")
                        && let Ok(diagram) = crate::mermaid::render(code, self.width.unwrap_or(120))
                    {
                        return diagram
                            .lines()
                            .map(|line| Line::from(line.to_owned()))
                            .collect();
                    }
                    crate::render::highlight::highlight_code_to_lines(code, language)
                },
            )
        }

        fn handle(&mut self, event: Event<'a>) {
            match event {
                Event::Start(tag) => self.start(tag),
                Event::End(tag) => self.end(tag),
                Event::Text(text) => self.push_text(&text),
                Event::Code(code) => self.push_styled_text(&code, action_style()),
                Event::SoftBreak | Event::HardBreak => self.flush_line(),
                Event::Rule => {
                    self.flush_line();
                    self.push_line(Line::from("———"));
                    self.needs_blank = true;
                }
                Event::Html(html) | Event::InlineHtml(html) => self.push_text(&html),
                Event::InlineMath(math) => {
                    let rendered = super::math::render_formula(&math, false)
                        .unwrap_or_else(|| math.to_string());
                    self.push_styled_text(&rendered, action_style());
                }
                Event::DisplayMath(math) => {
                    self.flush_line();
                    let rendered = super::math::render_formula(&math, true)
                        .unwrap_or_else(|| math.to_string());
                    for line in rendered.lines() {
                        self.push_line(Line::from(line.to_owned()));
                    }
                    self.needs_blank = true;
                }
                Event::FootnoteReference(_) | Event::TaskListMarker(_) => {}
            }
        }

        fn start(&mut self, tag: Tag<'a>) {
            match tag {
                Tag::Paragraph => {
                    if self.needs_blank && !self.lines.is_empty() {
                        self.push_line(Line::default());
                    }
                    self.needs_blank = false;
                }
                Tag::Heading { level, .. } => {
                    self.flush_line();
                    if !self.lines.is_empty() {
                        self.push_line(Line::default());
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
                        self.push_line(Line::default());
                    }
                    self.in_code_block = true;
                    self.code_lang = match kind {
                        CodeBlockKind::Fenced(language) => language
                            .split([',', ' ', '\t'])
                            .next()
                            .filter(|language| !language.is_empty())
                            .map(ToOwned::to_owned),
                        CodeBlockKind::Indented => None,
                    };
                    self.code_buffer.clear();
                }
                Tag::List(start) => self.list_stack.push(start),
                Tag::Item => {
                    if self.needs_blank && !self.lines.is_empty() && !self.list_stack.is_empty() {
                        self.push_line(Line::default());
                    }
                    self.flush_line();
                    let indent = "    ".repeat(self.list_stack.len().saturating_sub(1));
                    let marker = match self.list_stack.last_mut() {
                        Some(Some(index)) => {
                            let marker = format!("{index}. ");
                            *index += 1;
                            marker
                        }
                        _ => "- ".to_owned(),
                    };
                    self.item_prefix = Some(format!("{indent}{marker}"));
                    self.needs_blank = false;
                }
                Tag::Emphasis => self.styles.push(self.current_style().italic()),
                Tag::Strong => self.styles.push(self.current_style().bold()),
                Tag::Strikethrough => self.styles.push(self.current_style().crossed_out()),
                Tag::Link { dest_url, .. } => {
                    self.link_destinations
                        .push(safe_link_destination(&dest_url));
                    self.styles.push(
                        self.current_style()
                            .patch(action_style())
                            .add_modifier(Modifier::UNDERLINED),
                    );
                }
                Tag::Table(_) => {
                    self.flush_line();
                    if !self.lines.is_empty() {
                        self.push_line(Line::default());
                    }
                    self.table = Some(TableState {
                        header: None,
                        rows: Vec::new(),
                        current_row: Vec::new(),
                        current_cell: String::new(),
                        in_header: false,
                    });
                }
                Tag::TableHead => {
                    if let Some(table) = &mut self.table {
                        table.in_header = true;
                    }
                }
                Tag::TableRow => {
                    if let Some(table) = &mut self.table {
                        table.current_row.clear();
                    }
                }
                Tag::TableCell => {
                    if let Some(table) = &mut self.table {
                        table.current_cell.clear();
                    }
                }
                Tag::Image { .. } => {}
                Tag::HtmlBlock
                | Tag::FootnoteDefinition(_)
                | Tag::DefinitionList
                | Tag::DefinitionListTitle
                | Tag::DefinitionListDefinition
                | Tag::MetadataBlock(_) => {}
            }
        }

        fn end(&mut self, tag: TagEnd) {
            match tag {
                TagEnd::Paragraph => {
                    // Sem o flush a linha do paragrafo fica pendente e o proximo bloco
                    // concatena o proprio texto nela, grudando os dois paragrafos.
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
                    let code = std::mem::take(&mut self.code_buffer);
                    let code = code.strip_suffix('\n').unwrap_or(&code);
                    let highlighted = self.render_code_block(code, self.code_lang.as_deref());
                    let prefix = format!(
                        "{}{}",
                        "> ".repeat(self.quote_depth),
                        "  ".repeat(self.list_stack.len())
                    );
                    for mut line in highlighted {
                        let mut spans = vec![Span::styled(prefix.clone(), Style::default())];
                        spans.append(&mut line.spans);
                        self.lines.push(Line::from(spans).style(line.style));
                    }
                    self.in_code_block = false;
                    self.code_lang = None;
                    self.needs_blank = true;
                }
                TagEnd::List(_) => {
                    self.flush_line();
                    self.list_stack.pop();
                    self.item_prefix = None;
                    self.needs_blank = true;
                }
                TagEnd::Item => {
                    self.flush_line();
                    self.item_prefix = None;
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                    self.styles.pop();
                }
                TagEnd::Link => {
                    self.styles.pop();
                    self.link_destinations.pop();
                }
                TagEnd::Table => {
                    if let Some(table) = self.table.take() {
                        self.render_table(table);
                    }
                    self.needs_blank = true;
                }
                TagEnd::TableHead => {
                    if let Some(table) = &mut self.table {
                        table.header = Some(std::mem::take(&mut table.current_row));
                        table.in_header = false;
                    }
                }
                TagEnd::TableRow => {
                    if let Some(table) = &mut self.table {
                        let row = std::mem::take(&mut table.current_row);
                        if !table.in_header {
                            table.rows.push(row);
                        }
                    }
                }
                TagEnd::TableCell => {
                    if let Some(table) = &mut self.table {
                        table
                            .current_row
                            .push(std::mem::take(&mut table.current_cell));
                    }
                }
                TagEnd::Image
                | TagEnd::HtmlBlock
                | TagEnd::FootnoteDefinition
                | TagEnd::DefinitionList
                | TagEnd::DefinitionListTitle
                | TagEnd::DefinitionListDefinition
                | TagEnd::MetadataBlock(_) => {}
            }
        }

        fn push_text(&mut self, text: &str) {
            if self.table.is_some() {
                if let Some(table) = &mut self.table {
                    table.current_cell.push_str(text);
                }
                return;
            }
            if self.in_code_block {
                self.code_buffer.push_str(text);
                return;
            }
            self.push_styled_text(text, self.current_style());
        }

        fn push_styled_text(&mut self, text: &str, style: Style) {
            if let Some(table) = &mut self.table {
                table.current_cell.push_str(text);
                return;
            }
            for (index, part) in text.split('\n').enumerate() {
                if index > 0 {
                    self.flush_line();
                }
                if let Some(Some(destination)) = self.link_destinations.last().cloned() {
                    self.ensure_prefix();
                    self.current.push(Span::styled(
                        encode_terminal_hyperlink(&destination, &sanitize_terminal_text(part)),
                        style,
                    ));
                } else {
                    for span in ansi_spans(part, style) {
                        self.ensure_prefix();
                        self.current.push(span);
                    }
                }
            }
        }

        fn current_style(&self) -> Style {
            self.styles.last().copied().unwrap_or_default()
        }

        fn ensure_prefix(&mut self) {
            if !self.current.is_empty() {
                return;
            }
            if self.quote_depth > 0 {
                self.current.push(Span::styled(
                    "> ".repeat(self.quote_depth),
                    secondary_style(),
                ));
            }
            if let Some(prefix) = self.item_prefix.as_deref() {
                self.current.push(Span::styled(
                    prefix.to_owned(),
                    if prefix.trim_end().ends_with('.') {
                        Style::default().light_blue()
                    } else {
                        Style::default()
                    },
                ));
            }
        }

        fn flush_line(&mut self) {
            if self.current.is_empty() {
                return;
            }
            let line = Line::from(std::mem::take(&mut self.current));
            self.push_wrapped(line);
        }

        fn push_line(&mut self, line: Line<'static>) {
            self.lines.push(line);
        }

        fn push_wrapped(&mut self, line: Line<'static>) {
            let Some(width) = self.width.filter(|width| *width > 0) else {
                self.lines.push(line);
                return;
            };
            let prefix_width = leading_prefix_width(&line);
            let wrapped =
                crate::wrapping::wrap_line(line, width.saturating_sub(prefix_width).max(1));
            if wrapped.len() <= 1 {
                self.lines.extend(wrapped);
                return;
            }
            let continuation = " ".repeat(prefix_width);
            for (index, mut line) in wrapped.into_iter().enumerate() {
                if index > 0 {
                    line.spans.insert(0, Span::raw(continuation.clone()));
                }
                self.lines.push(line);
            }
        }

        fn render_table(&mut self, table: TableState) {
            let Some(mut header) = table.header else {
                return;
            };
            let columns = table
                .rows
                .iter()
                .map(Vec::len)
                .chain(std::iter::once(header.len()))
                .max()
                .unwrap_or(0);
            if columns == 0 {
                return;
            }
            header.resize(columns, String::new());
            let mut rows = table.rows;
            for row in &mut rows {
                row.resize(columns, String::new());
            }
            let mut widths = header
                .iter()
                .chain(rows.iter().flat_map(|row| row.iter()))
                .map(|cell| crate::wrapping::display_width(cell))
                .collect::<Vec<_>>();
            widths.truncate(columns);
            widths.resize(columns, 0);
            if let Some(width) = self.width {
                let budget = width.saturating_sub(columns.saturating_sub(1) * 2 + columns * 2);
                shrink_widths(&mut widths, budget);
            }
            self.lines.extend(render_table_row(
                &header,
                &widths,
                action_style().add_modifier(Modifier::BOLD),
            ));
            self.lines.push(
                Line::from(
                    widths
                        .iter()
                        .map(|width| "━".repeat((*width).max(3)))
                        .collect::<Vec<_>>()
                        .join("  "),
                )
                .style(secondary_style()),
            );
            for (index, row) in rows.iter().enumerate() {
                self.lines
                    .extend(render_table_row(row, &widths, primary_style()));
                if index + 1 < rows.len() {
                    self.lines.push(
                        Line::from(
                            widths
                                .iter()
                                .map(|width| "─".repeat((*width).max(3)))
                                .collect::<Vec<_>>()
                                .join("  "),
                        )
                        .style(secondary_style()),
                    );
                }
            }
        }
    }

    fn render_table_row(row: &[String], widths: &[usize], style: Style) -> Vec<Line<'static>> {
        let wrapped_cells = row
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                let width = widths.get(index).copied().unwrap_or(1).max(1);
                crate::wrapping::wrap_line(Line::from(cell.clone()), width.saturating_add(1))
                    .into_iter()
                    .map(|line| {
                        line.spans
                            .iter()
                            .map(|span| span.content.as_ref())
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let height = wrapped_cells.iter().map(Vec::len).max().unwrap_or(1);
        (0..height)
            .map(|line_index| {
                let mut text = String::new();
                for (index, width) in widths.iter().enumerate() {
                    if index > 0 {
                        text.push_str("  ");
                    }
                    let cell = wrapped_cells
                        .get(index)
                        .and_then(|lines| lines.get(line_index))
                        .map_or("", String::as_str);
                    text.push(' ');
                    text.push_str(cell);
                    let padding = width.saturating_sub(crate::wrapping::display_width(cell));
                    text.push_str(&" ".repeat(padding + 1));
                }
                Line::from(Span::styled(text.trim_end().to_owned(), style))
            })
            .collect()
    }

    fn shrink_widths(widths: &mut [usize], budget: usize) {
        while widths.iter().sum::<usize>() > budget {
            let Some((index, _)) = widths.iter().enumerate().max_by_key(|(_, width)| **width)
            else {
                break;
            };
            if widths[index] <= 3 {
                break;
            }
            widths[index] -= 1;
        }
    }

    fn leading_prefix_width(line: &Line<'static>) -> usize {
        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        let mut prefix_end = text
            .char_indices()
            .take_while(|(_, character)| character.is_whitespace())
            .map(|(index, character)| index + character.len_utf8())
            .last()
            .unwrap_or(0);
        loop {
            let rest = &text[prefix_end..];
            if let Some(after_quote) = rest.strip_prefix("> ") {
                prefix_end += rest.len() - after_quote.len();
                continue;
            }
            if let Some(after_marker) = rest.strip_prefix(['-', '*', '+'])
                && after_marker.starts_with(' ')
            {
                prefix_end += rest.len() - after_marker.len() + 1;
                break;
            }
            let Some(dot) = rest.find(". ") else {
                break;
            };
            if dot > 0
                && rest[..dot]
                    .chars()
                    .all(|character| character.is_ascii_digit())
            {
                prefix_end += dot + 2;
            }
            break;
        }
        crate::wrapping::display_width(&text[..prefix_end])
    }

    fn heading_style(level: pulldown_cmark::HeadingLevel) -> Style {
        match level {
            pulldown_cmark::HeadingLevel::H1 => {
                primary_style().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            }
            pulldown_cmark::HeadingLevel::H2 => primary_style().add_modifier(Modifier::BOLD),
            pulldown_cmark::HeadingLevel::H3 => {
                primary_style().add_modifier(Modifier::BOLD | Modifier::ITALIC)
            }
            _ => primary_style().add_modifier(Modifier::ITALIC),
        }
    }

    fn safe_link_destination(destination: &str) -> Option<String> {
        let destination = sanitize_terminal_text(destination);
        (!destination.is_empty() && !destination.chars().any(char::is_control))
            .then_some(destination)
    }
}

pub(crate) fn render_markdown_text(input: &str) -> Text<'static> {
    writer::render_markdown_text_with_width(input, None)
}

pub(crate) fn render_markdown_text_with_width(input: &str, width: Option<usize>) -> Text<'static> {
    writer::render_markdown_text_with_width(input, width)
}

#[cfg(test)]
mod tests {
    #[test]
    fn process_command_hides_shell_wrapper() {
        assert_eq!(
            super::format_process_command(
                "/bin/bash",
                &["-lc".to_owned(), "printf 'hello'".to_owned()]
            ),
            "printf 'hello'"
        );
    }

    fn rendered_lines(markdown: &str) -> Vec<String> {
        super::render_markdown_text(markdown)
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn consecutive_paragraphs_keep_their_blank_line() {
        assert_eq!(
            rendered_lines("primeiro paragrafo\n\nsegundo paragrafo"),
            vec![
                "primeiro paragrafo".to_owned(),
                String::new(),
                "segundo paragrafo".to_owned(),
            ]
        );
    }

    #[test]
    fn bold_title_line_and_the_next_paragraph_do_not_glue() {
        let lines = rendered_lines(
            "**Bridge de capabilities**\n\nO bridge de capabilities funciona como uma camada.",
        );

        assert_eq!(lines[0], "Bridge de capabilities");
        assert_eq!(lines[1], "");
        assert_eq!(
            lines[2],
            "O bridge de capabilities funciona como uma camada."
        );
        assert!(!lines.iter().any(|line| line.contains("capabilitiesO")));
    }

    #[test]
    fn multiple_blocks_keep_their_order_and_separation() {
        let lines = rendered_lines(
            "## Titulo\n\nPrimeiro paragrafo.\n\n- item\n- outro\n\n```rust\nlet x = 1;\n```\n\nFim.",
        );

        for expected in [
            "## Titulo",
            "Primeiro paragrafo.",
            "- item",
            "- outro",
            "let x = 1;",
            "Fim.",
        ] {
            assert!(
                lines.iter().any(|line| line.contains(expected)),
                "bloco ausente {expected:?}: {lines:?}"
            );
        }
        assert!(
            lines.iter().filter(|line| line.trim().is_empty()).count() >= 4,
            "blocos consecutivos devem manter linha em branco: {lines:?}"
        );
    }

    #[test]
    fn headings_use_a_contiguous_codex_prefix() {
        let rendered = super::render_markdown_text("## Result");
        let text = rendered.lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(text, "## Result");
    }

    #[test]
    fn code_block_inside_blockquote_keeps_quote_prefix() {
        let rendered = super::render_markdown_text("> ```rust\n> let answer = 42;\n> ```");
        let text = rendered
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert!(
            text.iter()
                .any(|line| line.starts_with("> ") && line.contains("answer"))
        );
    }

    #[test]
    fn code_block_inside_list_keeps_list_indent() {
        let rendered =
            super::render_markdown_text("- item\n\n  ```rust\n  let answer = 42;\n  ```");
        let text = rendered
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert!(
            text.iter()
                .any(|line| line.starts_with("  ") && line.contains("answer"))
        );
    }

    #[test]
    fn open_code_fence_is_rendered_before_the_closing_fence_arrives() {
        let rendered = super::render_markdown_text("```rust\nlet answer = 42;\n");
        let text = rendered
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert!(text.iter().any(|line| line.contains("let answer = 42;")));
        assert!(
            rendered
                .lines
                .iter()
                .flat_map(|line| line.spans.iter())
                .any(|span| span.style.fg.is_some())
        );
        insta::assert_snapshot!(text.join("\n"));
    }

    #[test]
    fn list_wrapping_preserves_item_indent() {
        let rendered = super::render_markdown_text_with_width(
            "- A deliberately long list item that wraps across multiple rows",
            Some(24),
        );
        let text = rendered
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(text[0].starts_with("- "));
        assert!(text.iter().skip(1).all(|line| line.starts_with("  ")));
        insta::assert_snapshot!("list_wrapping_preserves_item_indent", text.join("\n"));
    }

    #[test]
    fn tables_render_header_separator_and_rows() {
        let rendered = super::render_markdown_text_with_width(
            "| Name | Value |\n| --- | --- |\n| Atlas | 42 |",
            Some(30),
        );
        let text = rendered
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(text.iter().any(|line| line.contains("Name")));
        assert!(text.iter().any(|line| line.contains("Atlas")));
        assert!(text.iter().any(|line| line.contains('━')));
        insta::assert_snapshot!("tables_render_header_separator_and_rows", text.join("\n"));
    }

    #[test]
    fn rich_inline_content_stays_inside_table_cells() {
        let rendered = super::render_markdown_text(
            "| Name | Value |\n| --- | --- |\n| **Atlas** | `x^2` and $x^2$ |",
        );
        let text = rendered
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(text.contains("Atlas"));
        assert!(text.contains("x^2"));
        assert!(text.contains("x²"));
    }

    #[test]
    fn narrow_tables_wrap_cells_within_the_requested_width() {
        let rendered = super::render_markdown_text_with_width(
            "| Name | Value |\n| --- | --- |\n| A very long cell | Another very long cell |",
            Some(20),
        );
        assert!(rendered.lines.iter().all(|line| {
            let text = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>();
            crate::wrapping::display_width(&text) <= 20
        }));
    }

    #[test]
    fn nested_blockquote_list_preserves_structure() {
        let rendered =
            super::render_markdown_text_with_width("> - outer item\n>   - nested item", Some(40));
        let text = rendered
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!("nested_blockquote_list_preserves_structure", text);
    }

    #[test]
    fn math_uses_the_codex_unicode_renderer_with_literal_fallback() {
        let inline = super::render_markdown_text("Inline $x^2$ and $\\frac{1}{2}$");
        let inline_text = inline
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<String>();
        assert!(inline_text.contains("x²"));
        assert!(inline_text.contains("((1)/(2))"));

        let display = super::render_markdown_text("$$\\frac{1}{2}$$");
        let display_text = display
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(display_text.iter().any(|line| line.contains('─')));
        assert!(display_text.iter().any(|line| line.contains('1')));
        assert!(display_text.iter().any(|line| line.contains('2')));

        let fallback = super::render_markdown_text("$\\unknown{value}$");
        assert!(fallback.lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content.contains("\\unknown"))
        }));
    }

    #[test]
    fn math_scanner_supports_tex_delimiters_accents_and_named_delimiters() {
        let rendered = super::render_markdown_text(
            r"Inline \(\hat{x} + \varrho\) and display:

\[
\left\langle x \right\rangle
\]",
        );
        let text = rendered
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert!(text.iter().any(|line| line.contains("x\u{0302} + ϱ")));
        assert!(text.iter().any(|line| line.contains("⟨ x ⟩")));
    }

    #[test]
    fn code_and_link_contexts_keep_math_literal() {
        let rendered = super::render_markdown_text(r"`\(x^2\)` [\(x^2\)](https://example.test)");
        let text = rendered
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(text.contains(r"\(x^2\)"));
    }

    #[test]
    fn markdown_table_fence_unwrap_uses_canonical_table_rules() {
        assert_eq!(
            super::unwrap_markdown_fences(
                "```MD\n| Name | Value |\n| --- | --- |\n| Atlas | 42 |\n```"
            )
            .as_ref(),
            "| Name | Value |\n| --- | --- |\n| Atlas | 42 |\n"
        );
        assert_eq!(
            super::unwrap_markdown_fences("```md\n| Name | Value |\n| -- | -- |\n```").as_ref(),
            "```md\n| Name | Value |\n| -- | -- |\n```"
        );
    }

    #[test]
    fn mermaid_fences_use_the_codex_terminal_renderer_and_fallback_on_error() {
        let rendered =
            super::render_markdown_text("```mermaid\nflowchart LR; A[Start] --> B[Done]\n```");
        assert!(
            rendered
                .lines
                .iter()
                .any(|line| { line.spans.iter().any(|span| span.content.contains("Start")) })
        );
        assert!(
            rendered
                .lines
                .iter()
                .any(|line| { line.spans.iter().any(|span| span.content.contains('┌')) })
        );

        let fallback = super::render_markdown_text("```mermaid\nnot-supported syntax\n```");
        assert!(fallback.lines.iter().any(|line| {
            line.spans
                .iter()
                .any(|span| span.content.contains("not-supported"))
        }));
    }

    #[test]
    fn links_do_not_leak_osc8_sequences_into_transcript_text() {
        let rendered = super::render_markdown_text("[Atlas](https://example.com/atlas)");
        let text = rendered.lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(super::sanitize_terminal_text(&text), "Atlas");
        assert!(!text.contains("]8;;"));
        assert!(!text.chars().any(char::is_control));
        assert_eq!(crate::wrapping::display_width(&text), 5);
    }

    #[test]
    fn ansi_sanitizer_keeps_link_text_and_removes_osc8_controls() {
        let raw = "\x1b]8;;https://example.com/atlas\x07Atlas\x1b]8;;\x07";

        assert_eq!(super::sanitize_terminal_text(raw), "Atlas");
    }
}
