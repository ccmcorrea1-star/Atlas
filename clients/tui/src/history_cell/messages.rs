use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use std::sync::Mutex;
use unicode_width::UnicodeWidthStr;

use super::HistoryCell;
use super::markdown_render_cache::MarkdownRenderCache;
use super::plain_lines;
use crate::animation::reasoning_frame;
use crate::icons;
use crate::markdown::render_markdown_agent;
use crate::markdown::sanitize_terminal_text;
use crate::render::highlight_streaming::StreamingCodeHighlighter;
use crate::ui_consts::action_style;
use crate::ui_consts::primary_style;
use crate::ui_consts::secondary_style;
use crate::ui_consts::thinking_style;
use crate::ui_consts::thought_body_style;
use crate::ui_consts::thought_style;
use crate::ui_consts::user_surface_style;
use crate::wrapping::wrap_line;
use crate::wrapping::wrap_text;
use std::time::Instant;

const USER_MESSAGE_VERTICAL_PADDING: usize = 1;

#[derive(Debug)]
pub(crate) struct UserHistoryCell {
    pub(crate) message: String,
}

pub(crate) fn user_message_style() -> Style {
    user_surface_style()
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
        let wrap_width = usize::from(width).saturating_sub(2).max(1);
        let mut result = vec![Line::default(); USER_MESSAGE_VERTICAL_PADDING];
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
                        action_style()
                            .bg(message_style.bg.unwrap_or_default())
                            .add_modifier(Modifier::BOLD)
                    } else {
                        secondary_style().bg(message_style.bg.unwrap_or_default())
                    },
                ));
            }
        }
        result.extend(std::iter::repeat_n(
            Line::default(),
            USER_MESSAGE_VERTICAL_PADDING,
        ));
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

/// Bloco de reasoning recolhido por padrao para nao competir com a resposta.
#[derive(Debug)]
pub(crate) struct ThoughtCell {
    reasoning_id: String,
    /// Texto acumulado do reasoning; base do titulo e do corpo.
    source: String,
    /// Titulo extraido do primeiro bloco em negrito, como no OpenCode.
    title: Option<String>,
    /// Corpo do reasoning sem o bloco de titulo.
    body: String,
    expanded: bool,
    started_at: Instant,
    duration_ms: Option<u64>,
    frame: usize,
}

impl ThoughtCell {
    pub(crate) fn new_with_id(reasoning_id: impl Into<String>, source: impl Into<String>) -> Self {
        let mut cell = Self {
            reasoning_id: reasoning_id.into(),
            source: String::new(),
            title: None,
            body: String::new(),
            expanded: false,
            started_at: Instant::now(),
            duration_ms: None,
            frame: 0,
        };
        cell.set_source(source.into());
        cell
    }

    pub(crate) fn reasoning_id(&self) -> &str {
        &self.reasoning_id
    }

    pub(crate) fn append(&mut self, delta: &str) {
        let mut source = std::mem::take(&mut self.source);
        source.push_str(delta);
        self.set_source(source);
    }

    /// O provider pode revelar o titulo no meio do stream; por isso o texto
    /// acumulado e reinterpretado a cada delta.
    fn set_source(&mut self, source: String) {
        self.title = reasoning_title(&source).0;
        self.body = reasoning_body(&source);
        self.source = source;
    }

    pub(crate) fn finish(&mut self) {
        if self.duration_ms.is_none() {
            self.duration_ms = Some(elapsed_millis(self.started_at));
        }
    }

    pub(crate) fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    pub(crate) fn is_running(&self) -> bool {
        self.duration_ms.is_none()
    }

    pub(crate) fn has_visible_content(&self) -> bool {
        self.title.is_some() || !self.body.is_empty()
    }

    pub(crate) fn toggle(&mut self) {
        self.expanded = !self.expanded;
    }
}

