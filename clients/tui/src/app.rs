use crate::runtime::RuntimeEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Atlas,
    Tool,
    System,
}

impl MessageRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Atlas => "Atlas",
            Self::Tool => "Tool",
            Self::System => "System",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    pub id: Option<String>,
}

impl Message {
    fn new(role: MessageRole, content: impl Into<String>, id: Option<String>) -> Self {
        Self {
            role,
            content: content.into(),
            id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Ready,
    Sending,
    Thinking,
    Tool(String),
    Error(String),
}

impl Status {
    pub fn label(&self) -> String {
        match self {
            Self::Ready => "Ready".to_owned(),
            Self::Sending => "Sending".to_owned(),
            Self::Thinking => "Thinking".to_owned(),
            Self::Tool(name) => format!("Tool: {name}"),
            Self::Error(message) => format!("Error: {message}"),
        }
    }
}

#[derive(Debug)]
pub struct App {
    conversation_id: String,
    messages: Vec<Message>,
    input: String,
    cursor: usize,
    status: Status,
    turn_active: bool,
    should_quit: bool,
}

impl App {
    pub fn new(conversation_id: String) -> Self {
        Self {
            conversation_id,
            messages: Vec::new(),
            input: String::new(),
            cursor: 0,
            status: Status::Ready,
            turn_active: false,
            should_quit: false,
        }
    }

    pub fn conversation_id(&self) -> &str {
        &self.conversation_id
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn cursor_position(&self) -> usize {
        self.input[..self.cursor].chars().count()
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn status_label(&self) -> String {
        self.status().label()
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn turn_active(&self) -> bool {
        self.turn_active
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn insert_character(&mut self, character: char) {
        self.input.insert(self.cursor, character);
        self.cursor += character.len_utf8();
    }

    pub fn backspace(&mut self) {
        let Some((start, _)) = self.input[..self.cursor].char_indices().next_back() else {
            return;
        };
        self.input.drain(start..self.cursor);
        self.cursor = start;
    }

    pub fn move_cursor_left(&mut self) {
        if let Some((start, _)) = self.input[..self.cursor].char_indices().next_back() {
            self.cursor = start;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if let Some(character) = self.input[self.cursor..].chars().next() {
            self.cursor += character.len_utf8();
        }
    }

    pub fn submit_input(&mut self) -> Option<String> {
        if self.turn_active {
            return None;
        }

        let message = self.input.trim().to_owned();
        if message.is_empty() {
            return None;
        }

        self.messages
            .push(Message::new(MessageRole::User, &message, None));
        self.input.clear();
        self.cursor = 0;
        self.status = Status::Sending;
        self.turn_active = true;
        Some(message)
    }

    pub fn handle_runtime_event(&mut self, event: RuntimeEvent) {
        match event {
            RuntimeEvent::MessageDelta { message_id, delta } => {
                self.status = Status::Thinking;
                if let Some(message) = self.messages.iter_mut().rev().find(|message| {
                    message.role == MessageRole::Atlas && message.id.as_deref() == Some(&message_id)
                }) {
                    message.content.push_str(&delta);
                } else {
                    self.messages
                        .push(Message::new(MessageRole::Atlas, delta, Some(message_id)));
                }
            }
            RuntimeEvent::MessageCompleted {
                message_id,
                content,
            } => {
                self.status = Status::Thinking;
                if let Some(message) = self.messages.iter_mut().rev().find(|message| {
                    message.role == MessageRole::Atlas && message.id.as_deref() == Some(&message_id)
                }) {
                    message.content = content;
                } else {
                    self.messages
                        .push(Message::new(MessageRole::Atlas, content, Some(message_id)));
                }
            }
            RuntimeEvent::ToolStarted { tool_name } => {
                self.status = Status::Tool(tool_name.clone());
                self.messages.push(Message::new(
                    MessageRole::Tool,
                    format!("{tool_name} started"),
                    None,
                ));
            }
            RuntimeEvent::ToolCompleted { tool_name, output } => {
                self.status = Status::Thinking;
                let content = match output {
                    Some(output) if !output.is_empty() => {
                        format!("{tool_name} completed: {output}")
                    }
                    _ => format!("{tool_name} completed"),
                };
                self.messages
                    .push(Message::new(MessageRole::Tool, content, None));
            }
            RuntimeEvent::TurnStarted => {
                self.status = Status::Thinking;
                self.turn_active = true;
            }
            RuntimeEvent::TurnCompleted => {
                self.status = Status::Ready;
                self.turn_active = false;
            }
            RuntimeEvent::Error { message } => {
                self.status = Status::Error(message.clone());
                self.turn_active = false;
                self.messages.push(Message::new(
                    MessageRole::System,
                    format!("Error: {message}"),
                    None,
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{App, MessageRole, Status};
    use crate::runtime::RuntimeEvent;

    #[test]
    fn keeps_input_and_cursor_consistent_for_unicode() {
        let mut app = App::new("conversation".to_owned());
        app.insert_character('á');
        app.insert_character('t');
        app.move_cursor_left();
        app.backspace();

        assert_eq!(app.input(), "t");
        assert_eq!(app.cursor_position(), 0);
    }

    #[test]
    fn appends_message_deltas_to_the_same_atlas_message() {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::MessageDelta {
            message_id: "message-1".to_owned(),
            delta: "Hello".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::MessageDelta {
            message_id: "message-1".to_owned(),
            delta: " world".to_owned(),
        });

        assert_eq!(app.messages().len(), 1);
        assert_eq!(app.messages()[0].role, MessageRole::Atlas);
        assert_eq!(app.messages()[0].content, "Hello world");
        assert_eq!(app.status(), &Status::Thinking);
    }

    #[test]
    fn keeps_editing_input_but_ignores_enter_while_thinking() {
        let mut app = App::new("conversation".to_owned());
        app.insert_character('f');
        app.insert_character('i');
        app.insert_character('r');
        app.insert_character('s');
        app.insert_character('t');
        assert_eq!(app.submit_input().as_deref(), Some("first"));

        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        app.insert_character('s');
        app.insert_character('e');
        app.insert_character('c');
        app.insert_character('o');
        app.insert_character('n');
        app.insert_character('d');

        assert_eq!(app.submit_input(), None);
        assert_eq!(app.submit_input(), None);
        assert_eq!(app.input(), "second");
        assert_eq!(app.messages().len(), 1);

        app.handle_runtime_event(RuntimeEvent::TurnCompleted);
        assert_eq!(app.submit_input().as_deref(), Some("second"));
        assert_eq!(app.messages().len(), 2);
    }
}
