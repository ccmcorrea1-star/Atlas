use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use unicode_width::UnicodeWidthStr;

use super::model::CommandOutput;
use super::model::ExecCell;
use super::model::ExecState;
use crate::animation::tool_frame;
use crate::history_cell::HistoryCell;
use crate::icons;
use crate::markdown::render_ansi_line;
use crate::render::highlight::highlight_bash_to_lines;
use crate::ui_consts::TRANSCRIPT_HINT;
use crate::ui_consts::action_style;
use crate::ui_consts::error_style;
use crate::ui_consts::primary_style;
use crate::ui_consts::running_style;
use crate::ui_consts::secondary_style;
use crate::ui_consts::success_style;
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

    fn transcript_animation_tick(&self) -> Option<u64> {
        self.is_running().then_some(self.frame() as u64)
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
        let command = highlight_bash_to_lines(&command);
        let mut lines = Vec::new();
        for line in command {
            lines.extend(prefixed_wrapped_lines_with_styles(
                line,
                &format!("{} ", icons::SHELL),
                "    ",
                width,
                action_style(),
            ));
        }

        if let Some(output) = self.output() {
            lines.extend(transcript_output_lines(output, width));

            if let Some(duration_ms) = self.duration_ms() {
                let mut result = if self.succeeded() {
                    Line::from(Span::styled(icons::SUCCESS, success_style()))
                } else {
                    Line::from(vec![
                        Span::styled(icons::ERROR, error_style()),
                        Span::styled(
                            self.exit_code()
                                .map(|code| format!(" ({code})"))
                                .unwrap_or_default(),
                            primary_style(),
                        ),
                    ])
                };
                result.push_span(Span::styled(
                    format!(" • {}", format_duration(duration_ms)),
                    secondary_style(),
                ));
                lines.push(result);
            }
        }

        lines
    }

    fn display_lines_inner(&self, width: u16) -> Vec<Line<'static>> {
        let width = width.max(1);
        let (marker_style, title) = match (self.state(), self.exit_code()) {
            (ExecState::Running, _) => (running_style(), "Running"),
            (ExecState::Ran, Some(0)) => (success_style(), "Ran"),
            (ExecState::Ran, Some(_)) => (error_style(), "Ran"),
            (ExecState::Ran, None) => (running_style(), "Ran"),
        };
        let marker = if self.is_running() {
            tool_frame("shell.exec", self.frame())
        } else if self.succeeded() {
            icons::SUCCESS
        } else {
            icons::ERROR
        };
        let mut header = Line::from(vec![
            Span::styled(marker, marker_style),
            Span::raw(" "),
            Span::styled(title, marker_style),
            Span::raw(" "),
        ]);
        let command = self.command();
        let available = usize::from(width).saturating_sub(header.width()).max(1);
        let command_lines = crate::wrapping::wrap_plain_no_hyphenation(&command, available);
        if let Some(first) = command_lines.first() {
            header.push_span(Span::styled(first.clone(), action_style()));
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
                secondary_style(),
            ));
        }
        let omitted = command_lines
            .len()
            .saturating_sub(COMMAND_CONTINUATION_MAX_LINES + 1);
        if omitted > 0 {
            lines.push(prefixed_line(
                Line::from(format!("… +{omitted} lines")),
                "  │ ",
                secondary_style(),
            ));
        }

        if self.state() == ExecState::Ran {
            let output = output_lines(self.output(), TOOL_CALL_MAX_LINES, width);
            lines.extend(output);
            if let Some(duration_ms) = self.duration_ms() {
                let result = if self.succeeded() {
                    Span::styled(icons::SUCCESS, success_style())
                } else {
                    Span::styled(icons::ERROR, error_style())
                };
                let code = self
                    .exit_code()
                    .filter(|code| *code != 0)
                    .map(|code| format!(" ({code})"))
                    .unwrap_or_default();
                lines.push(Line::from(vec![
                    result,
                    Span::styled(
                        format!("{code} • {}", format_duration(duration_ms)),
                        secondary_style(),
                    ),
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
            secondary_style(),
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
    let tail_lines = output.lines().rev().take(tail).collect::<Vec<_>>();
    raw_lines.extend(tail_lines.into_iter().rev());
    let mut rendered = Vec::new();
    for (index, raw) in raw_lines.iter().enumerate() {
        let prefix = if index == 0 { "  └ " } else { "    " };
        let style = secondary_style();
        rendered.extend(prefixed_wrapped_lines(
            render_ansi_line(raw, style),
            prefix,
            width,
        ));
    }
    let omitted_hint = (omitted > 0).then_some(omitted);
    ExecCell::truncate_lines_middle(
        &rendered,
        OUTPUT_MAX_LINES,
        width,
        omitted_hint,
        Some(Line::from("    ")),
    )
}

fn transcript_output_lines(output: &CommandOutput, width: u16) -> Vec<Line<'static>> {
    let (total, retained) = output.line_counts();
    let rendered = output
        .transcript_lines()
        .flat_map(|raw| {
            wrap_line(
                crate::markdown::render_ansi_line(raw.as_ref(), primary_style()),
                usize::from(width.max(1)),
            )
        })
        .collect::<Vec<_>>();
    ExecCell::truncate_lines_middle(
        &rendered,
        OUTPUT_MAX_LINES,
        width,
        Some(total.saturating_sub(retained)),
        None,
    )
}

fn prefixed_wrapped_lines(line: Line<'static>, prefix: &str, width: u16) -> Vec<Line<'static>> {
    prefixed_wrapped_lines_with_styles(line, prefix, "    ", width, secondary_style())
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

impl ExecCell {
    fn output_ellipsis_text(omitted: usize) -> String {
        format!("… +{omitted} lines ({TRANSCRIPT_HINT})")
    }

    fn output_ellipsis_line_with_prefix(
        omitted: usize,
        prefix: Option<&Line<'static>>,
    ) -> Line<'static> {
        let mut line = prefix.cloned().unwrap_or_default();
        line.push_span(Span::styled(
            Self::output_ellipsis_text(omitted),
            secondary_style(),
        ));
        line
    }

    fn output_ellipsis_row_count(
        omitted: usize,
        width: u16,
        prefix: Option<&Line<'static>>,
    ) -> usize {
        Paragraph::new(Text::from(vec![Self::output_ellipsis_line_with_prefix(
            omitted, prefix,
        )]))
        .wrap(Wrap { trim: false })
        .line_count(width)
        .max(1)
    }

    /// Trunca pela quantidade de linhas visuais, preservando início e fim.
    ///
    /// O limite do card é medido em rows do terminal, não em linhas lógicas.
    fn truncate_lines_middle(
        lines: &[Line<'static>],
        max_rows: usize,
        width: u16,
        omitted_hint: Option<usize>,
        ellipsis_prefix: Option<Line<'static>>,
    ) -> Vec<Line<'static>> {
        let width = width.max(1);
        if max_rows == 0 {
            return Vec::new();
        }
        let row_counts = lines
            .iter()
            .map(|line| {
                Paragraph::new(Text::from(vec![line.clone()]))
                    .wrap(Wrap { trim: false })
                    .line_count(width)
                    .max(1)
            })
            .collect::<Vec<_>>();
        let total_rows = row_counts.iter().sum::<usize>();
        if total_rows <= max_rows {
            return lines.to_vec();
        }

        let omitted_hint = omitted_hint.unwrap_or_default();
        let estimated_omitted =
            omitted_hint + lines.len().saturating_sub(usize::from(omitted_hint > 0));
        let prefix = ellipsis_prefix.as_ref();
        let ellipsis_rows = Self::output_ellipsis_row_count(estimated_omitted, width, prefix);
        if ellipsis_rows >= max_rows {
            return vec![Self::output_ellipsis_line_with_prefix(
                estimated_omitted,
                prefix,
            )];
        }

        let available_rows = max_rows - ellipsis_rows;
        let head_budget = available_rows / 2;
        let tail_budget = available_rows - head_budget;
        let mut head = Vec::new();
        let mut head_rows = 0;
        let mut head_end = 0;
        while head_end < lines.len() && head_rows + row_counts[head_end] <= head_budget {
            head.push(lines[head_end].clone());
            head_rows += row_counts[head_end];
            head_end += 1;
        }

        let mut tail_reversed = Vec::new();
        let mut tail_rows = 0;
        let mut tail_start = lines.len();
        while tail_start > head_end {
            let index = tail_start - 1;
            if tail_rows + row_counts[index] > tail_budget {
                break;
            }
            tail_reversed.push(lines[index].clone());
            tail_rows += row_counts[index];
            tail_start -= 1;
        }

        let additional = lines
            .len()
            .saturating_sub(head.len() + tail_reversed.len())
            .saturating_sub(usize::from(omitted_hint > 0));
        head.push(Self::output_ellipsis_line_with_prefix(
            omitted_hint + additional,
            prefix,
        ));
        head.extend(tail_reversed.into_iter().rev());
        head
    }
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
            "bash".to_owned(),
            vec!["-lc".to_owned(), "echo hello".to_owned()],
        );
        cell.complete(
            "hello\nworld".to_owned(),
            String::new(),
            0,
            12,
            "success".to_owned(),
        );
        assert!(cell.succeeded());

        let mut failed = ExecCell::new("failure".to_owned(), "bash".to_owned(), Vec::new());
        failed.complete(String::new(), String::new(), 0, 1, "error".to_owned());
        assert!(!failed.succeeded());

        let display = cell.display_lines(80);
        let display = display.iter().map(line_text).collect::<Vec<_>>();
        assert!(!display.iter().any(|line| line.contains("capability:")));
        assert!(!display.iter().any(|line| line.contains("cwd:")));
        assert!(!display.iter().any(|line| line.contains("target:")));

        let lines = cell.transcript_lines(80);
        let rendered = lines.iter().map(line_text).collect::<Vec<_>>();
        assert_eq!(rendered, [">_ echo hello", "hello", "world", "✓ • 12ms"]);
        insta::assert_snapshot!(rendered.join("\n"));
    }

    #[test]
    fn snapshots_running_shell_execution() {
        let cell = ExecCell::new(
            "execution".to_owned(),
            "bash".to_owned(),
            vec!["-lc".to_owned(), "npm test".to_owned()],
        );
        let rendered = cell
            .display_lines(60)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>();
        insta::assert_snapshot!("shell_running", rendered.join("\n"));
    }

    #[test]
    fn output_limit_is_measured_in_visual_rows() {
        let output = CommandOutput::new(
            0,
            (0..8)
                .map(|index| format!("line-{index}-abcdefghij"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let lines = output_lines(Some(&output), 8, 36);
        let rows = Paragraph::new(Text::from(lines.clone()))
            .wrap(Wrap { trim: false })
            .line_count(36);

        assert!(rows <= OUTPUT_MAX_LINES, "rendered {rows} rows: {lines:?}");
        assert!(
            lines
                .iter()
                .any(|line| line_text(line).to_ascii_lowercase().contains("transcript"))
        );
    }

    #[test]
    fn output_limit_preserves_head_and_tail() {
        let output = CommandOutput::new(
            0,
            (0..20)
                .map(|index| format!("line-{index}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let lines = output_lines(Some(&output), 20, 80);
        let text = lines.iter().map(line_text).collect::<Vec<_>>();

        assert!(text.iter().any(|line| line.contains("line-0")));
        assert!(text.iter().any(|line| line.contains("line-19")));
        assert!(text.iter().any(|line| line.contains("+")));
    }

    #[test]
    fn transcript_summarizes_large_aggregate_output() {
        let mut cell = ExecCell::new("execution".to_owned(), "bash".to_owned(), Vec::new());
        cell.complete(
            (0..20)
                .map(|index| format!("line-{index}"))
                .collect::<Vec<_>>()
                .join("\n"),
            String::new(),
            0,
            1,
            "success".to_owned(),
        );

        let rendered = cell
            .transcript_lines(80)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>();
        assert!(rendered.len() <= OUTPUT_MAX_LINES + 2);
        assert!(rendered.iter().any(|line| line.contains("line-0")));
        assert!(rendered.iter().any(|line| line.contains("line-19")));
        assert!(rendered.iter().any(|line| line.contains("transcript")));
    }
}
