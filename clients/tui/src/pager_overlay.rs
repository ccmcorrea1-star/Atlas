//! Overlay de transcript no estilo pager, adaptado da TUI do Codex.
//!
//! Diferente do viewport principal, o transcript e renderizado em pager de tela
//! cheia com cabecalho, separador, percentual e dicas de teclas do Codex.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;
use std::cell::Cell;
use std::cell::RefCell;

use crate::app::App;
use crate::keymap::Action;
use crate::render::renderable::Renderable;
use crate::ui_consts::action_style;
use crate::ui_consts::secondary_style;
use crate::ui_consts::surface_style;
use crate::wrapping::wrap_line;

#[derive(Debug, Default)]
pub(crate) struct TranscriptOverlay {
    open: Cell<bool>,
    live_tail_cache: RefCell<Option<LiveTailCache>>,
    committed_cache: RefCell<Option<CommittedTranscriptCache>>,
    scroll_offset: Cell<usize>,
    last_max_scroll: Cell<usize>,
    last_content_height: Cell<usize>,
}

#[derive(Debug, Clone)]
struct LiveTailCache {
    width: u16,
    revision: u64,
    is_stream_continuation: bool,
    animation_tick: Option<u64>,
    lines: Vec<Line<'static>>,
}

#[derive(Debug, Clone)]
struct CommittedTranscriptCache {
    width: u16,
    cell_count: usize,
    revision: u64,
    cell_heights: Vec<usize>,
    lines: Vec<Line<'static>>,
    renderables: Vec<LinesRenderable>,
}

#[derive(Debug, Clone)]
struct LinesRenderable {
    lines: Vec<Line<'static>>,
}

impl Renderable for LinesRenderable {
    fn render(&self, area: Rect, buffer: &mut Buffer) {
        self.render_scrolled(area, buffer, 0);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        self.lines.len().try_into().unwrap_or(u16::MAX)
    }

