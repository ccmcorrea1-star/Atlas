use ratatui::text::Line;
use ratatui::text::Span;

/// Cria uma linha que empresta os spans de outra linha.
pub(crate) fn line_to_borrowed<'a>(line: &'a Line<'_>) -> Line<'a> {
    Line {
        style: line.style,
        alignment: line.alignment,
        spans: line
            .spans
            .iter()
            .map(|span| Span {
                style: span.style,
                content: std::borrow::Cow::Borrowed(span.content.as_ref()),
            })
            .collect(),
    }
}

/// Clona uma linha para uma linha owned estática.
pub(crate) fn line_to_static(line: &Line<'_>) -> Line<'static> {
    Line {
        style: line.style,
        alignment: line.alignment,
        spans: line
            .spans
            .iter()
            .map(|span| Span {
                style: span.style,
                content: std::borrow::Cow::Owned(span.content.to_string()),
            })
            .collect(),
    }
}

pub(crate) fn push_owned_lines<'a>(source: &[Line<'a>], target: &mut Vec<Line<'static>>) {
    target.extend(source.iter().map(line_to_static));
}

pub(crate) fn prefix_lines(
    lines: Vec<Line<'static>>,
    initial_prefix: Span<'static>,
    subsequent_prefix: Span<'static>,
) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let prefix = if index == 0 {
                initial_prefix.clone()
            } else {
                subsequent_prefix.clone()
            };
            let mut spans = Vec::with_capacity(line.spans.len() + 1);
            spans.push(prefix);
            spans.extend(line.spans);
            Line::from(spans).style(line.style)
        })
        .collect()
}
