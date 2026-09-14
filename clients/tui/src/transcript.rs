use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, Message, MessageRole, ToolCall};
use crate::presentation::{format_process_command, render_markdown, sanitize_terminal_text};
use crate::wrapping::{
    display_width, wrap_command_with_widths, wrap_lines, wrap_plain_no_hyphenation, wrap_text,
};

const AGENT_LABEL_PREFIX: &str = "  ";
const AGENT_BODY_PREFIX: &str = "  │ ";

#[derive(Debug, Clone)]
pub(crate) struct TranscriptLayoutCache {
    pub(crate) revision: u64,
    pub(crate) width: u16,
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) total_rows: usize,
}

pub(crate) struct ChatPanel;

impl ChatPanel {
    pub(crate) fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
        if area.is_empty() {
            return;
        }

        let block = Block::default()
            .title(" Chat ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        Transcript::draw(frame, app, inner);
    }
}

pub(crate) struct Transcript;

impl Transcript {
    pub(crate) fn draw(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
        if area.is_empty() {
            return;
        }

        let layout = if let Some(cache) = app.transcript_cache(area.width) {
            cache.clone()
        } else {
            let lines = Self::lines(app, area.width);
            let layout = TranscriptLayoutCache {
                revision: app.transcript_revision(),
                width: area.width,
                total_rows: wrapped_line_count(&lines, area.width),
                lines,
            };
            app.set_transcript_cache(layout.clone());
            layout
        };
        if layout.lines.is_empty() {
            return;
        }
        let visible_rows = usize::from(area.height);
        let max_scroll = layout.total_rows.saturating_sub(visible_rows);
        let scroll = max_scroll
            .saturating_sub(app.history_scroll())
            .min(u16::MAX as usize) as u16;
        frame.render_widget(
            Paragraph::new(Text::from(layout.lines))
                .wrap(Wrap { trim: false })
                .scroll((scroll, 0)),
            area,
        );
    }

    fn lines(app: &App, width: u16) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let mut previous_role = None;
        for message in app.messages() {
            if previous_role.is_some() {
                lines.push(Line::default());
            }
            if let Some(tool) = message.tool.as_ref() {
                lines.extend(ExecutionCell::lines(tool, width));
            } else {
                lines.extend(match message.role {
                    MessageRole::User => UserMessageCell::lines(message, width),
                    MessageRole::Atlas => AgentMessageCell::lines(message, width),
                    MessageRole::System => system_message_lines(message, width),
                    MessageRole::Tool => Vec::new(),
                });
            }
            previous_role = Some(message.role);
        }
        lines
    }
}

pub(crate) struct UserMessageCell;

impl UserMessageCell {
    fn lines(message: &Message, width: u16) -> Vec<Line<'static>> {
        let content = sanitize_terminal_text(&message.content);
        let content = content.trim_end_matches(['\r', '\n']);
        if content.is_empty() {
            return Vec::new();
        }

        let wrap_width = usize::from(width).saturating_sub(2).max(1);
        let mut lines = Vec::new();
        let mut first = true;
        for source_line in content.split('\n') {
            for line in wrap_text(source_line, wrap_width) {
                let prefix = if first { "› " } else { "  " };
                lines.push(prefixed_line(
                    Line::from(line),
                    prefix,
                    if first {
                        Style::default().fg(Color::Cyan).bold()
                    } else {
                        Style::default().fg(Color::Cyan)
                    },
                ));
                first = false;
            }
        }
        lines
    }
}

pub(crate) struct AgentMessageCell;

impl AgentMessageCell {
    fn lines(message: &Message, width: u16) -> Vec<Line<'static>> {
        let body = render_markdown(&message.content);
        if body.is_empty() {
            return Vec::new();
        }

        let wrap_width = usize::from(width)
            .saturating_sub(display_width(AGENT_BODY_PREFIX))
            .max(1);
        let mut lines = vec![prefixed_line(
            Line::from("Atlas"),
            AGENT_LABEL_PREFIX,
            Style::default().fg(Color::Cyan).bold(),
        )];
        for line in wrap_lines(body, wrap_width) {
            if line.width() == 0 {
                lines.push(Line::default());
                continue;
            }
            lines.push(prefixed_line(
                line,
                AGENT_BODY_PREFIX,
                Style::default().fg(Color::DarkGray),
            ));
        }
        lines
    }
}

pub(crate) struct ExecutionCell;

impl ExecutionCell {
    fn lines(tool: &ToolCall, width: u16) -> Vec<Line<'static>> {
        if tool.name == "process.exec" {
            return Self::process_lines(tool, width);
        }

