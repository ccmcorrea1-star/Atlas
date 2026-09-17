use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

use super::HistoryCell;
use super::markdown_render_cache::MarkdownRenderCache;
use super::plain_lines;
use crate::markdown::render_markdown_agent;
use crate::markdown::sanitize_terminal_text;
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
pub(crate) struct AgentMessageCell {
    pub(crate) message_id: String,
    pub(crate) markdown_source: String,
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
            completed: false,
            is_first_line,
        }
    }

    pub(crate) fn append(&mut self, delta: &str) {
        self.markdown_source.push_str(delta);
    }
}

impl HistoryCell for AgentMessageCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        render_agent_lines(&self.markdown_source, width, self.is_first_line)
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
