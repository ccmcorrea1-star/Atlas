//! Helpers de Markdown usados pelo renderer compativel com o Codex.

use ratatui::text::Line;

pub(crate) use crate::markdown_render::format_process_command;
pub(crate) use crate::markdown_render::render_ansi_line;
pub(crate) use crate::markdown_render::sanitize_terminal_text;
pub(crate) use crate::markdown_render::unwrap_markdown_fences;

#[allow(dead_code)]
pub(crate) fn render_markdown(input: &str) -> Vec<Line<'static>> {
    crate::markdown_render::render_markdown_text(input).lines
}

pub(crate) fn render_markdown_agent(input: &str, width: Option<usize>) -> Vec<Line<'static>> {
    crate::markdown_render::render_markdown_text_with_width(&unwrap_markdown_fences(input), width)
        .lines
}

#[allow(dead_code)]
pub(crate) fn append_markdown(input: &str, width: Option<usize>, lines: &mut Vec<Line<'static>>) {
    lines.extend(crate::markdown_render::render_markdown_text_with_width(input, width).lines);
}
