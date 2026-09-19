//! Ciclo de vida de turnos e tools do Runtime Atlas.

use super::*;

impl ChatWidget {
    pub(super) fn handle_command_lifecycle_event(&mut self, event: RuntimeEvent) {
        match event {
            RuntimeEvent::SessionUpdated { .. } => {}
            RuntimeEvent::TurnStarted => {
                self.status = Status::Thinking;
                self.activity = Some("Thinking".to_owned());
                self.turn_active = true;
                self.turn_started_at = Some(Instant::now());
            }
            RuntimeEvent::ContextUpdated { context } => {
                self.context_usage = Some(context);
            }
            RuntimeEvent::ToolStarted {
                tool_id,
                tool_name,
                target,
            } => {
                self.status = Status::Executing;
                self.activity = Some(crate::capability_names::capability_activity_with_target(
                    &tool_name,
                    target.as_deref(),
                ));
                if self.find_active_tool_mut(&tool_id).is_none() {
                    self.active_cells.push(Box::new(ToolCell::new_with_target(
                        tool_id,
                        bounded_metadata(&tool_name),
                        target.map(|value| bounded_metadata(&value)),
                    )));
                    self.bump_active_revision();
                }
            }
            RuntimeEvent::ToolCompleted {
                tool_id,
                tool_name,
                output,
            } => {
                self.status = Status::Thinking;
                self.activity = Some("Thinking".to_owned());
                if let Some(cell) = self.find_active_tool_mut(&tool_id) {
                    cell.complete(output.map(|text| bounded_output(&text)));
                    self.commit_active_tool(&tool_id);
                } else if let Some(cell) = self.find_tool_mut(&tool_id) {
                    cell.complete(output.map(|text| bounded_output(&text)));
                } else {
                    let mut cell = ToolCell::new(tool_id, bounded_metadata(&tool_name));
                    cell.complete(output.map(|text| bounded_output(&text)));
                    self.cells.push(Box::new(cell));
                }
                self.history_changed();
            }
            RuntimeEvent::TurnCompleted {
                content,
                message_id,
                context,
            } => self.finish_turn(content, message_id, context),
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
        self.status = Status::Ready;
        self.activity = None;
        self.turn_active = false;
        self.turn_started_at = None;
        self.history_changed();
    }

    fn fail_turn(&mut self, message: String) {
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
        self.activity = None;
        self.turn_active = false;
        self.turn_started_at = None;
        self.history_changed();
    }
}

fn abort_exec_cell(cell: &mut dyn HistoryCell) {
    if let Some(exec) = cell.as_any_mut().downcast_mut::<ExecCell>() {
        exec.abort();
    }
    if let Some(group) = cell.as_any_mut().downcast_mut::<RunningGroupCell>() {
        group.abort_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(widget.cells().len(), 1);
    }
}
