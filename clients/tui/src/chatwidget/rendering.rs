use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;

use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::app::App;
use crate::bottom_pane::BottomPaneView;
use crate::history_cell::HistoryCell;
use crate::render::renderable::Renderable;

pub(crate) fn render(
    area: Rect,
    buffer: &mut Buffer,
    app: &mut App,
    bottom_pane: &dyn BottomPaneView,
) -> Option<(u16, u16)> {
    if area.is_empty() {
        return None;
    }
    let composer_height = bottom_pane
        .desired_height(app, area.width)
        .min(area.height.saturating_sub(1));
    let [history_area, composer_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(composer_height)]).areas(area);

    render_history(buffer, app, history_area);
    bottom_pane.render(app, composer_area, buffer);
    let cursor = bottom_pane.cursor_pos(app, composer_area);
    if app.transcript_open() {
        app.transcript_overlay().render(app, area, buffer);
    }
    cursor
}

fn render_history(buffer: &mut Buffer, app: &mut App, area: Rect) {
    if area.is_empty() {
        return;
    }
    let width = area.width.max(1);
    let mut cells = Vec::new();
    let mut total_height = 0usize;
    for cell in app.cells().iter().chain(app.active_cells().iter()) {
        let rendered = TranscriptAreaRenderable {
            child: cell.as_ref(),
            top: if cells.is_empty() || cell.is_stream_continuation() {
                0
            } else {
                1
            },
            right: 0,
        };
        let height = usize::from(rendered.desired_height(width));
        cells.push((total_height, height, rendered));
        total_height = total_height.saturating_add(height);
    }
    Clear.render(area, buffer);
    let max_scroll = total_height.saturating_sub(usize::from(area.height));
    let scroll = max_scroll
        .saturating_sub(app.history_scroll())
        .min(max_scroll);
    for (start, height, rendered) in cells {
        let start = start as isize - scroll as isize;
        let end = start.saturating_add(height as isize);
        let viewport_height = isize::try_from(area.height).unwrap_or(isize::MAX);
        if end <= 0 || start >= viewport_height {
            continue;
        }
        let skip = usize::try_from(start.saturating_neg()).unwrap_or(0);
        let y = area
            .y
            .saturating_add(u16::try_from(start.max(0)).unwrap_or(0));
        let visible_height = usize::try_from(end.min(viewport_height))
            .unwrap_or(0)
            .saturating_sub(skip)
            .min(usize::from(area.height));
        if visible_height == 0 {
            continue;
        }
        let cell_area = Rect::new(
            area.x,
            y,
            area.width,
            u16::try_from(visible_height).unwrap_or(u16::MAX),
        );
        if skip == 0 || !rendered.render_scrolled(cell_area, buffer, skip as u16) {
            rendered.render(cell_area, buffer);
        }
    }
    app.record_history_content_height(total_height);
}

struct TranscriptAreaRenderable<'a> {
    child: &'a dyn HistoryCell,
    top: u16,
    right: u16,
}

impl Renderable for TranscriptAreaRenderable<'_> {
    fn render(&self, area: Rect, buffer: &mut Buffer) {
        let child_area = self.child_area(area);
        if child_area.is_empty() {
            return;
        }
        Paragraph::new(self.child.display_lines(child_area.width)).render(child_area, buffer);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.top.saturating_add(
            self.child
                .desired_height(width.saturating_sub(self.right).max(1)),
        )
    }

    fn render_scrolled(&self, area: Rect, buffer: &mut Buffer, scroll_offset: u16) -> bool {
        let top_visible = self.top.saturating_sub(scroll_offset);
        let child_offset = scroll_offset.saturating_sub(self.top);
        let child_area = Rect::new(
            area.x,
            area.y.saturating_add(top_visible),
            area.width.saturating_sub(self.right).max(1),
            area.height.saturating_sub(top_visible),
        );
        if child_area.is_empty() {
            return true;
        }
        Paragraph::new(self.child.display_lines(child_area.width))
            .scroll((child_offset, 0))
            .render(child_area, buffer);
        true
    }
}

impl TranscriptAreaRenderable<'_> {
    fn child_area(&self, area: Rect) -> Rect {
        Rect::new(
            area.x,
            area.y.saturating_add(self.top),
            area.width.saturating_sub(self.right).max(1),
            area.height.saturating_sub(self.top),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::TranscriptAreaRenderable;
    use crate::app::App;
    use crate::bottom_pane::bottom_pane_view::ChatComposerView;
    use crate::history_cell::HistoryCell;
    use crate::render::renderable::Renderable;
    use crate::runtime::RuntimeEvent;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::text::Line;
    use ratatui::text::Span;

    #[derive(Debug)]
    struct TestCell;

    impl HistoryCell for TestCell {
        fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
            vec![Line::from(Span::raw("123456789"))]
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn renders_active_process_exec_and_working_composer() {
        let mut app = App::new("render-test".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        app.handle_runtime_event(RuntimeEvent::ExecutionStarted {
            execution_id: "exec-1".to_owned(),
            capability: "process.exec".to_owned(),
            program: "node".to_owned(),
            args: vec!["--version".to_owned()],
            cwd: Some("/tmp".to_owned()),
            target: Some("local".to_owned()),
        });

        let area = Rect::new(0, 0, 100, 37);
        let mut buffer = Buffer::empty(area);
        super::render(area, &mut buffer, &mut app, &ChatComposerView);
        let screen = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(screen.contains("Running node --version"));
        assert!(screen.contains("Working ("));
        assert!(screen.contains("Ask Codex to do anything"));
    }

    #[test]
    fn reserves_codex_separator_and_right_inset() {
        let cell = TestCell;
        let renderable = TranscriptAreaRenderable {
            child: &cell,
            top: 1,
            right: 2,
        };

        assert_eq!(renderable.desired_height(10), 3);
    }
}
