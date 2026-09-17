pub(crate) mod bottom_pane_view;
pub(crate) mod chat_composer;
pub(crate) mod footer;
pub(crate) mod paste_burst;
pub(crate) mod prompt_args;
pub(crate) mod selection_popup;
pub(crate) mod slash_commands;
pub(crate) mod textarea;

pub(crate) use bottom_pane_view::ActiveBottomPaneView;
pub(crate) use bottom_pane_view::BottomPaneView;
pub(crate) use chat_composer::ChatComposer;

/// Mantem o estado do composer no painel interativo inferior.
///
/// A coordenacao do Runtime e do transcript permanece em `App`; este container
/// e o dono estrutural da edicao do rascunho e da interacao local.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BottomPaneSurface {
    Composer,
    Shortcuts,
}

#[derive(Debug)]
pub(crate) struct BottomPane {
    pub(crate) composer: ChatComposer,
    surface: BottomPaneSurface,
}

impl BottomPane {
    pub(crate) fn new() -> Self {
        Self {
            composer: ChatComposer::new(),
            surface: BottomPaneSurface::Composer,
        }
    }

    pub(crate) fn surface(&self) -> BottomPaneSurface {
        self.surface
    }

    pub(crate) fn shortcuts_open(&self) -> bool {
        self.surface == BottomPaneSurface::Shortcuts
    }

    pub(crate) fn open_shortcuts(&mut self) {
        self.surface = BottomPaneSurface::Shortcuts;
    }

    pub(crate) fn close_shortcuts(&mut self) {
        self.surface = BottomPaneSurface::Composer;
    }
}

#[cfg(test)]
mod tests {
    use super::BottomPane;
    use super::BottomPaneSurface;

    #[test]
    fn starts_with_isolated_composer_state() {
        let pane = BottomPane::new();

        assert!(pane.composer.textarea.is_empty());
        assert!(pane.composer.history_entries.is_empty());
        assert!(!pane.composer.history_search_open);
        assert_eq!(pane.surface(), BottomPaneSurface::Composer);
    }
}
