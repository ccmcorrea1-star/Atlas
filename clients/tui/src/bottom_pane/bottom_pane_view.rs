use std::time::Duration;
use std::time::Instant;

use crossterm::event::KeyEvent;
use crossterm::event::MouseEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::app::App;
use crate::bottom_pane::BottomPaneSurface;
use crate::bottom_pane::chat_composer;
use crate::bottom_pane::footer;
use crate::bottom_pane::paste_burst::PasteBurst;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ViewCompletion {
    Accepted,
    Cancelled,
}

/// A renderable and interactive view hosted by the canonical bottom pane.
///
/// Atlas keeps the composer state in `BottomPane` while the runtime and
/// transcript controller remains in `App`. The explicit `App` parameter is
/// therefore the narrow coordination seam; modal views can later own their
/// state and use the same lifecycle without introducing a second input path.
pub(crate) trait BottomPaneView {
    /// Render the active view for the current app state.
    fn render(&self, app: &App, area: Rect, buffer: &mut Buffer);

    /// Return the height required by this view.
    fn desired_height(&self, app: &App, width: u16) -> u16;

    /// Return the cursor position relative to the terminal area.
    fn cursor_pos(&self, app: &App, area: Rect) -> Option<(u16, u16)>;

    /// Handle an event after global overlays had a chance to consume it.
    fn handle_key_event(&self, app: &mut App, key: KeyEvent) -> Option<String>;

    /// Handle a terminal paste event. Returns whether the view consumed it.
    fn handle_paste(&self, app: &mut App, text: &str) -> bool;

    /// Handle mouse input routed to the bottom pane and its popups.
    fn handle_mouse_event(&self, app: &mut App, mouse: MouseEvent) -> bool;

    /// Return whether this view has completed and should be replaced.
    #[allow(dead_code)]
    fn is_complete(&self, _app: &App) -> bool {
        false
    }

    /// Return the completion reason when this view has finished.
    #[allow(dead_code)]
    fn completion(&self, _app: &App) -> Option<ViewCompletion> {
        None
    }

    /// Flush time-based input owned by this view before rendering.
    fn pre_draw_tick(&self, _app: &mut App, _now: Instant) -> bool {
        false
    }

    /// Report transient paste state so the event loop can schedule redraws.
    #[allow(dead_code)]
    fn is_in_paste_burst(&self, _app: &App) -> bool {
        false
    }

    /// Return the next redraw delay requested by this view.
    #[allow(dead_code)]
    fn next_frame_delay(&self, _app: &App) -> Option<Duration> {
        None
    }
}

/// The default bottom-pane view for the Atlas chat session.
pub(crate) struct ChatComposerView;

impl BottomPaneView for ChatComposerView {
    fn render(&self, app: &App, area: Rect, buffer: &mut Buffer) {
        chat_composer::render(app, area, buffer);
    }

    fn desired_height(&self, app: &App, width: u16) -> u16 {
        chat_composer::desired_height(app, width)
    }

    fn cursor_pos(&self, app: &App, area: Rect) -> Option<(u16, u16)> {
        chat_composer::cursor_position_for(app, area)
    }

    fn handle_key_event(&self, app: &mut App, key: KeyEvent) -> Option<String> {
        app.handle_key_event(key)
    }

    fn handle_paste(&self, app: &mut App, text: &str) -> bool {
        app.handle_paste(text);
        true
    }

    fn handle_mouse_event(&self, app: &mut App, mouse: MouseEvent) -> bool {
        app.handle_mouse_event(mouse);
        true
    }

    fn pre_draw_tick(&self, app: &mut App, now: Instant) -> bool {
        app.flush_paste_burst_if_due_at(now)
    }

    #[allow(dead_code)]
    fn is_in_paste_burst(&self, app: &App) -> bool {
        app.is_in_paste_burst()
    }

    #[allow(dead_code)]
    fn next_frame_delay(&self, app: &App) -> Option<Duration> {
        self.is_in_paste_burst(app)
            .then_some(PasteBurst::recommended_active_flush_delay())
    }
}

/// The view currently occupying the interactive bottom pane.
pub(crate) enum ActiveBottomPaneView {
    Composer(ChatComposerView),
    Shortcuts(ShortcutsView),
}

impl ActiveBottomPaneView {
    pub(crate) fn new() -> Self {
        Self::Composer(ChatComposerView)
    }

    /// Keep the concrete view aligned with the app-level overlay state.
    pub(crate) fn sync(&mut self, app: &App) -> Option<ViewCompletion> {
        let completion = self.completion(app);
        if app.bottom_pane().surface() == BottomPaneSurface::Shortcuts
            && matches!(self, Self::Composer(_))
        {
            *self = Self::Shortcuts(ShortcutsView);
        } else if app.bottom_pane().surface() == BottomPaneSurface::Composer
            && matches!(self, Self::Shortcuts(_))
        {
            *self = Self::Composer(ChatComposerView);
        }
        completion
    }
}

impl BottomPaneView for ActiveBottomPaneView {
    fn render(&self, app: &App, area: Rect, buffer: &mut Buffer) {
        match self {
            Self::Composer(view) => view.render(app, area, buffer),
            Self::Shortcuts(view) => view.render(app, area, buffer),
        }
    }

    fn desired_height(&self, app: &App, width: u16) -> u16 {
        match self {
            Self::Composer(view) => view.desired_height(app, width),
            Self::Shortcuts(view) => view.desired_height(app, width),
        }
    }

    fn cursor_pos(&self, app: &App, area: Rect) -> Option<(u16, u16)> {
        match self {
            Self::Composer(view) => view.cursor_pos(app, area),
            Self::Shortcuts(view) => view.cursor_pos(app, area),
        }
    }