impl HistoryCell for ThoughtCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let title = self.title.as_deref().unwrap_or_default();
        let mut header = if self.is_running() {
            Line::from(vec![
                Span::styled(reasoning_frame(self.frame), thinking_style()),
                Span::styled(" Thinking", thinking_style()),
            ])
        } else {
            let marker = if self.expanded { "- " } else { "+ " };
            Line::from(vec![
                Span::styled(marker, thought_style()),
                Span::styled("Thought", thought_style()),
            ])
        };
        if !title.is_empty() {
            let style = if self.is_running() {
                thinking_style()
            } else {
                thought_style()
            };
            header.push_span(Span::styled(format!(": {title}"), style));
        }
        if !self.is_running() {
            header.push_span(Span::styled(
                format!(
                    " · {}",
                    format_reasoning_duration(self.duration_ms.unwrap_or(0))
                ),
                secondary_style(),
            ));
        }
        let mut lines = vec![header];
        if self.expanded {
            let content_width = usize::from(width).saturating_sub(2).max(1);
            lines.extend(
                wrap_text(&self.body, content_width)
                    .into_iter()
                    .map(|line| prefixed_line(Line::from(line), "  ", thought_body_style())),
            );
        }
        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        plain_lines(self.source.lines().map(|line| Line::from(line.to_owned())))
    }

    fn transcript_animation_tick(&self) -> Option<u64> {
        self.is_running().then_some(self.frame as u64)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn elapsed_millis(started_at: Instant) -> u64 {
    started_at
        .elapsed()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn reasoning_title(source: &str) -> (Option<String>, Option<(usize, usize)>) {
    if let Some(start) = source.find("**") {
        let content_start = start + 2;
        if let Some(relative_end) = source[content_start..].find("**") {
            let end = content_start + relative_end + 2;
            let title = source[content_start..content_start + relative_end]
                .trim()
                .to_owned();
            if !title.is_empty() {
                return (Some(title), Some((start, end)));
            }
        }
        return (None, None);
    }

    let line_end = source.find('\n').unwrap_or(source.len());
    let title = source[..line_end].trim();
    if title.is_empty() {
        (None, None)
    } else {
        (Some(title.to_owned()), Some((0, line_end)))
    }
}

fn reasoning_body(source: &str) -> String {
    let (_, range) = reasoning_title(source);
    let body = range.map_or_else(
        || source.to_owned(),
        |(start, end)| format!("{}{}", &source[..start], &source[end..]),
    );
    sanitize_terminal_text(body.trim())
}

fn format_reasoning_duration(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms}ms")
    } else if duration_ms < 60_000 {
        format!("{:.1}s", duration_ms as f64 / 1_000.0)
    } else {
        format!(
            "{}m {:.1}s",
            duration_ms / 60_000,
            (duration_ms % 60_000) as f64 / 1_000.0
        )
    }
}

#[derive(Debug)]
pub(crate) struct CancelledCell;

