//! Ciclo de vida de turnos e tools do Runtime Atlas.

use super::*;

impl ChatWidget {
    pub(super) fn handle_command_lifecycle_event(&mut self, event: RuntimeEvent) {
        match event {
            RuntimeEvent::SessionUpdated { .. } => {}
            RuntimeEvent::TurnStarted => {
                self.status = Status::Working;
                self.turn_active = true;
            }
            RuntimeEvent::ContextUpdated { context } => {
                self.context_usage = Some(context);
            }
            RuntimeEvent::ToolStarted {
                tool_id,
                tool_name,
                target,
            } => {
                self.finalize_reasoning();
                self.status = Status::Executing;
                if self.find_active_tool_mut(&tool_id).is_none() {
                    let tool = ToolCell::new_with_target(
                        tool_id,
                        bounded_metadata(&tool_name),
                        target.map(|value| bounded_metadata(&value)),
                    );
                    let can_group = self
                        .active_tool_group_mut()
                        .is_some_and(|group| group.can_group(&tool_name));
                    if can_group {
                        self.active_tool_group_mut()
                            .expect("active tool group disappeared")
                            .push(tool);
                    } else {
                        self.active_cells
                            .push(Box::new(ToolGroupCell::from_tool(tool)));
                    }
                    self.bump_active_revision();
                }
            }
            RuntimeEvent::ToolCompleted {
                tool_id,
                tool_name,
                output,
            } => {
                if let Some(cell) = self.find_active_tool_mut(&tool_id) {
                    cell.complete_with_elapsed(output);
                    self.commit_active_tool(&tool_id);
                } else if let Some(cell) = self.find_tool_mut(&tool_id) {
                    cell.complete_with_elapsed(output);
                } else {
                    let mut cell = ToolCell::new(tool_id, bounded_metadata(&tool_name));
                    cell.complete_with_elapsed(output);
                    self.cells.push(Box::new(cell));
                }
                if self.active_running_tool_activities().is_empty() && !self.has_running_exec() {
                    self.status = Status::Working;
                } else {
                    self.status = Status::Executing;
                }
                self.history_changed();
            }
            RuntimeEvent::TurnCompleted {
                content,
                message_id,
                context,
            } => self.finish_turn(content, message_id, context),
            RuntimeEvent::TurnCancelled {
                content,
                message_id,
            } => self.cancel_turn(content, message_id),
            RuntimeEvent::Error { message } => self.fail_turn(message),
            _ => {}
        }
    }

    fn finish_turn(
        &mut self,
        content: String,
        message_id: Option<String>,
        context: Option<ContextUsage>,
    ) {
        self.finalize_reasoning();
        if let Some(context) = context {
            self.context_usage = Some(context);
        }
        if !content.is_empty() {
            let id = message_id.or_else(|| {
                self.active_cells.iter().rev().find_map(|cell| {
                    cell.as_any()
                        .downcast_ref::<AgentMessageCell>()
                        .map(|message| message.message_id.clone())
                })
            });
            let id = id.unwrap_or_else(|| "turn-completed".to_owned());
            if let Some(message) = self.find_active_agent_mut(&id) {
                message.set_markdown_source(bounded_text(&content));
                message.completed = true;
                self.commit_active_agent(&id);
            } else if let Some(message) = self.find_markdown_mut(&id) {
                message.set_markdown_source(bounded_text(&content));
            } else if self.find_agent(&id).is_none() {
                self.cells.push(Box::new(AgentMarkdownCell::with_message_id(
                    Some(id),
                    bounded_text(&content),
                )));
            }
        }
        self.commit_all_active_cells();
        self.stream_states.clear();
        self.status = Status::Ready;
        self.turn_active = false;
        self.history_changed();
    }

    fn cancel_turn(&mut self, content: String, message_id: Option<String>) {
        self.cancel_active_cells();
        self.finish_turn(content, message_id, None);
        self.cells.push(Box::new(CancelledCell));
        self.history_changed();
    }