    fn handle_key_event(&self, app: &mut App, key: KeyEvent) -> Option<String> {
        match self {
            Self::Composer(view) => view.handle_key_event(app, key),
            Self::Shortcuts(view) => view.handle_key_event(app, key),
        }
    }

    fn handle_paste(&self, app: &mut App, text: &str) -> bool {
        match self {
            Self::Composer(view) => view.handle_paste(app, text),
            Self::Shortcuts(view) => view.handle_paste(app, text),
        }
    }

    fn handle_mouse_event(&self, app: &mut App, mouse: MouseEvent) -> bool {
        match self {
            Self::Composer(view) => view.handle_mouse_event(app, mouse),
            Self::Shortcuts(view) => view.handle_mouse_event(app, mouse),
        }
    }

    fn is_complete(&self, app: &App) -> bool {
        match self {
            Self::Composer(view) => view.is_complete(app),
            Self::Shortcuts(view) => view.is_complete(app),
        }
    }

    fn completion(&self, app: &App) -> Option<ViewCompletion> {
        match self {
            Self::Composer(view) => view.completion(app),
            Self::Shortcuts(view) => view.completion(app),
        }
    }

    fn pre_draw_tick(&self, app: &mut App, now: Instant) -> bool {
        match self {
            Self::Composer(view) => view.pre_draw_tick(app, now),
            Self::Shortcuts(view) => view.pre_draw_tick(app, now),
        }
    }

    fn is_in_paste_burst(&self, app: &App) -> bool {
        match self {
            Self::Composer(view) => view.is_in_paste_burst(app),
            Self::Shortcuts(view) => view.is_in_paste_burst(app),
        }
    }

    fn next_frame_delay(&self, app: &App) -> Option<Duration> {
        match self {
            Self::Composer(view) => view.next_frame_delay(app),
            Self::Shortcuts(view) => view.next_frame_delay(app),
        }
    }
}

/// Modal view for the shortcuts surface.
pub(crate) struct ShortcutsView;

impl BottomPaneView for ShortcutsView {
    fn render(&self, app: &App, area: Rect, buffer: &mut Buffer) {
        footer::render(app, area, buffer);
    }

    fn desired_height(&self, app: &App, width: u16) -> u16 {
        footer::desired_height(app, width)
    }

    fn cursor_pos(&self, _app: &App, _area: Rect) -> Option<(u16, u16)> {
        None
    }

    fn handle_key_event(&self, _app: &mut App, _key: KeyEvent) -> Option<String> {
        None
    }

    fn handle_paste(&self, _app: &mut App, _text: &str) -> bool {
        true
    }

    fn handle_mouse_event(&self, _app: &mut App, _mouse: MouseEvent) -> bool {
        true
    }

    fn is_complete(&self, app: &App) -> bool {
        app.bottom_pane().surface() == BottomPaneSurface::Composer
    }

    fn completion(&self, app: &App) -> Option<ViewCompletion> {
        self.is_complete(app).then_some(ViewCompletion::Cancelled)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use std::time::Instant;

    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    use super::ActiveBottomPaneView;
    use super::BottomPaneView;
    use super::ChatComposerView;
    use super::ViewCompletion;
    use crate::app::App;

    #[test]
    fn composer_view_routes_paste_to_the_owned_composer_state() {
        let view = ChatComposerView;
        let mut app = App::new("test".to_owned());

        assert!(view.handle_paste(&mut app, "draft from paste"));
        assert_eq!(app.input(), "draft from paste");
    }

    #[test]
    fn composer_view_routes_submission_keys_and_returns_the_input() {
        let view = ChatComposerView;
        let mut app = App::new("test".to_owned());
        app.insert_text("submit me");

        let submitted =
            view.handle_key_event(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(submitted.as_deref(), Some("submit me"));
        assert!(app.input().is_empty());
    }

    #[test]
    fn composer_view_exposes_layout_and_cursor_through_the_view() {
        let view = ChatComposerView;
        let mut app = App::new("test".to_owned());
        app.insert_text("render me");

        assert!(view.desired_height(&app, 80) >= 1);
        assert!(
            view.cursor_pos(&app, ratatui::layout::Rect::new(0, 0, 80, 4))
                .is_some()
        );
    }

    #[test]
    fn composer_view_flushes_paste_burst_before_rendering() {
        let view = ChatComposerView;
        let mut app = App::new("test".to_owned());
        let plain = crossterm::event::KeyModifiers::NONE;
        app.handle_key_event(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            plain,
        ));
        app.handle_key_event(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('b'),
            plain,
        ));

        std::thread::sleep(Duration::from_millis(12));
        assert!(view.pre_draw_tick(&mut app, Instant::now()));
        assert!(!view.is_in_paste_burst(&app));
        assert!(view.next_frame_delay(&app).is_none());
        assert_eq!(app.input(), "ab");
    }

    #[test]
    fn active_view_switches_to_and_from_the_shortcuts_modal() {
        let mut view = ActiveBottomPaneView::new();
        let mut app = App::new("test".to_owned());

        assert!(matches!(view, ActiveBottomPaneView::Composer(_)));

        app.open_shortcuts();
        view.sync(&app);
        assert!(matches!(view, ActiveBottomPaneView::Shortcuts(_)));
        assert!(!view.is_complete(&app));
        assert_eq!(view.desired_height(&app, 80), 11);

        app.close_shortcuts();
        assert_eq!(view.sync(&app), Some(ViewCompletion::Cancelled));
        assert!(matches!(view, ActiveBottomPaneView::Composer(_)));
    }
}
