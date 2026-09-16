use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

use super::model::CommandOutput;
use super::model::ExecCell;
use super::model::ExecState;
use crate::history_cell::HistoryCell;
use crate::markdown::render_ansi_line;
use crate::ui_consts::TRANSCRIPT_HINT;
use crate::wrapping::wrap_line;

pub(crate) const TOOL_CALL_MAX_LINES: usize = 5;
const COMMAND_CONTINUATION_MAX_LINES: usize = 2;
const OUTPUT_MAX_LINES: usize = 5;

impl HistoryCell for ExecCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.display_lines_inner(width)
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.transcript_lines_inner(width)
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.transcript_lines(u16::MAX)
            .into_iter()
            .map(|line| {
                Line::from(
                    line.spans
                        .into_iter()
                        .map(|span| span.content)
                        .collect::<String>(),
                )
            })
            .collect()
    }

    fn has_stable_transcript_height(&self) -> bool {
        self.state() == ExecState::Ran
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl ExecCell {
    fn transcript_lines_inner(&self, width: u16) -> Vec<Line<'static>> {
        let width = width.max(1);
        let command = self.command();
        let command = crate::markdown::render_ansi_line(&command, Style::default());
        let mut lines = prefixed_wrapped_lines_with_styles(
            command,
            "$ ",
            "    ",
            width,
            Style::default().magenta(),
        );

        if let Some(output) = self.output() {
            for raw in output.transcript_lines() {
                lines.extend(wrap_line(
                    crate::markdown::render_ansi_line(raw.as_ref(), Style::default()),
                    usize::from(width),
                ));
            }

            if let Some(duration_ms) = self.duration_ms() {
                let mut result = if self.succeeded() {
                    Line::from("✓".green().bold())
                } else {
                    Line::from(vec![
                        "✗".red().bold(),
                        self.exit_code()
                            .map(|code| format!(" ({code})"))
                            .unwrap_or_default()
                            .into(),
                    ])
                };
                result.push_span(format!(" • {}", format_duration(duration_ms)).dim());
                lines.push(result);
            }
        }

        lines
    }

    fn display_lines_inner(&self, width: u16) -> Vec<Line<'static>> {
        let width = width.max(1);
        let (marker, title) = match (self.state(), self.exit_code()) {
            (ExecState::Running, _) => ("•".cyan().bold(), "Running"),
            (ExecState::Ran, Some(0)) => ("•".green().bold(), "Ran"),
            (ExecState::Ran, Some(_)) => ("•".red().bold(), "Ran"),
            (ExecState::Ran, None) => ("•".cyan().bold(), "Ran"),
        };
        let mut header = Line::from(vec![marker, " ".into(), title.bold(), " ".into()]);
        let command = self.command();
        let available = usize::from(width).saturating_sub(header.width()).max(1);
        let command_lines = crate::wrapping::wrap_plain_no_hyphenation(&command, available);
        if let Some(first) = command_lines.first() {
            header.push_span(first.clone().cyan());
        }

        let mut lines = vec![header];
        let continuation = command_lines
            .iter()
            .skip(1)
            .take(COMMAND_CONTINUATION_MAX_LINES);
        for line in continuation {
            lines.push(prefixed_line(
                Line::from(line.clone()),
                "  │ ",
                Style::default().dim(),
            ));
        }
        let omitted = command_lines
            .len()
            .saturating_sub(COMMAND_CONTINUATION_MAX_LINES + 1);
        if omitted > 0 {
            lines.push(prefixed_line(
                Line::from(format!("… +{omitted} lines")),
                "  │ ",
                Style::default().dim(),
            ));
        }

        if !self.capability().is_empty() {
            lines.push(prefixed_line(
                Line::from(format!("capability: {}", self.capability())),
                "  │ ",
                Style::default().dim(),
            ));
        }
        if let Some(cwd) = self.cwd() {
            lines.push(prefixed_line(
                Line::from(format!("cwd: {cwd}")),
                "  │ ",
                Style::default().dim(),
            ));
        }
        if let Some(target) = self.target() {
            lines.push(prefixed_line(
                Line::from(format!("target: {target}")),
                "  │ ",
                Style::default().dim(),
            ));
        }

        if self.state() == ExecState::Ran {
            let output = output_lines(self.output(), TOOL_CALL_MAX_LINES, width);
            lines.extend(output);
            if let Some(duration_ms) = self.duration_ms() {
                let result = if self.succeeded() {
                    "✓".green().bold()
                } else {
                    "✗".red().bold()
                };
                let code = self
                    .exit_code()
                    .filter(|code| *code != 0)
                    .map(|code| format!(" ({code})"))
                    .unwrap_or_default();
                lines.push(Line::from(vec![
                    result,
                    format!("{code} • {}", format_duration(duration_ms)).dim(),
                ]));
            }
        }
        lines
    }
}

