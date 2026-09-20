mod base;
mod exec;
mod markdown_render_cache;
mod messages;
mod session_header;

use std::any::Any;

use crate::render::renderable::Renderable;
use ratatui::text::Line;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;

pub(crate) use base::plain_lines;
pub(crate) use exec::ErrorCell;
pub(crate) use exec::ToolCell;
pub(crate) use exec::ToolGroupCell;
pub(crate) use messages::AgentMarkdownCell;
pub(crate) use messages::AgentMessageCell;
pub(crate) use messages::CancelledCell;
pub(crate) use messages::ThoughtCell;
pub(crate) use messages::UserHistoryCell;
pub(crate) use session_header::SessionHeaderCell;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HistoryRenderMode {
    Rich,
    Raw,
}

pub(crate) trait HistoryCell: std::fmt::Debug + Send + Sync + Any {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>>;

    fn background_style(&self) -> Option<ratatui::style::Style> {
        None
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        plain_lines(self.display_lines(u16::MAX))
    }

    fn display_lines_for_mode(&self, width: u16, mode: HistoryRenderMode) -> Vec<Line<'static>> {
        match mode {
            HistoryRenderMode::Rich => self.display_lines(width),
            HistoryRenderMode::Raw => self.raw_lines(),
        }
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.display_lines(width)
    }

    fn desired_height(&self, width: u16) -> u16 {
        Paragraph::new(Text::from(
            self.display_lines_for_mode(width, HistoryRenderMode::Rich),
        ))
        .wrap(Wrap { trim: false })
        .line_count(width)
        .try_into()
        .unwrap_or(0)
    }

    #[allow(dead_code)]
    fn desired_transcript_height(&self, width: u16) -> u16 {
        Paragraph::new(Text::from(self.transcript_lines(width)))
            .wrap(Wrap { trim: false })
            .line_count(width)
            .try_into()
            .unwrap_or(0)
    }

    #[allow(dead_code)]
    fn has_stable_transcript_height(&self) -> bool {
        true
    }

    fn is_stream_continuation(&self) -> bool {
        false
    }

    #[allow(dead_code)]
    fn transcript_animation_tick(&self) -> Option<u64> {
        None
    }

    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl Renderable for Box<dyn HistoryCell> {
    fn render(&self, area: ratatui::layout::Rect, buffer: &mut ratatui::buffer::Buffer) {
        ratatui::widgets::Paragraph::new(self.display_lines(area.width)).render(area, buffer);
    }

    fn desired_height(&self, width: u16) -> u16 {
        HistoryCell::desired_height(self.as_ref(), width)
    }
}

#[allow(dead_code)]
pub(crate) fn new_user_prompt(message: String) -> UserHistoryCell {
    UserHistoryCell::new(message)
}
