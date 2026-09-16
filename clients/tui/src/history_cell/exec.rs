use ratatui::style::Stylize;
use ratatui::text::Line;

use super::HistoryCell;
use super::plain_lines;
use crate::wrapping::display_width;
use crate::wrapping::wrap_text;

/// Generic runtime tool activity rendered with the Codex activity gutter.
#[derive(Debug)]
pub(crate) struct ToolCell {
    tool_id: String,
    tool_name: String,
    output: Option<String>,
    completed: bool,
}

impl ToolCell {
    pub(crate) fn new(tool_id: String, tool_name: String) -> Self {
        Self {
            tool_id,
            tool_name,
            output: None,
            completed: false,
        }
    }

    pub(crate) fn tool_id(&self) -> &str {
        &self.tool_id
    }

    pub(crate) fn complete(&mut self, output: Option<String>) {
        self.output = output;
        self.completed = true;
    }
}

impl HistoryCell for ToolCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let _completed = self.completed;
        let mut lines = wrap_text(&format!("• {}", self.tool_name), usize::from(width.max(1)))
            .into_iter()
            .enumerate()
            .map(|(index, line)| {
                Line::from(if index == 0 {
                    line
                } else {
                    format!("  {line}")
                })
            })
            .collect::<Vec<_>>();
        if let Some(output) = &self.output {
            let output_width = usize::from(width.max(1)).saturating_sub(4).max(1);
            lines.extend(
                output
                    .lines()
                    .flat_map(|line| wrap_text(line, output_width))
                    .enumerate()
                    .map(|(index, line)| {
                        Line::from(format!(
                            "{}{}",
                            if index == 0 { "  └ " } else { "    " },
                            line
                        ))
                    }),
            );
        }
        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        plain_lines(self.display_lines(u16::MAX))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[derive(Debug)]
pub(crate) struct ErrorCell {
    pub(crate) message: String,
}

impl ErrorCell {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl HistoryCell for ErrorCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let prefix = "! ";
        let usable = usize::from(width)
            .saturating_sub(display_width(prefix))
            .max(1);
        let mut result = Vec::new();
        for (line_index, source) in self.message.lines().enumerate() {
            for (part_index, line) in wrap_text(source, usable).into_iter().enumerate() {
                result.push(Line::from(vec![
                    (if line_index == 0 && part_index == 0 {
                        prefix
                    } else {
                        "  "
                    })
                    .to_string()
                    .red()
                    .dim(),
                    line.into(),
                ]));
            }
        }
        result
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        plain_lines(self.message.lines().map(|line| Line::from(line.to_owned())))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
