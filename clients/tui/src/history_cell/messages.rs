use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use std::sync::Mutex;
use unicode_width::UnicodeWidthStr;

use super::HistoryCell;
use super::markdown_render_cache::MarkdownRenderCache;
use super::plain_lines;
use crate::markdown::render_markdown_agent;
use crate::markdown::sanitize_terminal_text;
use crate::render::highlight_streaming::StreamingCodeHighlighter;
use crate::wrapping::wrap_line;

#[derive(Debug)]
pub(crate) struct UserHistoryCell {
    pub(crate) message: String,
}

const USER_MESSAGE_BACKGROUND: Color = Color::Rgb(51, 51, 51);

pub(crate) fn user_message_style() -> Style {
    Style::default().bg(USER_MESSAGE_BACKGROUND)
}

impl UserHistoryCell {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl HistoryCell for UserHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let message = sanitize_terminal_text(self.message.trim_end_matches(['\r', '\n']));
        if message.is_empty() {
            return Vec::new();
        }
        let message_style = user_message_style();
        let wrap_width = usize::from(width).saturating_sub(3).max(1);
        let mut result = vec![Line::from("").style(message_style)];
        for (line_index, source) in message.split('\n').enumerate() {
            let wrapped = wrap_line(
                crate::markdown::render_ansi_line(source, message_style),
                wrap_width,
            );
            for (part_index, line) in wrapped.into_iter().enumerate() {
                let first = line_index == 0 && part_index == 0;
                result.push(prefixed_line(
                    line,
                    if first { "› " } else { "  " },
                    if first {
                        message_style.add_modifier(Modifier::BOLD | Modifier::DIM)
                    } else {
                        message_style.dim()
                    },
                ));
            }
        }
        result.push(Line::from("").style(message_style));
        result
    }

    fn background_style(&self) -> Option<Style> {
        Some(user_message_style())
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        let message = sanitize_terminal_text(self.message.trim_end_matches(['\r', '\n']));
        plain_lines(message.split('\n').map(|line| Line::from(line.to_owned())))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[derive(Debug)]
struct StreamingRenderCache {
    width: u16,
    revision: u64,
    source: String,
    lines: Vec<Line<'static>>,
    open_code: Option<OpenCodeCache>,
    incremental_appends: usize,
}

#[derive(Debug)]
struct OpenCodeCache {
    language: String,
    body_len: usize,
    highlighter: StreamingCodeHighlighter,
}

#[derive(Debug)]
pub(crate) struct AgentMessageCell {
    pub(crate) message_id: String,
    pub(crate) markdown_source: String,
    stream_stable_source: Option<String>,
    stream_tail_source: String,
    stable_revision: u64,
    tail_revision: u64,
    stable_render_cache: Mutex<Option<StreamingRenderCache>>,
    tail_render_cache: Mutex<Option<StreamingRenderCache>>,
    pub(crate) completed: bool,
    pub(crate) is_first_line: bool,
}

impl AgentMessageCell {
    pub(crate) fn new(
        message_id: String,
        markdown_source: impl Into<String>,
        is_first_line: bool,
    ) -> Self {
        Self {
            message_id,
            markdown_source: markdown_source.into(),
            stream_stable_source: None,
            stream_tail_source: String::new(),
            stable_revision: 0,
            tail_revision: 0,
            stable_render_cache: Mutex::new(None),
            tail_render_cache: Mutex::new(None),
            completed: false,
            is_first_line,
        }
    }
}

impl HistoryCell for AgentMessageCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if let Some(stable) = &self.stream_stable_source {
            let mut lines = render_stream_part(
                &self.stable_render_cache,
                stable,
                width,
                self.is_first_line,
                self.stable_revision,
            );
            if !self.stream_tail_source.is_empty() {
                lines.extend(render_stream_part(
                    &self.tail_render_cache,
                    &self.stream_tail_source,
                    width,
                    false,
                    self.tail_revision,
                ));
            }
            lines
        } else {
            render_agent_lines(&self.markdown_source, width, self.is_first_line)
        }
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        plain_lines(render_markdown_agent(&self.markdown_source, None))
    }

    fn has_stable_transcript_height(&self) -> bool {
        self.completed
    }

    fn is_stream_continuation(&self) -> bool {
        !self.is_first_line
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl AgentMessageCell {
    pub(crate) fn set_stream_parts(&mut self, source: &str, stable_len: usize) {
        let stable_len = stable_len.min(source.len());
        let stable = source[..stable_len].to_owned();
        let tail = source[stable_len..].to_owned();
        if self.stream_stable_source.as_deref() != Some(stable.as_str()) {
            self.stable_revision = self.stable_revision.wrapping_add(1);
        }
        if self.stream_tail_source != tail {
            self.tail_revision = self.tail_revision.wrapping_add(1);
        }
        self.markdown_source.clear();
        self.markdown_source.push_str(source);
        self.stream_stable_source = Some(stable);
        self.stream_tail_source = tail;
    }

    pub(crate) fn clear_stream_parts(&mut self) {
        self.stream_stable_source = None;
        self.stream_tail_source.clear();
        self.stable_revision = self.stable_revision.wrapping_add(1);
        self.tail_revision = self.tail_revision.wrapping_add(1);
        self.stable_render_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        self.tail_render_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}

#[derive(Debug)]
pub(crate) struct AgentMarkdownCell {
    pub(crate) message_id: Option<String>,
    pub(crate) markdown_source: String,
    rendered_lines: MarkdownRenderCache,
}

impl AgentMarkdownCell {
    pub(crate) fn with_message_id(
        message_id: Option<String>,
        markdown_source: impl Into<String>,
    ) -> Self {
        Self {
            message_id,
            markdown_source: markdown_source.into(),
            rendered_lines: MarkdownRenderCache::default(),
        }
    }
}

impl HistoryCell for AgentMarkdownCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.rendered_lines.render(width, || {
            render_agent_lines(&self.markdown_source, width, true)
        })
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        plain_lines(render_markdown_agent(&self.markdown_source, None))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[allow(dead_code)]
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct StreamingAgentTailCell {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) is_first_line: bool,
}

#[allow(dead_code)]
impl StreamingAgentTailCell {
    pub(crate) fn new(lines: Vec<Line<'static>>, is_first_line: bool) -> Self {
        Self {
            lines,
            is_first_line,
        }
    }
}

impl HistoryCell for StreamingAgentTailCell {
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        self.lines
            .clone()
            .into_iter()
            .enumerate()
            .map(|(index, line)| {
                prefixed_line(
                    line,
                    if index == 0 && self.is_first_line {
                        "• "
                    } else {
                        "  "
                    },
                    Style::default().dim(),
                )
            })
            .collect()
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        plain_lines(self.lines.clone())
    }

    fn is_stream_continuation(&self) -> bool {
        !self.is_first_line
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn render_stream_part(
    cache: &Mutex<Option<StreamingRenderCache>>,
    source: &str,
    width: u16,
    first: bool,
    revision: u64,
) -> Vec<Line<'static>> {
    let mut cache_guard = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(cached) = cache_guard.as_mut() {
        if cached.width == width && cached.revision == revision {
            return cached.lines.clone();
        }
        if cached.width == width && cached.open_code.is_none() && source.starts_with(&cached.source)
        {
            let appended = &source[cached.source.len()..];
            if appended.is_empty() {
                cached.source = source.to_owned();
                cached.revision = revision;
                return cached.lines.clone();
            }
            if appended.ends_with('\n') {
                cached
                    .lines
                    .extend(render_agent_lines(appended, width, false));
                cached.incremental_appends = cached.incremental_appends.saturating_add(1);
                cached.source = source.to_owned();
                cached.revision = revision;
                return cached.lines.clone();
            }
        }
        if cached.width == width
            && source.starts_with(&cached.source)
            && let Some((language, body)) = open_code_parts(source)
            && let Some(open_code) = cached.open_code.as_mut()
            && open_code.language == language
            && body.len() >= open_code.body_len
        {
            let appended = &body[open_code.body_len..];
            if appended.is_empty() {
                cached.source = source.to_owned();
                cached.revision = revision;
                return cached.lines.clone();
            }
            let placeholder = StreamingCodeHighlighter::new("", &language)
                .expect("known streaming language should initialize");
            let highlighter = std::mem::replace(&mut open_code.highlighter, placeholder);
            if let Some((highlighter, appended_lines)) = highlighter.append(appended) {
                let wrap_width = usize::from(width).saturating_sub(2).max(1);
                for line in appended_lines {
                    for part in wrap_line(line, wrap_width) {
                        cached
                            .lines
                            .push(prefixed_line(part, "  ", Style::default().dim()));
                    }
                }
                open_code.body_len = body.len();
                open_code.highlighter = highlighter;
                cached.incremental_appends = cached.incremental_appends.saturating_add(1);
                cached.source = source.to_owned();
                cached.revision = revision;
                return cached.lines.clone();
            }
        }
    }

    let lines = render_agent_lines(source, width, first);
    let open_code = open_code_parts(source).and_then(|(language, body)| {
        StreamingCodeHighlighter::new(body, language.as_str()).map(|highlighter| OpenCodeCache {
            language,
            body_len: body.len(),
            highlighter,
        })
    });
    *cache_guard = Some(StreamingRenderCache {
        width,
        revision,
        source: source.to_owned(),
        lines: lines.clone(),
        open_code,
        incremental_appends: 0,
    });
    lines
}

fn open_code_parts(source: &str) -> Option<(String, &str)> {
    let (opening, body) = source.split_once('\n')?;
    let language = opening.strip_prefix("```")?.trim();
    if language.is_empty()
        || opening.contains("````")
        || body.lines().any(|line| line.trim().starts_with("```"))
    {
        return None;
    }
    Some((language.to_owned(), body))
}

fn render_agent_lines(source: &str, width: u16, first: bool) -> Vec<Line<'static>> {
    let usable_width = usize::from(width).saturating_sub(2).max(1);
    let rendered = render_markdown_agent(source, None);
    let mut result = Vec::new();
    for (line_index, line) in rendered.into_iter().enumerate() {
        let prefix = if first && line_index == 0 {
            "• "
        } else {
            "  "
        };
        let wrap_width = usable_width.saturating_sub(prefix.width()).max(1);
        for (part_index, part) in wrap_line(line, wrap_width).into_iter().enumerate() {
            result.push(prefixed_line(
                part,
                if line_index == 0 && part_index == 0 {
                    prefix
                } else {
                    "  "
                },
                Style::default().dim(),
            ));
        }
    }
    result
}

