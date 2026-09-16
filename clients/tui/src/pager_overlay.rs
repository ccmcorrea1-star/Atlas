//! Pager-style transcript overlay adapted from the Codex TUI.
//!
//! Unlike the main viewport, the transcript is rendered in a full-screen pager
//! with the same header, separator, scroll percentage, and key hints as Codex.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use std::cell::Cell;
use std::cell::RefCell;

use crate::app::App;
use crate::keymap::Action;
use crate::wrapping::wrap_line;

#[derive(Debug, Default)]
pub(crate) struct TranscriptOverlay {
    open: Cell<bool>,
    live_tail_cache: RefCell<Option<LiveTailCache>>,
    scroll_offset: Cell<usize>,
    last_max_scroll: Cell<usize>,
}

#[derive(Debug, Clone)]
struct LiveTailCache {
    width: u16,
    revision: u64,
    lines: Vec<Line<'static>>,
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
    }

    pub(crate) fn close(&self) {
        self.open.set(false);
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
                self.scroll_up(8);
                None
            }
            Some(Action::PageDown) => {
                self.scroll_down(8);
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

    pub(crate) fn render(&self, app: &App, area: Rect, buffer: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        Clear.render(area, buffer);

        let header = Rect::new(area.x, area.y, area.width, 1);
        Span::styled(
            "/ ".repeat(usize::from(area.width) / 2),
            Style::default().dim(),
        )
        .render(header, buffer);
        Span::styled("/ T R A N S C R I P T", Style::default().dim()).render(header, buffer);

        let content_height = area.height.saturating_sub(4);
        let content = Rect::new(area.x, area.y.saturating_add(1), area.width, content_height);
        let mut lines = Vec::new();
        for cell in app.cells() {
            if !lines.is_empty() && !cell.is_stream_continuation() {
                lines.push(Line::default());
            }
            lines.extend(wrap_lines(
                cell.transcript_lines(content.width.max(1)),
                content.width.max(1),
            ));
        }
        let active_lines = self.live_tail(app, content.width.max(1));
        if !active_lines.is_empty()
            && !lines.is_empty()
            && app
                .active_cells()
                .first()
                .is_none_or(|cell| !cell.is_stream_continuation())
        {
            lines.push(Line::default());
        }
        lines.extend(wrap_lines(active_lines, content.width.max(1)));

        let total_height = lines.len();
        let max_scroll = total_height.saturating_sub(usize::from(content.height));
        self.last_max_scroll.set(max_scroll);
        let scroll = self.scroll_offset.get().min(max_scroll);
        let visible = Paragraph::new(lines).scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0));
        visible.render(content, buffer);

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
        Span::styled(
            "─".repeat(usize::from(separator.width)),
            Style::default().dim(),
        )
        .render(separator, buffer);
        let percent = if max_scroll == 0 {
            100
        } else {
            ((scroll as f32 / max_scroll as f32) * 100.0).round() as u8
        };
        Span::styled(format!(" {percent}% "), Style::default().dim()).render(
            Rect::new(
                separator.right().saturating_sub(6),
                separator.y,
                6.min(separator.width),
                1,
            ),
            buffer,
        );
        let navigation = Rect::new(area.x, separator.y.saturating_add(1), area.width, 1);
        Line::from(Span::styled(
            " ↑/↓ to scroll   pgup/pgdn to page   home/end to jump",
            Style::default().dim(),
        ))
        .render(navigation, buffer);
        Line::from(Span::styled(
            " q close   esc to edit prev",
            Style::default().dim(),
        ))
        .render(
            Rect::new(area.x, navigation.y.saturating_add(1), area.width, 1),
            buffer,
        );
    }

    fn live_tail(&self, app: &App, width: u16) -> Vec<Line<'static>> {
        let revision = app.active_revision();
        if let Some(cache) = self.live_tail_cache.borrow().as_ref()
            && cache.width == width
            && cache.revision == revision
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
            lines: lines.clone(),
        });
        lines
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

        let rows = (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(rows[0].starts_with("/ T R A N S C R I P T"));
        assert!(rows[8].contains("to scroll"));
        assert!(rows[9].contains("q close"));
    }

    #[test]
    fn owns_open_state_and_returns_completion_when_closed() {
        let overlay = TranscriptOverlay::default();
        assert!(!overlay.is_open());

        overlay.open();
        assert!(overlay.is_open());
        assert_eq!(overlay.handle_action(Some(Action::PageDown)), None);
        assert_eq!(
            overlay.handle_action(Some(Action::CloseOverlay)),
            Some(TranscriptViewCompletion::Closed)
        );
        assert!(!overlay.is_open());
    }
}