    fn render_scrolled(&self, area: Rect, buffer: &mut Buffer, scroll_offset: u16) -> bool {
        for (row, line) in self
            .lines
            .iter()
            .skip(usize::from(scroll_offset))
            .take(usize::from(area.height))
            .enumerate()
        {
            line.clone().render(
                Rect::new(
                    area.x,
                    area.y
                        .saturating_add(u16::try_from(row).unwrap_or(u16::MAX)),
                    area.width,
                    1,
                ),
                buffer,
            );
        }
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TranscriptViewCompletion {
    Closed,
}

impl TranscriptOverlay {
    pub(crate) fn is_open(&self) -> bool {
        self.open.get()
    }

    pub(crate) fn open(&self) {
        self.open.set(true);
        self.scroll_offset.set(usize::MAX);
    }

    pub(crate) fn close(&self) {
        self.open.set(false);
    }

    pub(crate) fn on_resize(&self) {
        self.live_tail_cache.borrow_mut().take();
        self.committed_cache.borrow_mut().take();
        self.last_content_height.set(0);
    }

    pub(crate) fn handle_action(&self, action: Option<Action>) -> Option<TranscriptViewCompletion> {
        match action {
            Some(Action::Cancel) | Some(Action::CloseOverlay) => {
                self.close();
                Some(TranscriptViewCompletion::Closed)
            }
            Some(Action::ScrollUp) => {
                self.scroll_up(1);
                None
            }
            Some(Action::ScrollDown) => {
                self.scroll_down(1);
                None
            }
            Some(Action::PageUp) => {
                self.scroll_up(self.page_height());
                None
            }
            Some(Action::PageDown) => {
                self.scroll_down(self.page_height());
                None
            }
            Some(Action::HalfPageUp) => {
                self.scroll_up((self.page_height() / 2).max(1));
                None
            }
            Some(Action::HalfPageDown) => {
                self.scroll_down((self.page_height() / 2).max(1));
                None
            }
            Some(Action::JumpTop) => {
                self.scroll_to_top();
                None
            }
            Some(Action::JumpBottom) => {
                self.scroll_to_bottom();
                None
            }
            _ => None,
        }
    }

    pub(crate) fn scroll_up(&self, amount: usize) {
        let current = self.scroll_offset.get();
        let current = if current == usize::MAX {
            self.last_max_scroll.get()
        } else {
            current
        };
        self.scroll_offset
            .set(current.saturating_sub(amount.max(1)));
    }

    pub(crate) fn scroll_down(&self, amount: usize) {
        let current = self.scroll_offset.get();
        let current = if current == usize::MAX {
            self.last_max_scroll.get()
        } else {
            current
        };
        let next = current.saturating_add(amount.max(1));
        self.scroll_offset
            .set(if next >= self.last_max_scroll.get() {
                usize::MAX
            } else {
                next
            });
    }

    pub(crate) fn scroll_to_top(&self) {
        self.scroll_offset.set(0);
    }

    pub(crate) fn scroll_to_bottom(&self) {
        self.scroll_offset.set(usize::MAX);
    }

    fn page_height(&self) -> usize {
        self.last_content_height.get().max(1)
    }

    pub(crate) fn render(&self, app: &App, area: Rect, buffer: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        Clear.render(area, buffer);
        buffer.set_style(area, surface_style());

        let header = Rect::new(area.x, area.y, area.width, 1);
        Span::styled("/ ".repeat(usize::from(area.width) / 2), secondary_style())
            .render(header, buffer);
        Span::styled("/ T R A N S C R I P T", action_style()).render(header, buffer);

        let content_height = area.height.saturating_sub(5);
        let content = Rect::new(area.x, area.y.saturating_add(1), area.width, content_height);
        let width = content.width.max(1);
        let cell_count = app.cells().len();
        let revision = app.history_revision();
        let cached = self
            .committed_cache
            .borrow()
            .as_ref()
            .filter(|cache| {
                cache.width == width && cache.cell_count == cell_count && cache.revision == revision
            })
            .map(|cache| {
                debug_assert_eq!(cache.cell_heights.iter().sum::<usize>(), cache.lines.len());
                (cache.lines.clone(), cache.renderables.clone())
            });
        let (mut lines, mut renderables) = cached.unwrap_or_else(|| {
            let mut lines = Vec::new();
            let mut renderables = Vec::new();
            let mut cell_heights = Vec::with_capacity(cell_count);
            for cell in app.cells() {
                let start = lines.len();
                if !lines.is_empty() && !cell.is_stream_continuation() {
                    let blank = LinesRenderable {
                        lines: vec![Line::default()],
                    };
                    lines.push(Line::default());
                    renderables.push(blank);
                }
                let wrapped = wrap_lines(cell.transcript_lines(width), width);
                lines.extend(wrapped.clone());
                renderables.push(LinesRenderable { lines: wrapped });
                cell_heights.push(lines.len().saturating_sub(start));
            }
            *self.committed_cache.borrow_mut() = Some(CommittedTranscriptCache {
                width,
                cell_count,
                revision,
                cell_heights,
                lines: lines.clone(),
                renderables: renderables.clone(),
            });
            (lines, renderables)
        });
        let active_lines = self.live_tail(app, width);
        if !active_lines.is_empty()
            && !lines.is_empty()
            && app
                .active_cells()
                .first()
                .is_none_or(|cell| !cell.is_stream_continuation())
        {
            lines.push(Line::default());
            renderables.push(LinesRenderable {
                lines: vec![Line::default()],
            });
        }
        let wrapped_active = wrap_lines(active_lines, content.width.max(1));
        lines.extend(wrapped_active.clone());
        if !wrapped_active.is_empty() {
            renderables.push(LinesRenderable {
                lines: wrapped_active,
            });
        }

        let total_height = lines.len();
        self.last_content_height.set(usize::from(content.height));
        let max_scroll = total_height.saturating_sub(usize::from(content.height));
        self.last_max_scroll.set(max_scroll);
        let scroll = self.scroll_offset.get().min(max_scroll);
        render_visible_renderables(&renderables, content, scroll, buffer);

        let drawn_rows = total_height
            .saturating_sub(scroll)
            .min(usize::from(content.height));
        for row in usize::from(content.y) + drawn_rows..usize::from(content.bottom()) {
            if let Ok(y) = u16::try_from(row) {
                buffer[(content.x, y)] = ratatui::buffer::Cell::from('~');
                for x in content.x.saturating_add(1)..content.right() {
                    buffer[(x, y)] = ratatui::buffer::Cell::from(' ');
                }
            }
        }

        let separator = Rect::new(area.x, content.bottom(), area.width, 1);
        Span::styled("─".repeat(usize::from(separator.width)), secondary_style())
            .render(separator, buffer);
        let percent = if max_scroll == 0 {
            100
        } else {
            ((scroll as f32 / max_scroll as f32) * 100.0).round() as u8
        };
        Span::styled(format!(" {percent}% "), secondary_style()).render(
            Rect::new(
                separator.right().saturating_sub(6),
                separator.y,
                6.min(separator.width),
                1,
            ),
            buffer,
        );
        let navigation = Rect::new(area.x, separator.y.saturating_add(1), area.width, 1);
        let navigation_hint = format!(
            " {}/{} to scroll   {}/{} to page   {}/{} to jump",
            crate::keymap::hint(Action::ScrollUp),
            crate::keymap::hint(Action::ScrollDown),
            crate::keymap::hint(Action::PageUp),
            crate::keymap::hint(Action::PageDown),
            crate::keymap::hint(Action::JumpTop),
            crate::keymap::hint(Action::JumpBottom),
        );
        Line::from(Span::styled(navigation_hint, secondary_style())).render(navigation, buffer);
        let close_hint = format!(
            " {} close   {} to edit prev",
            crate::keymap::hint(Action::CloseOverlay),
            crate::keymap::hint(Action::Cancel),
        );
        Line::from(Span::styled(close_hint, secondary_style())).render(
            Rect::new(area.x, navigation.y.saturating_add(1), area.width, 1),
            buffer,
        );
    }

    fn live_tail(&self, app: &App, width: u16) -> Vec<Line<'static>> {
        let revision = app.active_revision();
        let is_stream_continuation = app
            .active_cells()
            .first()
            .is_some_and(|cell| cell.is_stream_continuation());
        let animation_tick = app
            .active_cells()
            .iter()
            .find_map(|cell| cell.transcript_animation_tick());
        if let Some(cache) = self.live_tail_cache.borrow().as_ref()
            && cache.width == width
            && cache.revision == revision
            && cache.is_stream_continuation == is_stream_continuation
            && cache.animation_tick == animation_tick
        {
            return cache.lines.clone();
        }
        let mut lines = Vec::new();
        for cell in app.active_cells() {
            if !lines.is_empty() && !cell.is_stream_continuation() {
                lines.push(Line::default());
            }
            lines.extend(cell.transcript_lines(width));
        }
        *self.live_tail_cache.borrow_mut() = Some(LiveTailCache {
            width,
            revision,
            is_stream_continuation,
            animation_tick,
            lines: lines.clone(),
        });
        lines
    }
}

fn render_visible_renderables(
    renderables: &[LinesRenderable],
    area: Rect,
    scroll: usize,
    buffer: &mut Buffer,
) {
    let mut offset = 0usize;
    let viewport_end = scroll.saturating_add(usize::from(area.height));
    for renderable in renderables {
        let height = usize::from(renderable.desired_height(area.width));
        let renderable_end = offset.saturating_add(height);
        let visible_start = offset.max(scroll);
        let visible_end = renderable_end.min(viewport_end);
        if visible_start < visible_end {
            let y = area.y.saturating_add(
                u16::try_from(visible_start.saturating_sub(scroll)).unwrap_or(u16::MAX),
            );
            let visible_area = Rect::new(
                area.x,
                y,
                area.width,
                u16::try_from(visible_end - visible_start).unwrap_or(u16::MAX),
            );
            let local_scroll = u16::try_from(visible_start - offset).unwrap_or(u16::MAX);
            renderable.render_scrolled(visible_area, buffer, local_scroll);
        }
        offset = renderable_end;
        if offset >= viewport_end {
            break;
        }
    }
}

fn wrap_lines(lines: Vec<Line<'static>>, width: u16) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .flat_map(|line| wrap_line(line, usize::from(width.max(1))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::runtime::RuntimeEvent;

    #[test]
    fn transcript_matches_codex_full_screen_shape() {
        let mut app = App::new("pager".to_owned());
        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message".to_owned(),
            content: "answer".to_owned(),
        });
        let area = Rect::new(0, 0, 40, 10);
        let mut buffer = Buffer::empty(area);