fn prefixed_line(mut line: Line<'static>, prefix: &str, style: Style) -> Line<'static> {
    let mut spans = vec![Span::styled(prefix.to_owned(), style)];
    spans.append(&mut line.spans);
    Line::from(spans).style(line.style)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn streaming_renderer_consumes_stable_region_and_mutable_tail() {
        let mut cell = AgentMessageCell::new("message".to_owned(), "", true);
        cell.set_stream_parts("intro\n**tail**", "intro\n".len());

        let lines = cell.display_lines(80);
        assert!(cell.stable_render_cache.lock().unwrap().is_some());
        assert!(cell.tail_render_cache.lock().unwrap().is_some());
        let stable_revision = cell.stable_revision;
        cell.set_stream_parts("intro\n**tail2**", "intro\n".len());
        let _ = cell.display_lines(80);
        assert_eq!(cell.stable_revision, stable_revision);
        let rendered = lines.iter().map(line_text).collect::<Vec<_>>();
        assert!(rendered.iter().any(|line| line.contains("intro")));
        assert!(rendered.iter().any(|line| line.contains("tail")));
        assert!(
            rendered
                .iter()
                .filter(|line| line.starts_with("• "))
                .count()
                <= 1
        );
    }

    #[test]
    fn stable_stream_appends_only_the_new_complete_region() {
        let mut cell = AgentMessageCell::new("stable".to_owned(), "", true);
        cell.set_stream_parts("intro\n", 6);
        let _ = cell.display_lines(80);
        cell.set_stream_parts("intro\nsecond\n", 13);
        let rendered = cell.display_lines(80);

        let cache = cell.stable_render_cache.lock().unwrap();
        assert_eq!(
            cache.as_ref().map(|cache| cache.incremental_appends),
            Some(1)
        );
        assert!(
            rendered
                .iter()
                .any(|line| line_text(line).contains("second"))
        );
    }

    #[test]
    fn open_code_stream_appends_to_the_existing_highlighter_cache() {
        let mut cell = AgentMessageCell::new("code".to_owned(), "", true);
        cell.set_stream_parts("```rust\nlet answer = 42;\n", 0);
        let _ = cell.display_lines(80);
        cell.set_stream_parts("```rust\nlet answer = 42;\nprintln!(\"ok\");\n", 0);
        let rendered = cell.display_lines(80);

        let cache = cell.tail_render_cache.lock().unwrap();
        assert_eq!(
            cache.as_ref().map(|cache| cache.incremental_appends),
            Some(1)
        );
        assert!(
            rendered
                .iter()
                .any(|line| line_text(line).contains("println!"))
        );
    }
}
