//! Ciclo de vida dos blocos de reasoning expostos pelo Runtime.

use super::*;

impl ChatWidget {
    pub(super) fn handle_reasoning_event(&mut self, event: RuntimeEvent) {
        match event {
            RuntimeEvent::ReasoningStart { reasoning_id } => {
                if self.has_thought(&reasoning_id) {
                    return;
                }
                self.finalize_reasoning();
                self.active_cells
                    .push(Box::new(ThoughtCell::new_with_id(reasoning_id, "")));
                self.status = Status::Working;
                self.bump_active_revision();
                self.history_changed();
            }
            RuntimeEvent::ReasoningDelta {
                reasoning_id,
                delta,
            } => {
                if self.active_thought_index(&reasoning_id).is_none()
                    && !self.has_thought(&reasoning_id)
                {
                    self.handle_reasoning_event(RuntimeEvent::ReasoningStart {
                        reasoning_id: reasoning_id.clone(),
                    });
                }
                if let Some(thought) = self.find_active_thought_mut(&reasoning_id) {
                    thought.append(&bounded_text(&delta));
                    self.status = Status::Working;
                    self.bump_active_revision();
                    self.history_changed();
                }
            }
            RuntimeEvent::ReasoningEnd { reasoning_id } => {
                if let Some(index) = self.active_thought_index(&reasoning_id) {
                    let cell = self.active_cells.remove(index);
                    if let Some(cell) = finish_thought(cell) {
                        self.cells.push(cell);
                    }
                    self.status = Status::Working;
                    self.bump_active_revision();
                    self.history_changed();
                }
            }
            _ => {}
        }
    }

    pub(super) fn finalize_reasoning(&mut self) {
        let positions = self
            .active_cells
            .iter()
            .enumerate()
            .filter_map(|(index, cell)| cell.as_any().is::<ThoughtCell>().then_some(index))
            .collect::<Vec<_>>();
        if positions.is_empty() {
            return;
        }
        for index in positions.into_iter().rev() {
            let cell = self.active_cells.remove(index);
            if let Some(cell) = finish_thought(cell) {
                self.cells.push(cell);
            }
        }
        self.bump_active_revision();
        self.history_changed();
    }

    fn active_thought_index(&self, reasoning_id: &str) -> Option<usize> {
        self.active_cells.iter().rposition(|cell| {
            cell.as_any()
                .downcast_ref::<ThoughtCell>()
                .is_some_and(|thought| thought.reasoning_id() == reasoning_id)
        })
    }

    fn find_active_thought_mut(&mut self, reasoning_id: &str) -> Option<&mut ThoughtCell> {
        self.active_cells.iter_mut().rev().find_map(|cell| {
            cell.as_any_mut()
                .downcast_mut::<ThoughtCell>()
                .filter(|thought| thought.reasoning_id() == reasoning_id)
        })
    }

    fn has_thought(&self, reasoning_id: &str) -> bool {
        self.cells
            .iter()
            .chain(self.active_cells.iter())
            .filter_map(|cell| cell.as_any().downcast_ref::<ThoughtCell>())
            .any(|thought| thought.reasoning_id() == reasoning_id)
    }
}

