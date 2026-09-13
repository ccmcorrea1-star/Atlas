use crate::presentation::normalize_tool_output;
use crate::runtime::RuntimeEvent;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthChar;

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
            Self::User => "Você",
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
    pub(crate) tool: Option<ToolCall>,
}

impl Message {
    fn new(role: MessageRole, content: impl Into<String>, id: Option<String>) -> Self {
        Self {
            role,
            content: content.into(),
            id,
            tool: None,
        }
    }

    fn tool(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            role: MessageRole::Tool,
            content: String::new(),
            id: None,
            tool: Some(ToolCall {
                name,
                output: None,
                started_at: Some(Instant::now()),
                duration: None,
                completed: false,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolCall {
    pub(crate) name: String,
    pub(crate) output: Option<String>,
    pub(crate) started_at: Option<Instant>,
    pub(crate) duration: Option<Duration>,
    pub(crate) completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Ready,
    Sending,
    Thinking,
    Tool(String),
    Error(String),
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
    history_scroll: usize,
    manual_scroll: bool,
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
            history_scroll: 0,
            manual_scroll: false,
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

    pub fn cursor_byte_position(&self) -> usize {
        self.cursor
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn turn_active(&self) -> bool {
        self.turn_active
    }

    pub fn history_scroll(&self) -> usize {
        self.history_scroll
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn insert_character(&mut self, character: char) {
        self.input.insert(self.cursor, character);
        self.cursor += character.len_utf8();
    }

    pub fn insert_newline(&mut self) {
        self.insert_character('\n');
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

    pub fn move_cursor_home(&mut self) {
        self.cursor = self.input[..self.cursor]
            .rfind('\n')
            .map_or(0, |index| index + 1);
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor = self.input[self.cursor..]
            .find('\n')
            .map_or(self.input.len(), |index| self.cursor + index);
    }

    pub fn move_cursor_up(&mut self) {
        let current_start = self.input[..self.cursor]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        if current_start == 0 {
            return;
        }
        let previous_end = current_start - 1;
        let previous_start = self.input[..previous_end]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let target_column = display_width(&self.input[current_start..self.cursor]);
        self.cursor = position_at_display_column(
            &self.input[previous_start..previous_end],
            previous_start,
            target_column,
        );
    }

    pub fn move_cursor_down(&mut self) {
        let current_end = self.input[self.cursor..]
            .find('\n')
            .map_or(self.input.len(), |index| self.cursor + index);
        if current_end == self.input.len() {
            return;
        }
        let next_start = current_end + 1;
        let next_end = self.input[next_start..]
            .find('\n')
            .map_or(self.input.len(), |index| next_start + index);
        let current_start = self.input[..self.cursor]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let target_column = display_width(&self.input[current_start..self.cursor]);
        self.cursor = position_at_display_column(
            &self.input[next_start..next_end],
            next_start,
            target_column,
        );
    }

    pub fn delete_forward(&mut self) {
        let Some(character) = self.input[self.cursor..].chars().next() else {
            return;
        };
        let end = self.cursor + character.len_utf8();
        self.input.drain(self.cursor..end);
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.history_scroll = self.history_scroll.saturating_add(amount.max(1));
        self.manual_scroll = true;
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.history_scroll = self.history_scroll.saturating_sub(amount.max(1));
        if self.history_scroll == 0 {
            self.manual_scroll = false;
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
        self.history_changed();
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
                    self.history_changed();
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
                    self.history_changed();
                }
            }
            RuntimeEvent::ToolStarted { tool_name } => {
                self.status = Status::Tool(tool_name.clone());
                self.messages.push(Message::tool(tool_name));
                self.history_changed();
            }
            RuntimeEvent::ToolCompleted { tool_name, output } => {
                self.status = Status::Thinking;
                let normalized_output = output
                    .as_deref()
                    .map(normalize_tool_output)
                    .filter(|output| !output.is_empty());
                let mut completed = false;
                if let Some(message) = self.messages.iter_mut().rev().find(|message| {
                    message.role == MessageRole::Tool
                        && message
                            .tool
                            .as_ref()
                            .is_some_and(|tool| tool.name == tool_name && !tool.completed)
                }) {
                    if let Some(tool) = message.tool.as_mut() {
                        tool.output = normalized_output.clone();
                        tool.duration = Some(
                            tool.started_at
                                .map(|started_at| started_at.elapsed())
                                .unwrap_or_default(),
                        );
                        tool.completed = true;
                    }
                    message.content = normalized_output.clone().unwrap_or_default();
                    completed = true;
                }
                if !completed {
                    let mut message = Message::tool(tool_name);
                    if let Some(tool) = message.tool.as_mut() {
                        tool.output = normalized_output.clone();
                        tool.started_at = None;
                        tool.completed = true;
                    }
                    message.content = normalized_output.unwrap_or_default();
                    self.messages.push(message);
                    self.history_changed();
                }
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
                self.history_changed();
            }
        }
    }

    fn history_changed(&mut self) {
        if !self.manual_scroll {
            self.history_scroll = 0;
        }
    }
}

fn display_width(text: &str) -> usize {
    text.chars()
        .map(|character| UnicodeWidthChar::width(character).unwrap_or(0))
        .sum()
}

fn position_at_display_column(text: &str, offset: usize, target: usize) -> usize {
    let mut width = 0;
    for (index, character) in text.char_indices() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if width + character_width > target {
            return offset + index;
        }
        width += character_width;
    }
    offset + text.len()
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
        assert_eq!(app.cursor_byte_position(), 0);
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

    #[test]
    fn keeps_tool_start_and_completion_in_one_transcript_cell() {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_name: "process.exec".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::ToolCompleted {
            tool_name: "process.exec".to_owned(),
            output: Some(r#"{\"ok\":true}"#.to_owned()),
        });

        assert_eq!(app.messages().len(), 1);
        let message = &app.messages()[0];
        assert_eq!(message.role, MessageRole::Tool);
        assert_eq!(message.content, "{\n  \"ok\": true\n}");
        let tool = message.tool.as_ref().expect("tool cell");
        assert!(tool.completed);
        assert!(tool.duration.is_some());
    }

    #[test]
    fn preserves_manual_transcript_scroll_when_new_content_arrives() {
        let mut app = App::new("conversation".to_owned());
        app.scroll_up(4);
        app.handle_runtime_event(RuntimeEvent::MessageDelta {
            message_id: "message-1".to_owned(),
            delta: "new content".to_owned(),
        });

        assert_eq!(app.history_scroll(), 4);
        app.scroll_down(4);
        assert_eq!(app.history_scroll(), 0);
    }

    #[test]
    fn moves_multiline_composer_by_visual_line() {
        let mut app = App::new("conversation".to_owned());
        for character in "ab\ncd".chars() {
            app.insert_character(character);
        }
        app.move_cursor_up();

        assert_eq!(app.input(), "ab\ncd");
        assert_eq!(app.cursor_byte_position(), 2);
    }
}
