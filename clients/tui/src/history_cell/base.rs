use super::HistoryCell;
use ratatui::text::Line;

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct PlainHistoryCell {
    lines: Vec<Line<'static>>,
}

#[allow(dead_code)]
impl PlainHistoryCell {
    pub(crate) fn new(lines: Vec<Line<'static>>) -> Self {
        Self { lines }
    }
}

impl HistoryCell for PlainHistoryCell {
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        self.lines.clone()
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        plain_lines(self.lines.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct CompositeHistoryCell {
    parts: Vec<Box<dyn HistoryCell>>,
}

#[allow(dead_code)]
impl CompositeHistoryCell {
    pub(crate) fn new(parts: Vec<Box<dyn HistoryCell>>) -> Self {
        Self { parts }
    }
}

impl HistoryCell for CompositeHistoryCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        join_parts(self.parts.iter().map(|part| part.display_lines(width)))
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        join_parts(self.parts.iter().map(|part| part.raw_lines()))
    }

    fn has_stable_transcript_height(&self) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[allow(dead_code)]
fn join_parts(parts: impl Iterator<Item = Vec<Line<'static>>>) -> Vec<Line<'static>> {
    let mut result = Vec::new();
    let mut first = true;
    for mut part in parts {
        if part.is_empty() {
            continue;
        }
        if !first {
            result.push(Line::default());
        }
        result.append(&mut part);
        first = false;
    }
    result
}

pub(crate) fn plain_lines(lines: impl IntoIterator<Item = Line<'static>>) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| {
            Line::from(
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>(),
            )
        })
        .collect()
}
