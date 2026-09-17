//! Adapter de deltas do Runtime Atlas para o estado de streaming do transcript.

use super::*;

impl ChatWidget {
    pub(super) fn handle_streaming_event(&mut self, event: RuntimeEvent) {
        match event {
            RuntimeEvent::MessageDelta { message_id, delta } => {
                self.status = Status::Thinking;
                if let Some(message) = self.find_active_agent_mut(&message_id) {
                    let available = message.markdown_source.len();
                    message.append(&truncate_delta(&delta, available));
                } else {
                    let is_first_line = self.active_cells.is_empty();
                    self.active_cells.push(Box::new(AgentMessageCell::new(
                        message_id,
                        bounded_text(&delta),
                        is_first_line,
                    )));
                }
                self.bump_active_revision();
                self.history_changed();
            }
            RuntimeEvent::MessageCompleted {
                message_id,
                content,
            } => {
                self.status = Status::Thinking;
                if let Some(message) = self.find_active_agent_mut(&message_id) {
                    message.markdown_source = bounded_text(&content);
                    message.completed = true;
                    self.commit_active_agent(&message_id);
                } else if let Some(message) = self.find_agent_mut(&message_id) {
                    message.markdown_source = bounded_text(&content);
                    message.completed = true;
                } else {
                    self.cells.push(Box::new(AgentMarkdownCell::with_message_id(
                        Some(message_id),
                        bounded_text(&content),
                    )));
                }
                self.history_changed();
            }
            _ => {}
        }
    }
}