        let title = if tool.completed { "Ran" } else { "Running" };
        let line = Line::from(vec![
            execution_marker(tool),
            Span::raw(" "),
            Span::styled(title, Style::default().bold()),
        ]);
        vec![line]
    }

    fn process_lines(tool: &ToolCall, width: u16) -> Vec<Line<'static>> {
        let command = tool
            .program
            .as_deref()
            .map(|program| format_process_command(program, &tool.args))
            .unwrap_or_default();
        let title = if tool.completed { "Ran" } else { "Running" };
        let mut header = Line::from(vec![
            execution_marker(tool),
            Span::raw(" "),
            Span::styled(title, Style::default().bold()),
        ]);
        let header_width = header.width();
        let mut lines = Vec::new();

        if !command.is_empty() {
            let command_lines = wrap_command_with_widths(
                &command,
                usize::from(width).saturating_sub(header_width).max(1),
                usize::from(width).saturating_sub(4).max(1),
            );
            if let Some(first) = command_lines.first() {
                header.push_span(" ");
                header.push_span(Span::styled(first.clone(), command_style()));
            }
            lines.push(header);
            let continuation_count = command_lines.len().saturating_sub(1);
            for continuation in command_lines.iter().skip(1).take(2) {
                lines.push(prefixed_line(
                    Line::from(Span::styled(continuation.clone(), command_style())),
                    "  │ ",
                    Style::default().dim(),
                ));
            }
            if continuation_count > 2 {
                lines.push(prefixed_line(
                    Line::from(format!("… +{} lines", continuation_count - 2)),
                    "  │ ",
                    Style::default().dim(),
                ));
            }
        } else {
            lines.push(header);
        }

        if tool.completed {
            append_output(&mut lines, tool, width);
            if let Some(duration) = tool.duration {
                if tool.success {
                    lines[0].push_span(format!(" · {}", format_duration(duration)).dim());
                } else {
                    let exit_code = tool
                        .exit_code
                        .map_or_else(|| "?".to_owned(), |code| code.to_string());
                    lines.push(prefixed_line(
                        Line::from(format!(
                            "✗ exit {exit_code} · {}",
                            format_duration(duration)
                        )),
                        "  ",
                        Style::default().fg(Color::Red).dim(),
                    ));
                }
            }
        } else {
            lines.push(prefixed_line(
                Line::from("aguardando..."),
                "  └ ",
                Style::default().dim(),
            ));
        }
        lines
    }
}

fn system_message_lines(message: &Message, width: u16) -> Vec<Line<'static>> {
    let prefix = "! ";
    let wrap_width = usize::from(width)
        .saturating_sub(display_width(prefix))
        .max(1);
    let mut lines = Vec::new();
    let mut first = true;
    for source_line in sanitize_terminal_text(&message.content).split('\n') {
        for line in wrap_text(source_line, wrap_width) {
            lines.push(prefixed_line(
                Line::from(line),
                if first { prefix } else { "  " },
                Style::default().fg(Color::Red).dim(),
            ));
            first = false;
        }
    }
    lines
}

fn execution_marker(tool: &ToolCall) -> Span<'static> {
    if !tool.completed {
        return Span::styled("•", Style::default().fg(Color::Cyan).bold());
    }
    Span::styled(
        "•",
        if tool.success {
            Style::default().fg(Color::Green).bold()
        } else {
            Style::default().fg(Color::Red).bold()
        },
    )
}

fn command_style() -> Style {
    Style::default().fg(Color::Cyan)
}

fn append_output(lines: &mut Vec<Line<'static>>, tool: &ToolCall, width: u16) {
    if tool.output.as_deref().unwrap_or_default().is_empty()
        && tool.stderr.as_deref().unwrap_or_default().is_empty()
    {
        lines.push(prefixed_line(
            Line::from("(no output)"),
            "  └ ",
            Style::default().dim(),
        ));
        return;
    }

    let output_width = usize::from(width).saturating_sub(4).max(1);
    let mut has_rendered_output = false;
    append_output_stream(
        lines,
        tool.output.as_deref().unwrap_or_default(),
        output_width,
        Style::default().dim(),
        &mut has_rendered_output,
    );
    append_output_stream(
        lines,
        tool.stderr.as_deref().unwrap_or_default(),
        output_width,
        Style::default().fg(Color::Red).dim(),
        &mut has_rendered_output,
    );
}

fn append_output_stream(
    lines: &mut Vec<Line<'static>>,
    output: &str,
    width: usize,
    style: Style,
    has_rendered_output: &mut bool,
) {
    if output.is_empty() {
        return;
    }

    for output_line in output.lines() {
        let prefix = if *has_rendered_output {
            "    "
        } else {
            "  └ "
        };
        let wrapped = wrap_plain_no_hyphenation(output_line, width);
        for (index, line) in wrapped.into_iter().enumerate() {
            lines.push(prefixed_line(
                Line::from(line),
                if *has_rendered_output || index > 0 {
                    "    "
                } else {
                    prefix
                },
                style,
            ));
            *has_rendered_output = true;
        }
    }
}

fn prefixed_line(mut line: Line<'static>, prefix: &str, style: Style) -> Line<'static> {
    let mut spans = vec![Span::styled(prefix.to_owned(), style)];
    spans.append(&mut line.spans);
    Line::from(spans).style(line.style)
}

fn wrapped_line_count(lines: &[Line<'static>], width: u16) -> usize {
    let width = usize::from(width.max(1));
    lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum()
}

fn format_duration(duration: Duration) -> String {
    if duration.as_millis() < 1_000 {
        return format!("{}ms", duration.as_millis());
    }
    if duration.as_secs() < 60 {
        return format!("{:.1}s", duration.as_secs_f64());
    }
    format!(
        "{}m {:02}s",
        duration.as_secs() / 60,
        duration.as_secs() % 60
    )
}