fn output_lines(
    output: Option<&CommandOutput>,
    line_limit: usize,
    width: u16,
) -> Vec<Line<'static>> {
    let Some(output) = output else {
        return vec![prefixed_line(
            Line::from("(no output)"),
            "  └ ",
            Style::default().dim(),
        )];
    };
    let _exit_code = output.exit_code;
    let (total, retained) = output.line_counts();
    let head = total.min(line_limit).min(retained);
    let mut raw_lines = output.lines().take(head).collect::<Vec<_>>();
    let tail = total
        .saturating_sub(head)
        .min(line_limit)
        .min(retained.saturating_sub(head));
    let omitted = total.saturating_sub(head + tail);
    if omitted > 0 {
        raw_lines.push(std::borrow::Cow::Owned(format!(
            "… +{omitted} lines ({TRANSCRIPT_HINT})"
        )));
    }
    let tail_lines = output.lines().rev().take(tail).collect::<Vec<_>>();
    raw_lines.extend(tail_lines.into_iter().rev());
    let mut rendered = Vec::new();
    for (index, raw) in raw_lines.iter().enumerate() {
        let prefix = if index == 0 { "  └ " } else { "    " };
        let style = Style::default().dim();
        rendered.extend(prefixed_wrapped_lines(
            render_ansi_line(raw, style),
            prefix,
            width,
        ));
    }
    truncate_middle(rendered, OUTPUT_MAX_LINES)
}

fn prefixed_wrapped_lines(line: Line<'static>, prefix: &str, width: u16) -> Vec<Line<'static>> {
    prefixed_wrapped_lines_with_styles(line, prefix, "    ", width, Style::default().dim())
}

fn prefixed_wrapped_lines_with_styles(
    line: Line<'static>,
    prefix: &str,
    continuation: &str,
    width: u16,
    prefix_style: Style,
) -> Vec<Line<'static>> {
    let wrap_width = usize::from(width)
        .saturating_sub(prefix.width().max(continuation.width()))
        .max(1);
    wrap_line(line, wrap_width)
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            prefixed_line(
                line,
                if index == 0 { prefix } else { continuation },
                prefix_style,
            )
        })
        .collect()
}

fn prefixed_line(mut line: Line<'static>, prefix: &str, style: Style) -> Line<'static> {
    let mut spans = vec![Span::styled(prefix.to_owned(), style)];
    spans.append(&mut line.spans);
    Line::from(spans).style(line.style)
}

fn truncate_middle(lines: Vec<Line<'static>>, max_lines: usize) -> Vec<Line<'static>> {
    if lines.len() <= max_lines {
        return lines;
    }
    let omitted = lines.len() - max_lines + 1;
    let marker = prefixed_line(
        Line::from(format!("… +{omitted} lines ({TRANSCRIPT_HINT})")),
        "    ",
        Style::default().dim(),
    );
    let keep = max_lines.saturating_sub(1);
    let head = keep / 2;
    let tail = keep - head;
    let mut result = lines[..head].to_vec();
    result.push(marker);
    result.extend(lines[lines.len() - tail..].iter().cloned());
    result
}

fn format_duration(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms}ms")
    } else if duration_ms < 60_000 {
        format!("{:.1}s", duration_ms as f64 / 1_000.0)
    } else {
        format!("{}m {:02}s", duration_ms / 60_000, duration_ms / 1_000 % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn transcript_uses_codex_expanded_command_and_result() {
        let mut cell = ExecCell::new(
            "execution".to_owned(),
            "process.exec".to_owned(),
            "bash".to_owned(),
            vec!["-lc".to_owned(), "echo hello".to_owned()],
            Some("/tmp".to_owned()),
            Some("local".to_owned()),
        );
        cell.complete(
            "hello\nworld".to_owned(),
            String::new(),
            0,
            12,
            "success".to_owned(),
        );
        assert!(cell.succeeded());

        let mut failed = ExecCell::new(
            "failure".to_owned(),
            "process.exec".to_owned(),
            "bash".to_owned(),
            Vec::new(),
            None,
            None,
        );
        failed.complete(String::new(), String::new(), 0, 1, "error".to_owned());
        assert!(!failed.succeeded());

        let display = cell.display_lines(80);
        let display = display.iter().map(line_text).collect::<Vec<_>>();
        assert!(
            display
                .iter()
                .any(|line| line.contains("capability: process.exec"))
        );
        assert!(display.iter().any(|line| line.contains("cwd: /tmp")));
        assert!(display.iter().any(|line| line.contains("target: local")));

        let lines = cell.transcript_lines(80);
        let rendered = lines.iter().map(line_text).collect::<Vec<_>>();
        assert_eq!(rendered, ["$ echo hello", "hello", "world", "✓ • 12ms"]);
    }
}