fn finish_thought(mut cell: Box<dyn HistoryCell>) -> Option<Box<dyn HistoryCell>> {
    let keep = {
        let thought = cell.as_any_mut().downcast_mut::<ThoughtCell>()?;
        thought.finish();
        thought.has_visible_content()
    };
    keep.then_some(cell)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell_text(cell: &dyn HistoryCell) -> String {
        cell.display_lines(80)
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn thought_texts(widget: &ChatWidget, active: bool) -> Vec<String> {
        let cells = if active {
            widget.active_cells()
        } else {
            widget.cells()
        };
        cells
            .iter()
            .filter(|cell| cell.as_any().is::<ThoughtCell>())
            .map(|cell| cell_text(cell.as_ref()))
            .collect()
    }

    fn reasoning_widget() -> ChatWidget {
        let mut widget = ChatWidget::new();
        widget.handle_runtime_event(RuntimeEvent::TurnStarted);
        widget.handle_runtime_event(RuntimeEvent::ReasoningStart {
            reasoning_id: "reasoning-1".to_owned(),
        });
        widget
    }

    fn send_reasoning_deltas(widget: &mut ChatWidget) {
        for delta in [
            "**Completing",
            " todo updates**\n\nCheck every pending item before the answer.",
        ] {
            widget.handle_runtime_event(RuntimeEvent::ReasoningDelta {
                reasoning_id: "reasoning-1".to_owned(),
                delta: delta.to_owned(),
            });
        }
    }

    #[test]
    fn turn_start_does_not_create_a_reasoning_placeholder() {
        let mut widget = ChatWidget::new();
        widget.handle_runtime_event(RuntimeEvent::TurnStarted);
        assert!(widget.active_cells().is_empty());
        assert!(widget.cells().is_empty());
        assert!(matches!(widget.status(), Status::Working));

        widget.handle_runtime_event(RuntimeEvent::ReasoningStart {
            reasoning_id: "reasoning-1".to_owned(),
        });

        assert_eq!(thought_texts(&widget, true).len(), 1);
        assert!(widget.cells().is_empty());
    }

    #[test]
    fn reasoning_deltas_build_the_title_and_commit_the_thought_cell() {
        let mut widget = reasoning_widget();
        send_reasoning_deltas(&mut widget);

        assert_eq!(
            thought_texts(&widget, true),
            vec!["⠋ Thinking: Completing todo updates".to_owned()]
        );

        widget.handle_runtime_event(RuntimeEvent::ReasoningEnd {
            reasoning_id: "reasoning-1".to_owned(),
        });

        assert!(thought_texts(&widget, true).is_empty());
        let committed = thought_texts(&widget, false);
        assert_eq!(committed.len(), 1);
        assert!(committed[0].starts_with("+ Thought: Completing todo updates · "));
        assert!(!committed[0].contains("Check every pending item"));
    }

    #[test]
    fn empty_reasoning_is_discarded_when_it_ends() {
        let mut widget = reasoning_widget();
        widget.handle_runtime_event(RuntimeEvent::ReasoningDelta {
            reasoning_id: "reasoning-1".to_owned(),
            delta: " \n\t".to_owned(),
        });
        widget.handle_runtime_event(RuntimeEvent::ReasoningEnd {
            reasoning_id: "reasoning-1".to_owned(),
        });

        assert!(thought_texts(&widget, true).is_empty());
        assert!(thought_texts(&widget, false).is_empty());
    }

    #[test]
    fn finalizing_empty_reasoning_does_not_persist_a_thought() {
        let mut widget = reasoning_widget();

        widget.finalize_reasoning();

        assert!(thought_texts(&widget, true).is_empty());
        assert!(thought_texts(&widget, false).is_empty());
    }

    #[test]
    fn reasoning_with_a_title_is_committed_without_a_body() {
        let mut widget = reasoning_widget();
        widget.handle_runtime_event(RuntimeEvent::ReasoningDelta {
            reasoning_id: "reasoning-1".to_owned(),
            delta: "Gathering sources".to_owned(),
        });
        widget.handle_runtime_event(RuntimeEvent::ReasoningEnd {
            reasoning_id: "reasoning-1".to_owned(),
        });

        let committed = thought_texts(&widget, false);
        assert_eq!(committed.len(), 1);
        assert!(committed[0].starts_with("+ Thought: Gathering sources · "));
    }

    #[test]
    fn expanding_a_committed_thought_shows_the_full_body() {
        let mut widget = reasoning_widget();
        send_reasoning_deltas(&mut widget);
        widget.handle_runtime_event(RuntimeEvent::ReasoningEnd {
            reasoning_id: "reasoning-1".to_owned(),
        });

        let index = widget
            .cells()
            .iter()
            .position(|cell| cell.as_any().is::<ThoughtCell>())
            .expect("thought cell was committed");
        assert!(widget.toggle_thought(false, index));

        let expanded = thought_texts(&widget, false);
        assert!(expanded[0].starts_with("- Thought: Completing todo updates · "));
        assert!(expanded[0].contains("Check every pending item before the answer."));
    }

    #[test]
    fn reasoning_delta_without_start_opens_the_thought_cell() {
        let mut widget = ChatWidget::new();
        widget.handle_runtime_event(RuntimeEvent::TurnStarted);
        widget.handle_runtime_event(RuntimeEvent::ReasoningDelta {
            reasoning_id: "reasoning-2".to_owned(),
            delta: "**Reading the diff**".to_owned(),
        });

        assert_eq!(
            thought_texts(&widget, true),
            vec!["⠋ Thinking: Reading the diff".to_owned()]
        );
    }

    #[test]
    fn duplicate_reasoning_events_keep_one_active_and_one_committed_thought() {
        let mut widget = reasoning_widget();
        widget.handle_runtime_event(RuntimeEvent::ReasoningStart {
            reasoning_id: "reasoning-1".to_owned(),
        });
        send_reasoning_deltas(&mut widget);
        widget.handle_runtime_event(RuntimeEvent::ReasoningEnd {
            reasoning_id: "reasoning-1".to_owned(),
        });
        widget.handle_runtime_event(RuntimeEvent::ReasoningEnd {
            reasoning_id: "reasoning-1".to_owned(),
        });
        widget.handle_runtime_event(RuntimeEvent::ReasoningDelta {
            reasoning_id: "reasoning-1".to_owned(),
            delta: "late duplicate".to_owned(),
        });

        assert!(thought_texts(&widget, true).is_empty());
        assert_eq!(thought_texts(&widget, false).len(), 1);
        assert!(!thought_texts(&widget, false)[0].contains("late duplicate"));
    }
}