    fn fail_turn(&mut self, message: String) {
        self.finalize_reasoning();
        for cell in &mut self.cells {
            abort_exec_cell(cell.as_mut());
        }
        for cell in &mut self.active_cells {
            abort_exec_cell(cell.as_mut());
        }
        self.cells.append(&mut self.active_cells);
        self.bump_active_revision();
        self.cells
            .push(Box::new(ErrorCell::new(bounded_metadata(&message))));
        self.status = Status::Error(bounded_metadata(&message));
        self.turn_active = false;
        self.history_changed();
    }
}

pub(super) fn abort_exec_cell(cell: &mut dyn HistoryCell) {
    if let Some(exec) = cell.as_any_mut().downcast_mut::<ExecCell>() {
        exec.abort();
    }
    if let Some(group) = cell.as_any_mut().downcast_mut::<RunningGroupCell>() {
        group.abort_all();
    }
}

pub(super) fn abort_tool_cell(cell: &mut dyn HistoryCell) {
    if let Some(tool) = cell.as_any_mut().downcast_mut::<ToolCell>() {
        tool.cancel();
    }
    if let Some(group) = cell.as_any_mut().downcast_mut::<ToolGroupCell>() {
        group.cancel_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_response_stays_working_without_thinking_or_thought() {
        let mut widget = ChatWidget::new();
        widget.handle_runtime_event(RuntimeEvent::TurnStarted);
        assert!(matches!(widget.status(), Status::Working));
        assert!(widget.active_cells().is_empty());

        widget.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-1".to_owned(),
            content: "Ola".to_owned(),
        });
        assert!(matches!(widget.status(), Status::Working));
        assert!(
            widget
                .cells()
                .iter()
                .all(|cell| !cell.as_any().is::<ThoughtCell>())
        );

        widget.handle_runtime_event(RuntimeEvent::TurnCompleted {
            content: "Ola".to_owned(),
            message_id: Some("message-1".to_owned()),
            context: None,
        });
        assert!(matches!(widget.status(), Status::Ready));
        assert_eq!(widget.cells().len(), 1);
    }

    #[test]
    fn duplicate_tool_started_updates_one_active_card() {
        let mut widget = ChatWidget::new();
        widget.handle_runtime_event(RuntimeEvent::TurnStarted);
        widget.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "tool-1".to_owned(),
            tool_name: "search".to_owned(),
            target: None,
        });
        widget.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "tool-1".to_owned(),
            tool_name: "search".to_owned(),
            target: None,
        });

        assert_eq!(widget.active_cells().len(), 1);

        widget.handle_runtime_event(RuntimeEvent::ToolCompleted {
            tool_id: "tool-1".to_owned(),
            tool_name: "search".to_owned(),
            output: Some("done".to_owned()),
        });

        assert!(widget.active_cells().is_empty());
        assert!(matches!(widget.status(), Status::Working));
        assert_eq!(widget.cells().len(), 1);
    }

    #[test]
    fn groups_repeated_completed_tools_in_one_history_cell() {
        let mut widget = ChatWidget::new();
        widget.handle_runtime_event(RuntimeEvent::TurnStarted);
        for (tool_id, target) in [("tool-1", "package.json"), ("tool-2", "Cargo.toml")] {
            widget.handle_runtime_event(RuntimeEvent::ToolStarted {
                tool_id: tool_id.to_owned(),
                tool_name: "filesystem.read".to_owned(),
                target: Some(target.to_owned()),
            });
            widget.handle_runtime_event(RuntimeEvent::ToolCompleted {
                tool_id: tool_id.to_owned(),
                tool_name: "filesystem.read".to_owned(),
                output: None,
            });
        }

        assert_eq!(widget.cells().len(), 1);
        assert_eq!(
            widget
                .cells()
                .iter()
                .filter(|cell| cell.as_any().is::<ToolGroupCell>())
                .count(),
            1
        );
    }

    #[test]
    fn tool_lifecycle_returns_to_working_without_creating_thinking() {
        let mut widget = ChatWidget::new();
        widget.handle_runtime_event(RuntimeEvent::TurnStarted);
        assert!(widget.active_cells().is_empty());
        assert!(matches!(widget.status(), Status::Working));

        widget.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
            target: None,
        });
        assert!(
            widget
                .active_cells()
                .iter()
                .any(|cell| { cell.as_any().downcast_ref::<ToolGroupCell>().is_some() })
        );

        widget.handle_runtime_event(RuntimeEvent::ToolCompleted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
            output: None,
        });
        assert!(widget.active_cells().is_empty());
        assert!(matches!(widget.status(), Status::Working));

        widget.handle_runtime_event(RuntimeEvent::MessageDelta {
            message_id: "message-1".to_owned(),
            delta: "answer".to_owned(),
        });
        assert!(
            widget
                .active_cells()
                .iter()
                .any(|cell| { cell.as_any().downcast_ref::<AgentMessageCell>().is_some() })
        );
        assert_eq!(widget.cells().len(), 1);
    }

    #[test]
    fn tool_then_reasoning_commits_one_thought_before_the_response() {
        let mut widget = ChatWidget::new();
        widget.handle_runtime_event(RuntimeEvent::TurnStarted);
        widget.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
            target: None,
        });
        widget.handle_runtime_event(RuntimeEvent::ToolCompleted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
            output: None,
        });
        assert!(matches!(widget.status(), Status::Working));
        assert!(
            widget
                .cells()
                .iter()
                .all(|cell| !cell.as_any().is::<ThoughtCell>())
        );

        widget.handle_runtime_event(RuntimeEvent::ReasoningStart {
            reasoning_id: "reasoning-1".to_owned(),
        });
        widget.handle_runtime_event(RuntimeEvent::ReasoningDelta {
            reasoning_id: "reasoning-1".to_owned(),
            delta: "**Organizing tools for clarity**".to_owned(),
        });
        assert!(widget.active_cells().iter().any(|cell| {
            cell.as_any()
                .downcast_ref::<ThoughtCell>()
                .is_some_and(|thought| thought.is_running())
        }));
        widget.handle_runtime_event(RuntimeEvent::ReasoningEnd {
            reasoning_id: "reasoning-1".to_owned(),
        });

        assert_eq!(
            widget
                .cells()
                .iter()
                .filter(|cell| cell.as_any().is::<ThoughtCell>())
                .count(),
            1
        );
        assert!(widget.active_cells().is_empty());
    }

    #[test]
    fn cancellation_preserves_partial_output_and_marks_the_turn_without_error() {
        let mut widget = ChatWidget::new();
        widget.handle_runtime_event(RuntimeEvent::TurnStarted);
        widget.handle_runtime_event(RuntimeEvent::MessageDelta {
            message_id: "message-1".to_owned(),
            delta: "partial answer".to_owned(),
        });
        widget.handle_runtime_event(RuntimeEvent::TurnCancelled {
            content: "partial answer".to_owned(),
            message_id: Some("message-1".to_owned()),
        });

        assert!(!widget.turn_active());
        assert!(!matches!(widget.status(), Status::Error(_)));
        assert!(widget.cells().iter().any(|cell| {
            cell.as_any()
                .downcast_ref::<AgentMarkdownCell>()
                .is_some_and(|message| message.markdown_source == "partial answer")
        }));
        assert!(
            widget
                .cells()
                .iter()
                .any(|cell| cell.as_any().is::<CancelledCell>())
        );
    }

    #[test]
    fn cancellation_commits_tools_that_were_already_started() {
        let mut widget = ChatWidget::new();
        widget.handle_runtime_event(RuntimeEvent::TurnStarted);
        widget.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
            target: Some("README.md".to_owned()),
        });
        widget.handle_runtime_event(RuntimeEvent::ToolCompleted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
            output: Some("done".to_owned()),
        });
        widget.handle_runtime_event(RuntimeEvent::TurnCancelled {
            content: String::new(),
            message_id: None,
        });

        assert!(widget.active_cells().is_empty());
        assert!(
            widget
                .cells()
                .iter()
                .any(|cell| cell.as_any().is::<ToolGroupCell>())
        );
        assert!(
            widget
                .cells()
                .iter()
                .any(|cell| cell.as_any().is::<CancelledCell>())
        );
    }
}