impl HistoryCell for CancelledCell {
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        vec![Line::from(vec![
            Span::styled(icons::CANCELLED, secondary_style()),
            Span::styled(" Cancelled", secondary_style()),
        ])]
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
    /// Ha uma fronteira de bloco entre a regiao estavel e o tail.
    block_break: bool,
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
            block_break: false,
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
                true,
            );
            if !self.stream_tail_source.is_empty() {
                if self.block_break {
                    lines.push(blank_block_separator());
                }
                lines.extend(render_stream_part(
                    &self.tail_render_cache,
                    &self.stream_tail_source,
                    width,
                    false,
                    self.tail_revision,
                    false,
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
    pub(crate) fn set_markdown_source(&mut self, source: impl Into<String>) {
        self.markdown_source = source.into();
        self.clear_stream_parts();
    }

    /// Atualiza as partes do stream; `block_break` marca que o tail abre um bloco novo.
    pub(crate) fn set_stream_parts(&mut self, source: &str, stable_len: usize, block_break: bool) {
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
        self.block_break = block_break;
    }

    pub(crate) fn clear_stream_parts(&mut self) {
        self.stream_stable_source = None;
        self.stream_tail_source.clear();
        self.block_break = false;
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
    pub(crate) fn set_markdown_source(&mut self, markdown_source: impl Into<String>) {
        self.markdown_source = markdown_source.into();
        self.rendered_lines.clear();
    }

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

fn blank_block_separator() -> Line<'static> {
    prefixed_line(Line::default(), "  ", secondary_style())
}

/// Renderiza uma parte do stream mantendo o cache incremental.
///
/// `block_separator_on_append` vale para a regiao estavel, que cresce por blocos completos:
/// sem a linha em branco a materializacao incremental colaria dois blocos consecutivos.
fn render_stream_part(
    cache: &Mutex<Option<StreamingRenderCache>>,
    source: &str,
    width: u16,
    first: bool,
    revision: u64,
    block_separator_on_append: bool,
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
            if appended.ends_with('\n') && safe_incremental_suffix(appended) {
                if block_separator_on_append
                    && !cached.lines.is_empty()
                    && !appended.trim().is_empty()
                {
                    cached.lines.push(blank_block_separator());
                }
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
                            .push(prefixed_line(part, "  ", secondary_style()));
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
    let marker = if opening.starts_with("```") {
        "```"
    } else if opening.starts_with("~~~") {
        "~~~"
    } else {
        return None;
    };
    let language = opening.strip_prefix(marker)?.trim();
    if language.is_empty()
        || opening.contains(&format!("{marker}{marker}"))
        || body.lines().any(|line| line.trim().starts_with(marker))
    {
        return None;
    }
    Some((language.to_owned(), body))
}

fn safe_incremental_suffix(source: &str) -> bool {
    source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .all(|line| {
            let trimmed = line.trim_start();
            let starts_ordered_list = trimmed.find('.').is_some_and(|dot| {
                dot > 0
                    && trimmed[..dot]
                        .chars()
                        .all(|character| character.is_ascii_digit())
            });
            !trimmed.starts_with(['-', '*', '+', '>', '|', '`', '~', '<']) && !starts_ordered_list
        })
}

fn render_agent_lines(source: &str, width: u16, first: bool) -> Vec<Line<'static>> {
    let usable_width = usize::from(width).max(1);
    let rendered = render_markdown_agent(source, None);
    let mut result = Vec::new();
    for (line_index, line) in rendered.into_iter().enumerate() {
        let prefix = if first && line_index == 0 { "" } else { "  " };
        let wrap_width = usable_width.saturating_sub(prefix.width()).max(1);
        for (part_index, part) in wrap_line(line, wrap_width).into_iter().enumerate() {
            let part_style = part.style;
            let part = part.style(primary_style().patch(part_style));
            result.push(prefixed_line(
                part,
                if line_index == 0 && part_index == 0 {
                    prefix
                } else {
                    "  "
                },
                secondary_style(),
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

    fn cell_rows(lines: Vec<Line<'static>>) -> Vec<String> {
        lines.iter().map(line_text).collect()
    }

    #[test]
    fn agent_cell_keeps_the_separator_between_consecutive_paragraphs() {
        let cell = AgentMarkdownCell::with_message_id(
            None,
            "**Bridge de capabilities**\n\nO bridge de capabilities funciona como uma camada.",
        );

        let rows = cell_rows(cell.display_lines(70));

        assert_eq!(rows[0], "Bridge de capabilities");
        assert_eq!(rows[1], "  ");
        assert_eq!(
            rows[2],
            "  O bridge de capabilities funciona como uma camada."
        );
        assert!(!rows.iter().any(|row| row.contains("capabilitiesO")));
    }

    #[test]
    fn incremental_stable_append_keeps_the_separator_between_blocks() {
        let mut cell = AgentMessageCell::new("stable-blocks".to_owned(), "", true);
        cell.set_stream_parts("primeiro paragrafo\n\n", 20, false);
        let _ = cell.display_lines(60);
        cell.set_stream_parts("primeiro paragrafo\n\nsegundo paragrafo\n\n", 41, true);
        let rows = cell_rows(cell.display_lines(60));

        assert_eq!(rows[0], "primeiro paragrafo");
        assert_eq!(rows[1], "  ");
        assert_eq!(rows[2], "  segundo paragrafo");
    }

    #[test]
    fn materialized_tail_starts_with_the_block_separator() {
        let mut cell = AgentMessageCell::new("tail-block".to_owned(), "", false);
        cell.set_stream_parts("texto do bloco", 0, true);
        let rows = cell_rows(cell.display_lines(60));

        assert_eq!(rows[0], "  ");
        assert_eq!(rows[1], "  texto do bloco");
    }

    #[test]
    fn final_source_update_invalidates_same_width_markdown_cache() {
        let mut cell = AgentMarkdownCell::with_message_id(Some("message".to_owned()), "old");
        let first = cell.display_lines(80);
        assert!(first.iter().any(|line| line_text(line).contains("old")));
        cell.set_markdown_source("new");
        let second = cell.display_lines(80);
        assert!(second.iter().any(|line| line_text(line).contains("new")));
        assert!(!second.iter().any(|line| line_text(line).contains("old")));
    }

    #[test]
    fn streaming_renderer_consumes_stable_region_and_mutable_tail() {
        let mut cell = AgentMessageCell::new("message".to_owned(), "", true);
        cell.set_stream_parts("intro\n**tail**", "intro\n".len(), true);

        let lines = cell.display_lines(80);
        assert!(cell.stable_render_cache.lock().unwrap().is_some());
        assert!(cell.tail_render_cache.lock().unwrap().is_some());
        let stable_revision = cell.stable_revision;
        cell.set_stream_parts("intro\n**tail2**", "intro\n".len(), true);
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
        cell.set_stream_parts("intro\n", 6, false);
        let _ = cell.display_lines(80);
        cell.set_stream_parts("intro\nsecond\n", 13, false);
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
    fn context_sensitive_list_suffix_falls_back_to_full_render() {
        let mut cell = AgentMessageCell::new("list".to_owned(), "", true);
        cell.set_stream_parts("- first\n", 8, false);
        let _ = cell.display_lines(80);
        cell.set_stream_parts("- first\n- second\n", 18, false);
        let _ = cell.display_lines(80);

        let cache = cell.stable_render_cache.lock().unwrap();
        assert_eq!(
            cache.as_ref().map(|cache| cache.incremental_appends),
            Some(0)
        );
    }

    #[test]
    fn tilde_code_stream_appends_to_the_existing_highlighter_cache() {
        let mut cell = AgentMessageCell::new("tilde-code".to_owned(), "", true);
        cell.set_stream_parts("~~~rust\nlet answer = 42;\n", 0, false);
        let _ = cell.display_lines(80);
        cell.set_stream_parts("~~~rust\nlet answer = 42;\nprintln!(\"ok\");\n", 0, false);
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

    #[test]
    fn open_code_stream_appends_to_the_existing_highlighter_cache() {
        let mut cell = AgentMessageCell::new("code".to_owned(), "", true);
        cell.set_stream_parts("```rust\nlet answer = 42;\n", 0, false);
        let _ = cell.display_lines(80);
        cell.set_stream_parts("```rust\nlet answer = 42;\nprintln!(\"ok\");\n", 0, false);
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

    #[test]
    fn snapshots_user_message_and_agent_response_hierarchy() {
        let user = UserHistoryCell::new("Review the auth flow\nand keep the error path visible.");
        insta::assert_snapshot!(
            "user_message",
            format!(
                "height: {}\n{}",
                user.desired_height(60),
                user.display_lines(60)
                    .iter()
                    .map(line_text)
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        );

        let agent = AgentMarkdownCell::with_message_id(
            Some("agent-1".to_owned()),
            "## Ready\n\nThe error path is covered by the patch.",
        );
        insta::assert_snapshot!(
            "agent_response",
            agent
                .display_lines(60)
                .iter()
                .map(line_text)
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn adds_vertical_breathing_room_to_user_messages() {
        let message = UserHistoryCell::new("Review the auth flow");
        let lines = message.display_lines(60);

        assert_eq!(lines.len(), 3);
        assert!(lines[0].spans.is_empty());
        assert_eq!(line_text(&lines[1]), "› Review the auth flow");
        assert!(lines[2].spans.is_empty());
    }

    #[test]
    fn thought_extracts_bold_title_and_formats_short_duration_in_milliseconds() {
        let mut thought = ThoughtCell::new_with_id(
            "reasoning-1",
            "**Inspect the error path**\n\nThe existing handler drops the cause.",
        );
        thought.duration_ms = Some(625);

        assert_eq!(thought.reasoning_id(), "reasoning-1");
        assert_eq!(thought.title.as_deref(), Some("Inspect the error path"));
        assert_eq!(thought.body, "The existing handler drops the cause.");
        assert_eq!(
            line_text(&thought.display_lines(60)[0]),
            "+ Thought: Inspect the error path · 625ms"
        );

        thought.toggle();
        let lines = thought
            .display_lines(60)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>();
        assert!(lines.iter().any(|line| line.contains("existing handler")));
        assert!(!lines.iter().any(|line| line.contains("**Inspect")));
    }

    #[test]
    fn running_thought_shows_the_thinking_frame_with_the_title() {
        let mut thought = ThoughtCell::new_with_id(
            "reasoning-1",
            "**Completing todo updates**\n\nChecking every pending item before the answer.",
        );

        assert_eq!(
            line_text(&thought.display_lines(60)[0]),
            "⠋ Thinking: Completing todo updates"
        );

        thought.append(" The list is complete.");

        assert_eq!(
            line_text(&thought.display_lines(60)[0]),
            "⠋ Thinking: Completing todo updates"
        );
        assert!(thought.body.contains("The list is complete."));
        assert_eq!(thought.display_lines(60).len(), 1);
    }

    #[test]
    fn thought_title_and_body_are_rebuilt_as_deltas_arrive() {
        let mut thought = ThoughtCell::new_with_id("reasoning-2", "**Completing");

        assert_eq!(thought.title, None);
        assert_eq!(thought.body, "**Completing");

        thought.append(" todo updates**\n\nStart by listing the pending items.");

        assert_eq!(thought.title.as_deref(), Some("Completing todo updates"));
        assert_eq!(thought.body, "Start by listing the pending items.");
    }

    #[test]
    fn snapshots_active_collapsed_and_expanded_thought() {
        let thought = ThoughtCell::new_with_id("reasoning-1", "**Completing todo updates**");
        insta::assert_snapshot!(
            "thought_active",
            thought
                .display_lines(60)
                .iter()
                .map(line_text)
                .collect::<Vec<_>>()
                .join("\n")
        );

        let mut thought = ThoughtCell::new_with_id(
            "reasoning-1",
            "**Completing todo updates**\n\nCheck the existing error handling before editing.",
        );
        thought.duration_ms = Some(625);
        assert!(!thought.expanded);
        assert_eq!(
            thought.display_lines(60)[0].spans[1].style.fg,
            Some(crate::ui_consts::COLOR_THOUGHT)
        );
        insta::assert_snapshot!(
            "thought_closed",
            thought
                .display_lines(60)
                .iter()
                .map(line_text)
                .collect::<Vec<_>>()
                .join("\n")
        );

        thought.toggle();
        assert!(thought.expanded);
        assert_eq!(
            thought.display_lines(60)[1].spans[0].style.fg,
            Some(crate::ui_consts::COLOR_THOUGHT_BODY)
        );
        insta::assert_snapshot!(
            "thought_open",
            thought
                .display_lines(60)
                .iter()
                .map(line_text)
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn uses_explicit_atlas_colors_for_reasoning_and_response() {
        let running = ThoughtCell::new_with_id("reasoning-1", "**Organizing tools for clarity**");
        assert_eq!(
            running.display_lines(60)[0].spans[0].style.fg,
            Some(crate::ui_consts::COLOR_THINKING)
        );

        let response = AgentMarkdownCell::with_message_id(None, "A final answer.");
        assert_eq!(
            response.display_lines(60)[0].style.fg,
            Some(crate::ui_consts::COLOR_TEXT_PRIMARY)
        );
    }
}
