//! Cache de uma única largura para células Markdown finalizadas.

use std::sync::Mutex;
use std::sync::PoisonError;

use ratatui::text::Line;

#[derive(Debug, Default)]
pub(crate) struct MarkdownRenderCache {
    cached: Mutex<Option<(u16, Vec<Line<'static>>)>>,
}

impl MarkdownRenderCache {
    pub(crate) fn clear(&self) {
        self.cached
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }

    pub(crate) fn render(
        &self,
        width: u16,
        render: impl FnOnce() -> Vec<Line<'static>>,
    ) -> Vec<Line<'static>> {
        let mut cached = self.cached.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((cached_width, lines)) = cached.as_ref()
            && *cached_width == width
        {
            return lines.clone();
        }
        let lines = render();
        *cached = Some((width, lines.clone()));
        lines
    }
}
