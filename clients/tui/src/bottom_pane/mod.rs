pub(crate) mod bottom_pane_view;
pub(crate) mod chat_composer;
pub(crate) mod footer;
pub(crate) mod paste_burst;
pub(crate) mod prompt_args;
pub(crate) mod slash_commands;
pub(crate) mod textarea;

pub(crate) use bottom_pane_view::ActiveBottomPaneView;
pub(crate) use bottom_pane_view::BottomPaneView;
pub(crate) use chat_composer::ChatComposer;

/// Owns the composer state for the lower interactive pane.
///
/// Runtime/transcript coordination remains in `App`; this container is the
/// structural owner of draft editing and its local interaction state.
#[derive(Debug)]
pub(crate) struct BottomPane {
    pub(crate) composer: ChatComposer,
}

impl BottomPane {
    pub(crate) fn new() -> Self {
        Self {
            composer: ChatComposer::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BottomPane;

    #[test]
    fn starts_with_isolated_composer_state() {
        let pane = BottomPane::new();

        assert!(pane.composer.textarea.is_empty());
        assert!(pane.composer.history_entries.is_empty());
        assert!(!pane.composer.history_search_open);
    }
}
