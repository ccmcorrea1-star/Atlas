use crate::presentation::parse_tool_output;
use crate::runtime::{ContextUsage, RuntimeEvent};
use crate::transcript::TranscriptLayoutCache;
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const MAX_MESSAGES: usize = 500;
const MAX_HISTORY_CONTENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 128 * 1024;
const MAX_METADATA_BYTES: usize = 8 * 1024;
const MAX_INPUT_BYTES: usize = 64 * 1024;
const TRUNCATION_MARKER: &str = "\n[output truncated]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Atlas,
    Tool,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
    pub id: Option<String>,
    pub(crate) tool: Option<ToolCall>,
    truncated: bool,
}

impl Message {
    fn new(role: MessageRole, content: impl Into<String>, id: Option<String>) -> Self {
        let (content, truncated) = truncate_text(&content.into(), MAX_MESSAGE_BYTES);
        Self {
            role,
            content,
            id,
            tool: None,
            truncated,
        }
    }

    fn tool_with_id(id: impl Into<String>, name: impl Into<String>) -> Self {
        let id = id.into();
        let name = bounded_metadata(&name.into());
        Self {
            role: MessageRole::Tool,
            content: String::new(),
            id: Some(id.clone()),
            tool: Some(ToolCall {
                id,
                name,
                program: None,
                args: Vec::new(),
                cwd: None,
                target: None,
                output: None,
                stderr: None,
                started_at: Some(Instant::now()),
                duration: None,
                exit_code: None,
                execution_status: None,
                completed: false,
                success: true,
            }),
            truncated: false,
        }
    }

