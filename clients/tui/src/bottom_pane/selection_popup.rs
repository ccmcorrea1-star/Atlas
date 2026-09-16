use std::ops::Range;

#[derive(Debug, Default)]
pub(crate) struct SelectionPopupState {
    selected: usize,
    scroll: usize,
}

impl SelectionPopupState {
    pub(crate) fn selected(&self) -> usize {
        self.selected
    }

    pub(crate) fn reset(&mut self) {
        self.selected = 0;
        self.scroll = 0;
    }

    pub(crate) fn move_by(&mut self, item_count: usize, down: bool, visible_rows: usize) {
        if item_count == 0 {
            self.selected = 0;
            self.scroll = 0;
            return;
        }
        self.selected = if down {
            (self.selected + 1) % item_count
        } else if self.selected == 0 {
            item_count - 1
        } else {
            self.selected - 1
        };
        self.ensure_visible(item_count, visible_rows);
    }

    pub(crate) fn visible_range(&self, item_count: usize, visible_rows: usize) -> Range<usize> {
        let rows = visible_rows.max(1);
        let start = self.scroll.min(item_count);
        let end = (start + rows).min(item_count);
        start..end
    }

    pub(crate) fn selected_visible_row(
        &self,
        item_count: usize,
        visible_rows: usize,
    ) -> Option<usize> {
        let range = self.visible_range(item_count, visible_rows);
        range
            .contains(&self.selected)
            .then_some(self.selected - range.start)
    }

    fn ensure_visible(&mut self, item_count: usize, visible_rows: usize) {
        let rows = visible_rows.max(1);
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + rows {
            self.scroll = self.selected + 1 - rows;
        }
        self.scroll = self.scroll.min(item_count.saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::SelectionPopupState;

    #[test]
    fn wraps_selection_and_keeps_it_inside_the_visible_window() {
        let mut state = SelectionPopupState::default();
        state.move_by(10, false, 3);
        assert_eq!(state.selected(), 9);
        assert_eq!(state.visible_range(10, 3), 7..10);
        assert_eq!(state.selected_visible_row(10, 3), Some(2));
        state.move_by(10, true, 3);
        assert_eq!(state.selected(), 0);
        assert_eq!(state.visible_range(10, 3), 0..3);
    }

    #[test]
    fn clamps_state_when_the_popup_query_changes() {
        let mut state = SelectionPopupState::default();
        state.move_by(4, true, 8);
        state.move_by(4, true, 8);
        state.move_by(4, true, 8);
        state.reset();
        assert_eq!(state.selected(), 0);
        assert_eq!(state.visible_range(1, 8), 0..1);
    }
}
