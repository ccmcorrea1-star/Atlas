#![allow(dead_code)]

pub(crate) mod highlight;
pub(crate) mod line_utils;
pub mod renderable;

use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug, Default)]
pub struct Insets {
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
    pub left: u16,
}

impl Insets {
    pub const fn new(top: u16, right: u16, bottom: u16, left: u16) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }
}

pub trait RectExt {
    fn inset(self, insets: Insets) -> Rect;
}

impl RectExt for Rect {
    fn inset(self, insets: Insets) -> Rect {
        let horizontal = insets.left.saturating_add(insets.right);
        let vertical = insets.top.saturating_add(insets.bottom);
        Rect::new(
            self.x.saturating_add(insets.left),
            self.y.saturating_add(insets.top),
            self.width.saturating_sub(horizontal),
            self.height.saturating_sub(vertical),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::renderable::{FlexRenderable, Renderable};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::text::Line;

    #[test]
    fn flex_renderable_allocates_fixed_and_flexible_children() {
        let mut layout = FlexRenderable::new();
        layout.push(0, Box::new(Line::from("fixed")) as Box<dyn Renderable>);
        layout.push(1, Box::new(Line::from("flex")) as Box<dyn Renderable>);

        assert_eq!(layout.desired_height(20), 2);

        let area = Rect::new(0, 0, 20, 4);
        let mut buffer = Buffer::empty(area);
        layout.render(area, &mut buffer);
        assert_eq!(buffer[(0, 0)].symbol(), "f");
        assert_eq!(buffer[(0, 1)].symbol(), "f");
    }
}