    fn execution_with_id(
        id: impl Into<String>,
        capability: impl Into<String>,
        program: String,
        args: Vec<String>,
        cwd: Option<String>,
        target: Option<String>,
    ) -> Self {
        let mut message = Self::tool_with_id(id, capability);
        if let Some(tool) = message.tool.as_mut() {
            tool.program = Some(bounded_metadata(&program));
            tool.args = args
                .iter()
                .map(|argument| bounded_metadata(argument))
                .collect();
            tool.cwd = cwd.map(|path| bounded_metadata(&path));
            tool.target = target.map(|target| bounded_metadata(&target));
        }
        message
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolCall {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) program: Option<String>,
    pub(crate) args: Vec<String>,
    pub(crate) cwd: Option<String>,
    pub(crate) target: Option<String>,
    pub(crate) output: Option<String>,
    pub(crate) stderr: Option<String>,
    pub(crate) started_at: Option<Instant>,
    pub(crate) duration: Option<Duration>,
    pub(crate) exit_code: Option<i32>,
    pub(crate) execution_status: Option<String>,
    pub(crate) completed: bool,
    pub(crate) success: bool,
}

struct ExecutionCompletion {
    stdout: String,
    stderr: String,
    exit_code: i32,
    duration_ms: u64,
    status: String,
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
    #[allow(dead_code)]
    conversation_id: String,
    messages: Vec<Message>,
    input: String,
    cursor: usize,
    status: Status,
    turn_active: bool,
    should_quit: bool,
    history_scroll: usize,
    manual_scroll: bool,
    context_usage: Option<ContextUsage>,
    shortcuts_open: bool,
    quit_confirmation: bool,
    animation_tick: u64,
    transcript_revision: u64,
    transcript_cache: Option<TranscriptLayoutCache>,
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
            context_usage: None,
            shortcuts_open: false,
            quit_confirmation: false,
            animation_tick: 0,
            transcript_revision: 0,
            transcript_cache: None,
        }
    }

    #[allow(dead_code)]
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

    pub fn context_usage(&self) -> Option<ContextUsage> {
        self.context_usage
    }

    #[allow(dead_code)]
    pub(crate) fn set_context_usage(&mut self, context: ContextUsage) {
        self.context_usage = Some(context);
    }

    pub fn shortcuts_open(&self) -> bool {
        self.shortcuts_open
    }

    pub fn open_shortcuts(&mut self) {
        self.shortcuts_open = true;
    }

    pub fn close_shortcuts(&mut self) {
        self.shortcuts_open = false;
    }

    pub fn quit_confirmation(&self) -> bool {
        self.quit_confirmation
    }

    pub(crate) fn animation_tick(&self) -> u64 {
        self.animation_tick
    }

    pub(crate) fn tick(&mut self) {
        self.animation_tick = self.animation_tick.wrapping_add(1);
    }

    pub fn open_quit_confirmation(&mut self) {
        self.quit_confirmation = true;
    }

    pub fn close_quit_confirmation(&mut self) {
        self.quit_confirmation = false;
    }

    pub fn history_scroll(&self) -> usize {
        self.history_scroll
    }

    pub(crate) fn transcript_revision(&self) -> u64 {
        self.transcript_revision
    }

    pub(crate) fn transcript_cache(&self, width: u16) -> Option<&TranscriptLayoutCache> {
        self.transcript_cache
            .as_ref()
            .filter(|cache| cache.revision == self.transcript_revision && cache.width == width)
    }

    pub(crate) fn set_transcript_cache(&mut self, cache: TranscriptLayoutCache) {
        if self.manual_scroll
            && let Some(previous) = self.transcript_cache.as_ref()
            && previous.width == cache.width
        {
            if cache.total_rows >= previous.total_rows {
                self.history_scroll = self
                    .history_scroll
                    .saturating_add(cache.total_rows - previous.total_rows);
            } else {
                self.history_scroll = self
                    .history_scroll
                    .saturating_sub(previous.total_rows - cache.total_rows);
            }
        }
        self.transcript_cache = Some(cache);
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn insert_character(&mut self, character: char) {
        if self.input.len().saturating_add(character.len_utf8()) > MAX_INPUT_BYTES {
            return;
        }
        self.input.insert(self.cursor, character);
        self.cursor += character.len_utf8();
    }

    pub fn insert_newline(&mut self) {
        self.insert_character('\n');
    }

    pub fn backspace(&mut self) {
        let Some((start, _)) = self.input[..self.cursor].grapheme_indices(true).next_back() else {
            return;
        };
        self.input.drain(start..self.cursor);
        self.cursor = start;
    }

    pub fn move_cursor_left(&mut self) {
        if let Some((start, _)) = self.input[..self.cursor].grapheme_indices(true).next_back() {
            self.cursor = start;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if let Some(grapheme) = self.input[self.cursor..].graphemes(true).next() {
            self.cursor += grapheme.len();
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
        let Some(grapheme) = self.input[self.cursor..].graphemes(true).next() else {
            return;
        };
        let end = self.cursor + grapheme.len();
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

    pub fn scroll_to_bottom(&mut self) {
        self.history_scroll = 0;
        self.manual_scroll = false;
    }

    pub fn scroll_to_top(&mut self) {
        self.history_scroll = usize::MAX;
        self.manual_scroll = true;
    }

    pub fn submit_input(&mut self) -> Option<String> {
        if self.turn_active {
            return None;
        }

        if self.input.trim().is_empty() {
            return None;
        }
        let message = self.input.clone();

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
                    append_message_delta(message, &delta);
                    self.history_changed();
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
                    let (content, truncated) = truncate_text(&content, MAX_MESSAGE_BYTES);
                    message.content = content;
                    message.truncated = truncated;
                    self.history_changed();
                } else {
                    self.messages
                        .push(Message::new(MessageRole::Atlas, content, Some(message_id)));
                    self.history_changed();
                }
            }
            RuntimeEvent::ToolStarted { tool_id, tool_name } => {
                self.status = Status::Tool(bounded_metadata(&tool_name));
                if let Some(message) = self.messages.iter_mut().rev().find(|message| {
                    message.role == MessageRole::Tool
                        && message.tool.as_ref().is_some_and(|tool| tool.id == tool_id)
                }) {
                    if let Some(tool) = message.tool.as_mut() {
                        tool.name = bounded_metadata(&tool_name);
                        if tool.completed {
                            tool.completed = false;
                            tool.success = true;
                            tool.started_at = Some(Instant::now());
                            tool.duration = None;
                            tool.exit_code = None;
                            tool.execution_status = None;
                        }
                    }
                } else {
                    self.messages
                        .push(Message::tool_with_id(tool_id.clone(), tool_name));
                }
                self.history_changed();
            }
            RuntimeEvent::ExecutionStarted {
                execution_id,
                capability,
                program,
                args,
                cwd,
                target,
            } => {
                self.status = Status::Tool(bounded_metadata(&capability));
                if let Some(message) = self.messages.iter_mut().rev().find(|message| {
                    message.role == MessageRole::Tool
                        && message
                            .tool
                            .as_ref()
                            .is_some_and(|tool| tool.id == execution_id)
                }) {
                    if let Some(tool) = message.tool.as_mut() {
                        tool.name = bounded_metadata(&capability);
                        tool.program = Some(bounded_metadata(&program));
                        tool.args = args
                            .iter()
                            .map(|argument| bounded_metadata(argument))
                            .collect();
                        tool.cwd = cwd.map(|path| bounded_metadata(&path));
                        tool.target = target.map(|target| bounded_metadata(&target));
                        tool.started_at = Some(Instant::now());
                        tool.completed = false;
                    }
                } else {
                    self.messages.push(Message::execution_with_id(
                        execution_id,
                        capability,
                        program,
                        args,
                        cwd,
                        target,
                    ));
                }
                self.history_changed();
            }
            RuntimeEvent::ToolCompleted {
                tool_id,
                tool_name,
                output,
            } => {
                self.status = Status::Thinking;
                let parsed_output = output.as_deref().map(parse_tool_output);
                let normalized_output = parsed_output
                    .as_ref()
                    .map(|output| output.display_text())
                    .filter(|output| !output.is_empty());
                let mut completed = false;
                if let Some(message) = self.messages.iter_mut().rev().find(|message| {
                    message.role == MessageRole::Tool
                        && message.tool.as_ref().is_some_and(|tool| tool.id == tool_id)
                }) {
                    if let Some(tool) = message.tool.as_mut() {
                        tool.output = parsed_output
                            .as_ref()
                            .map(|output| output.stdout.clone())
                            .filter(|output| !output.is_empty());
                        tool.stderr = parsed_output
                            .as_ref()
                            .map(|output| output.stderr.clone())
                            .filter(|output| !output.is_empty());
                        tool.duration = Some(
                            parsed_output
                                .as_ref()
                                .and_then(|output| output.duration)
                                .or_else(|| tool.started_at.map(|started_at| started_at.elapsed()))
                                .unwrap_or_default(),
                        );
                        tool.completed = true;
                        tool.success = parsed_output
                            .as_ref()
                            .and_then(|output| output.success)
                            .unwrap_or_else(|| {
                                parsed_output
                                    .as_ref()
                                    .is_none_or(|output| output.stderr.is_empty())
                            });
                    }
                    let (content, truncated) = truncate_text(
                        normalized_output.as_deref().unwrap_or_default(),
                        MAX_OUTPUT_BYTES,
                    );
                    message.content = content;
                    message.truncated = truncated;
                    completed = true;
                }
                if !completed {
                    let mut message = Message::tool_with_id(tool_id.clone(), tool_name);
                    if let Some(tool) = message.tool.as_mut() {
                        tool.output = parsed_output
                            .as_ref()
                            .map(|output| output.stdout.clone())
                            .filter(|output| !output.is_empty());
                        tool.stderr = parsed_output
                            .as_ref()
                            .map(|output| output.stderr.clone())
                            .filter(|output| !output.is_empty());
                        tool.started_at = None;
                        tool.duration = parsed_output.as_ref().and_then(|output| output.duration);
                        tool.completed = true;
                        tool.success = parsed_output
                            .as_ref()
                            .and_then(|output| output.success)
                            .unwrap_or_else(|| {
                                parsed_output
                                    .as_ref()
                                    .is_none_or(|output| output.stderr.is_empty())
                            });
                    }
                    let (content, truncated) = truncate_text(
                        normalized_output.as_deref().unwrap_or_default(),
                        MAX_OUTPUT_BYTES,
                    );
                    message.content = content;
                    message.truncated = truncated;
                    self.messages.push(message);
                }
                self.history_changed();
            }
            RuntimeEvent::ExecutionCompleted {
                execution_id,
                capability,
                stdout,
                stderr,
                exit_code,
                duration_ms,
                status,
            } => {
                self.complete_execution(
                    execution_id,
                    capability,
                    ExecutionCompletion {
                        stdout,
                        stderr,
                        exit_code,
                        duration_ms,
                        status,
                    },
                );
            }
            RuntimeEvent::TurnStarted => {
                self.status = Status::Thinking;
                self.turn_active = true;
            }
            RuntimeEvent::TurnCompleted { context } => {
                if let Some(context) = context {
                    self.context_usage = Some(context);
                }
                self.status = Status::Ready;
                self.turn_active = false;
                self.quit_confirmation = false;
            }
            RuntimeEvent::Error { message } => {
                self.status = Status::Error(bounded_metadata(&message));
                self.turn_active = false;
                self.quit_confirmation = false;
                for transcript_message in &mut self.messages {
                    let Some(tool) = transcript_message.tool.as_mut() else {
                        continue;
                    };
                    if tool.completed {
                        continue;
                    }
                    tool.completed = true;
                    tool.success = false;
                    tool.execution_status = Some("aborted".to_owned());
                    tool.duration = tool.started_at.map(|started_at| started_at.elapsed());
                }
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
        // Message mutations invalidate wrapping, height, and scroll calculations.
        self.transcript_revision = self.transcript_revision.wrapping_add(1);
        while (self.messages.len() > MAX_MESSAGES
            || self.history_content_bytes() > MAX_HISTORY_CONTENT_BYTES)
            && self.messages.len() > 1
        {
            let remove_count = self.messages.len() - MAX_MESSAGES;
            if self.messages.len() > MAX_MESSAGES {
                self.messages.drain(..remove_count);
            } else {
                self.messages.remove(0);
            }
        }
        if !self.manual_scroll {
            self.history_scroll = 0;
        }
    }

    fn history_content_bytes(&self) -> usize {
        self.messages
            .iter()
            .map(|message| message.content.len())
            .sum()
    }

    fn complete_execution(
        &mut self,
        execution_id: String,
        capability: String,
        completion: ExecutionCompletion,
    ) {
        let ExecutionCompletion {
            stdout,
            stderr,
            exit_code,
            duration_ms,
            status,
        } = completion;
        self.status = Status::Thinking;
        let (stdout, _) = truncate_text(
            &crate::presentation::sanitize_terminal_text(&stdout),
            MAX_OUTPUT_BYTES,
        );
        let (stderr, _) = truncate_text(
            &crate::presentation::sanitize_terminal_text(&stderr),
            MAX_OUTPUT_BYTES,
        );
        let status = bounded_metadata(&status);
        let mut completed = false;
        if let Some(message) = self.messages.iter_mut().rev().find(|message| {
            message.role == MessageRole::Tool
                && message
                    .tool
                    .as_ref()
                    .is_some_and(|tool| tool.id == execution_id)
        }) {
            if let Some(tool) = message.tool.as_mut() {
                tool.output = (!stdout.is_empty()).then_some(stdout.clone());
                tool.stderr = (!stderr.is_empty()).then_some(stderr.clone());
                tool.duration = Some(Duration::from_millis(duration_ms));
                tool.exit_code = Some(exit_code);
                tool.execution_status = Some(status.clone());
                tool.completed = true;
                tool.success = status == "success" && exit_code == 0;
            }
            let content = [stdout.as_str(), stderr.as_str()]
                .into_iter()
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            let (content, truncated) = truncate_text(&content, MAX_OUTPUT_BYTES);
            message.content = content;
            message.truncated = truncated;
            completed = true;
        }
        if !completed {
            let mut message = Message::execution_with_id(
                execution_id,
                capability,
                String::new(),
                Vec::new(),
                None,
                None,
            );
            if let Some(tool) = message.tool.as_mut() {
                tool.output = (!stdout.is_empty()).then_some(stdout.clone());
                tool.stderr = (!stderr.is_empty()).then_some(stderr.clone());
                tool.started_at = None;
                tool.duration = Some(Duration::from_millis(duration_ms));
                tool.exit_code = Some(exit_code);
                tool.execution_status = Some(status.clone());
                tool.completed = true;
                tool.success = status == "success" && exit_code == 0;
            }
            let content = [stdout.as_str(), stderr.as_str()]
                .into_iter()
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            let (content, truncated) = truncate_text(&content, MAX_OUTPUT_BYTES);
            message.content = content;
            message.truncated = truncated;
            self.messages.push(message);
        }
        self.history_changed();
    }
}

fn truncate_text(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_owned(), false);
    }

    let marker = TRUNCATION_MARKER;
    let mut end = max_bytes.saturating_sub(marker.len()).min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{}{}", &text[..end], marker), true)
}