        let overlay = TranscriptOverlay::default();
        overlay.render(&app, area, &mut buffer);

        let cache = overlay.committed_cache.borrow();
        let cache = cache.as_ref().expect("committed transcript cache");
        assert_eq!(cache.cell_count, 2);
        assert_eq!(cache.cell_heights.iter().sum::<usize>(), cache.lines.len());

        let rows = (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rows[0].starts_with("/ T R A N S C R I P T"));
        assert!(rows[7].contains("to scroll"));
        assert!(rows[8].contains("q close"));
        insta::assert_snapshot!(rows.join("\n"));
    }

    #[test]
    fn transcript_wraps_long_lines_in_the_pager_viewport() {
        let mut app = App::new("pager-wrap".to_owned());
        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message".to_owned(),
            content: "This is a deliberately long transcript line that must wrap inside the pager."
                .to_owned(),
        });
        let area = Rect::new(0, 0, 24, 10);
        let mut buffer = Buffer::empty(area);
        let overlay = TranscriptOverlay::default();
        overlay.render(&app, area, &mut buffer);

        let rows = (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        insta::assert_snapshot!(rows.join("\n"));
    }

    #[test]
    fn manual_scroll_position_is_preserved_when_transcript_grows() {
        let mut app = App::new("pager-growth".to_owned());
        let content = (0..20)
            .map(|index| format!("line-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "first".to_owned(),
            content,
        });
        let area = Rect::new(0, 0, 32, 10);
        let mut buffer = Buffer::empty(area);
        let overlay = TranscriptOverlay::default();
        overlay.render(&app, area, &mut buffer);
        overlay.scroll_to_top();

        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "second".to_owned(),
            content: "new tail".to_owned(),
        });
        overlay.render(&app, area, &mut buffer);

        assert_eq!(overlay.scroll_offset.get(), 0);
        let rows = (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rows.iter().any(|row| row.contains("█████")));
        assert!(!rows.iter().any(|row| row.contains("new tail")));
    }

    #[test]
    fn half_page_actions_use_the_rendered_viewport_height() {
        let overlay = TranscriptOverlay::default();
        overlay.last_content_height.set(10);
        overlay.last_max_scroll.set(100);
        overlay.scroll_offset.set(50);

        overlay.handle_action(Some(Action::HalfPageUp));
        assert_eq!(overlay.scroll_offset.get(), 45);
        overlay.handle_action(Some(Action::HalfPageDown));
        assert_eq!(overlay.scroll_offset.get(), 50);
    }

    #[test]
    fn owns_open_state_and_returns_completion_when_closed() {
        let overlay = TranscriptOverlay::default();
        assert!(!overlay.is_open());

        overlay.open();
        assert!(overlay.is_open());
        assert_eq!(overlay.scroll_offset.get(), usize::MAX);
        assert_eq!(overlay.handle_action(Some(Action::PageDown)), None);
        assert_eq!(
            overlay.handle_action(Some(Action::CloseOverlay)),
            Some(TranscriptViewCompletion::Closed)
        );
        assert!(!overlay.is_open());
    }
}