fn bounded_metadata(text: &str) -> String {
    truncate_text(text, MAX_METADATA_BYTES).0
}

fn append_message_delta(message: &mut Message, delta: &str) {
    if message.truncated {
        return;
    }
    let available = MAX_MESSAGE_BYTES
        .saturating_sub(message.content.len())
        .saturating_sub(TRUNCATION_MARKER.len());
    if delta.len() <= available {
        message.content.push_str(delta);
        return;
    }

    let mut end = available.min(delta.len());
    while end > 0 && !delta.is_char_boundary(end) {
        end -= 1;
    }
    message.content.push_str(&delta[..end]);
    message.content.push_str(TRUNCATION_MARKER);
    message.truncated = true;
}

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

fn position_at_display_column(text: &str, offset: usize, target: usize) -> usize {
    let mut width = 0;
    for (index, grapheme) in text.grapheme_indices(true) {
        let grapheme_width = UnicodeWidthStr::width(grapheme);
        if width + grapheme_width > target {
            return offset + index;
        }
        width += grapheme_width;
    }
    offset + text.len()
}

#[cfg(test)]
mod tests {
    use super::{App, MessageRole, Status};
    use crate::runtime::RuntimeEvent;
    use std::time::Duration;

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

        app.handle_runtime_event(RuntimeEvent::TurnCompleted { context: None });
        assert_eq!(app.submit_input().as_deref(), Some("second"));
        assert_eq!(app.messages().len(), 2);
    }

    #[test]
    fn keeps_tool_start_and_completion_in_one_transcript_cell() {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "tool-1".to_owned(),
            tool_name: "demo.tool".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::ToolCompleted {
            tool_id: "tool-1".to_owned(),
            tool_name: "demo.tool".to_owned(),
            output: Some(r#"{\"stdout\":\"ok\",\"exit_code\":0,\"duration_ms\":10}"#.to_owned()),
        });

        assert_eq!(app.messages().len(), 1);
        let message = &app.messages()[0];
        assert_eq!(message.role, MessageRole::Tool);
        assert_eq!(message.content, "ok");
        let tool = message.tool.as_ref().expect("tool cell");
        assert!(tool.completed);
        assert_eq!(tool.duration, Some(Duration::from_millis(10)));
    }

    #[test]
    fn keeps_execution_start_and_completion_by_execution_id() {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::ExecutionStarted {
            execution_id: "execution-1".to_owned(),
            capability: "process.exec".to_owned(),
            program: "node".to_owned(),
            args: vec!["--version".to_owned()],
            cwd: Some("/tmp".to_owned()),
            target: Some("local".to_owned()),
        });
        app.handle_runtime_event(RuntimeEvent::ExecutionCompleted {
            execution_id: "execution-1".to_owned(),
            capability: "process.exec".to_owned(),
            stdout: "v22.x.x".to_owned(),
            stderr: String::new(),
            exit_code: 0,
            duration_ms: 120,
            status: "success".to_owned(),
        });

        assert_eq!(app.messages().len(), 1);
        let tool = app.messages()[0].tool.as_ref().expect("execution cell");
        assert_eq!(tool.id, "execution-1");
        assert_eq!(tool.program.as_deref(), Some("node"));
        assert_eq!(tool.args, ["--version"]);
        assert_eq!(tool.cwd.as_deref(), Some("/tmp"));
        assert_eq!(tool.exit_code, Some(0));
        assert_eq!(tool.execution_status.as_deref(), Some("success"));
        assert!(tool.completed);
        assert!(tool.success);
    }

    #[test]
    fn does_not_duplicate_an_execution_when_completion_is_repeated() {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::ExecutionStarted {
            execution_id: "execution-1".to_owned(),
            capability: "process.exec".to_owned(),
            program: "node".to_owned(),
            args: vec!["--version".to_owned()],
            cwd: None,
            target: Some("local".to_owned()),
        });
        for output in ["first", "second"] {
            app.handle_runtime_event(RuntimeEvent::ExecutionCompleted {
                execution_id: "execution-1".to_owned(),
                capability: "process.exec".to_owned(),
                stdout: output.to_owned(),
                stderr: String::new(),
                exit_code: 0,
                duration_ms: 8,
                status: "success".to_owned(),
            });
        }

        assert_eq!(app.messages().len(), 1);
        assert_eq!(app.messages()[0].content, "second");
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
    fn matches_same_named_tools_by_id() {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "tool-1".to_owned(),
            tool_name: "demo.tool".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "tool-2".to_owned(),
            tool_name: "demo.tool".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::ToolCompleted {
            tool_id: "tool-1".to_owned(),
            tool_name: "demo.tool".to_owned(),
            output: Some(r#"{\"stdout\":\"first\"}"#.to_owned()),
        });

        assert_eq!(app.messages()[0].content, "first");
        assert!(
            !app.messages()[1]
                .tool
                .as_ref()
                .expect("tool cell")
                .completed
        );
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

    #[test]
    fn edits_combining_graphemes_as_one_character() {
        let mut app = App::new("conversation".to_owned());
        app.insert_character('e');
        app.insert_character('\u{301}');

        app.move_cursor_left();
        assert_eq!(app.cursor_byte_position(), 0);
        app.delete_forward();
        assert!(app.input().is_empty());
    }

    #[test]
    fn moves_right_over_a_combining_grapheme_as_one_character() {
        let mut app = App::new("conversation".to_owned());
        app.insert_character('e');
        app.insert_character('\u{301}');
        app.move_cursor_home();
        app.move_cursor_right();

        assert_eq!(app.cursor_byte_position(), "e\u{301}".len());
    }

    #[test]
    fn deduplicates_repeated_generic_tool_start_events() {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
        });

        assert_eq!(app.messages().len(), 1);
    }

    #[test]
    fn ctrl_home_and_end_control_transcript_scroll_without_moving_composer() {
        let mut app = App::new("conversation".to_owned());
        app.scroll_to_top();
        assert!(app.manual_scroll);
        app.scroll_to_bottom();
        assert_eq!(app.history_scroll(), 0);
        assert!(!app.manual_scroll);
    }

    #[test]
    fn preserves_significant_whitespace_in_submitted_input() {
        let mut app = App::new("conversation".to_owned());
        for character in "  code\n".chars() {
            app.insert_character(character);
        }

        assert_eq!(app.submit_input().as_deref(), Some("  code\n"));
        assert_eq!(app.messages()[0].content, "  code\n");
    }

    #[test]
    fn marks_pending_tools_as_aborted_when_the_turn_fails() {
        let mut app = App::new("conversation".to_owned());
        app.handle_runtime_event(RuntimeEvent::ExecutionStarted {
            execution_id: "execution-1".to_owned(),
            capability: "process.exec".to_owned(),
            program: "sleep".to_owned(),
            args: vec!["10".to_owned()],
            cwd: None,
            target: Some("local".to_owned()),
        });
        app.handle_runtime_event(RuntimeEvent::Error {
            message: "connection lost".to_owned(),
        });

        let tool = app.messages()[0].tool.as_ref().expect("execution cell");
        assert!(tool.completed);
        assert!(!tool.success);
        assert_eq!(tool.execution_status.as_deref(), Some("aborted"));
    }
}
