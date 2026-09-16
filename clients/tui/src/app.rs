use std::collections::VecDeque;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::time::Instant;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;

use crate::bottom_pane::BottomPane;
use crate::bottom_pane::paste_burst::CharDecision;
use crate::bottom_pane::paste_burst::FlushResult;
use crate::bottom_pane::prompt_args::parse_slash_name;
use crate::bottom_pane::slash_commands::SlashCommand;
use crate::bottom_pane::slash_commands::exact;
use crate::bottom_pane::slash_commands::matching;
use crate::bottom_pane::textarea::TextArea;
use crate::chatwidget::ChatWidget;
use crate::file_search;
use crate::history_cell::HistoryCell;
use crate::history_store::HistoryStore;
use crate::keymap::Action;
use crate::pager_overlay::TranscriptOverlay;
use crate::runtime::ContextUsage;
use crate::runtime::RuntimeEvent;

const MAX_COMPLETION_ROWS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Status {
    Ready,
    Thinking,
    Executing,
    Error(String),
}

#[derive(Debug)]
pub struct App {
    chatwidget: ChatWidget,
    transcript_overlay: TranscriptOverlay,
    bottom_pane: BottomPane,
    history_store: HistoryStore,
    should_quit: bool,
    history_scroll: usize,
    history_content_height: usize,
    manual_scroll: bool,

    quit_confirmation: bool,
    queued_inputs: VecDeque<String>,
    submission_pending: bool,
    cancel_requested: bool,
    external_editor_requested: bool,
}

impl App {
    pub fn new(conversation_id: String) -> Self {
        #[cfg(test)]
        let _ = conversation_id;
        #[cfg(test)]
        let history_store = HistoryStore::disabled();
        #[cfg(not(test))]
        let history_store = HistoryStore::for_conversation(&conversation_id);
        Self::with_history_store(history_store)
    }

    #[cfg(test)]
    pub(crate) fn new_with_history_dir(conversation_id: &str, root: impl Into<PathBuf>) -> Self {
        Self::with_history_store(HistoryStore::new(root, conversation_id))
    }

    fn with_history_store(history_store: HistoryStore) -> Self {
        let mut bottom_pane = BottomPane::new();
        bottom_pane.composer.history_entries = history_store.load();
        Self {
            chatwidget: ChatWidget::new(),
            transcript_overlay: TranscriptOverlay::default(),
            bottom_pane,
            history_store,
            should_quit: false,
            history_scroll: 0,
            history_content_height: 0,
            manual_scroll: false,

            quit_confirmation: false,
            queued_inputs: VecDeque::new(),
            submission_pending: false,
            cancel_requested: false,
            external_editor_requested: false,
        }
    }

    pub fn cells(&self) -> &[Box<dyn HistoryCell>] {
        self.chatwidget.cells()
    }

    pub(crate) fn active_cells(&self) -> &[Box<dyn HistoryCell>] {
        self.chatwidget.active_cells()
    }

    pub(crate) fn active_revision(&self) -> u64 {
        self.chatwidget.active_revision()
    }

    pub(crate) fn transcript_overlay(&self) -> &TranscriptOverlay {
        &self.transcript_overlay
    }

    pub fn input(&self) -> &str {
        self.bottom_pane.composer.textarea.text()
    }

    pub(crate) fn textarea(&self) -> &TextArea {
        &self.bottom_pane.composer.textarea
    }

    pub(crate) fn bottom_pane(&self) -> &BottomPane {
        &self.bottom_pane
    }

    pub fn status(&self) -> &Status {
        self.chatwidget.status()
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn turn_active(&self) -> bool {
        self.chatwidget.turn_active()
    }

    pub fn take_cancel_requested(&mut self) -> bool {
        std::mem::take(&mut self.cancel_requested)
    }

    pub fn take_external_editor_requested(&mut self) -> bool {
        std::mem::take(&mut self.external_editor_requested)
    }

    pub(crate) fn working_seconds(&self) -> u64 {
        self.chatwidget.working_seconds()
    }

    pub fn context_usage(&self) -> Option<ContextUsage> {
        self.chatwidget.context_usage()
    }

    pub fn shortcuts_open(&self) -> bool {
        self.bottom_pane.shortcuts_open()
    }

    pub fn open_shortcuts(&mut self) {
        self.bottom_pane.open_shortcuts();
    }

    pub fn close_shortcuts(&mut self) {
        self.bottom_pane.close_shortcuts();
    }

    pub fn transcript_open(&self) -> bool {
        self.transcript_overlay.is_open()
    }

    pub fn open_transcript(&mut self) {
        self.transcript_overlay.open();
    }

    pub fn quit_confirmation(&self) -> bool {
        self.quit_confirmation
    }

    pub(crate) fn esc_backtrack_hint(&self) -> bool {
        self.bottom_pane.composer.esc_backtrack_hint
    }

    pub(crate) fn history_search_open(&self) -> bool {
        self.bottom_pane.composer.history_search_open
    }

    pub(crate) fn history_search_query(&self) -> &str {
        &self.bottom_pane.composer.history_search_query
    }

    pub(crate) fn history_search_has_match(&self) -> bool {
        !self.bottom_pane.composer.history_search_matches.is_empty()
    }

    pub(crate) fn slash_popup_commands(&self) -> Vec<SlashCommand> {
        if !self.slash_popup_active() {
            return Vec::new();
        }
        let prefix = self.input().lines().next().unwrap_or_default();
        matching(prefix).collect()
    }

    fn slash_popup_active(&self) -> bool {
        if self.bottom_pane.composer.slash_popup_suppressed
            || self.shortcuts_open()
            || self.bottom_pane.composer.history_search_open
        {
            return false;
        }
        let first_line = self.input().lines().next().unwrap_or_default();
        if self.input().contains('\n') {
            return false;
        }
        first_line == "/"
            || parse_slash_name(first_line).is_some_and(|(_, rest, _)| rest.is_empty())
    }

    fn all_completion_popup_items(&self) -> Vec<(String, String)> {
        if self.slash_popup_active() {
            return self
                .slash_popup_commands()
                .into_iter()
                .map(|command| (command.name.to_owned(), command.description.to_owned()))
                .collect();
        }
        self.file_popup_items()
    }

    pub(crate) fn completion_popup_items(&self) -> Vec<(String, String)> {
        let start = self.completion_popup_start();
        self.all_completion_popup_items()
            .into_iter()
            .skip(start)
            .take(MAX_COMPLETION_ROWS)
            .collect()
    }

    pub(crate) fn completion_selection(&self) -> usize {
        self.bottom_pane
            .composer
            .completion_selection
            .min(self.all_completion_popup_items().len().saturating_sub(1))
    }

    pub(crate) fn completion_popup_selected_row(&self) -> Option<usize> {
        self.completion_selection()
            .checked_sub(self.completion_popup_start())
            .filter(|row| *row < self.completion_popup_items().len())
    }

    fn completion_popup_start(&self) -> usize {
        self.completion_selection()
            .saturating_sub(MAX_COMPLETION_ROWS.saturating_sub(1))
    }

    fn move_completion_selection(&mut self, down: bool) {
        let item_count = self.all_completion_popup_items().len();
        if item_count == 0 {
            self.bottom_pane.composer.completion_selection = 0;
            return;
        }
        self.bottom_pane.composer.completion_selection = if down {
            (self.bottom_pane.composer.completion_selection + 1) % item_count
        } else {
            if self.bottom_pane.composer.completion_selection == 0 {
                item_count - 1
            } else {
                self.bottom_pane.composer.completion_selection - 1
            }
        };
    }

    fn file_popup_items(&self) -> Vec<(String, String)> {
        if !self.file_popup_active() {
            return Vec::new();
        }
        let Some((_, _, query)) = self.file_token() else {
            return Vec::new();
        };
        let Ok(root) = std::env::current_dir() else {
            return Vec::new();
        };
        file_search::search(Path::new(&root), &query)
            .into_iter()
            .map(|path| {
                let display = path.to_string_lossy().into_owned();
                (format!("@{display}"), "file".to_owned())
            })
            .collect()
    }

    fn file_token(&self) -> Option<(usize, usize, String)> {
        let cursor = self.bottom_pane.composer.textarea.cursor();
        let before = &self.input()[..cursor];
        let start = before.rfind('@')?;
        if start > 0 {
            let previous = before[..start].chars().next_back()?;
            if !previous.is_whitespace() {
                return None;
            }
        }
        let query = &before[start + 1..];
        if query.chars().any(char::is_whitespace) {
            return None;
        }
        Some((start, cursor, query.to_owned()))
    }

    fn file_popup_active(&self) -> bool {
        !self.bottom_pane.composer.file_popup_suppressed
            && !self.shortcuts_open()
            && !self.bottom_pane.composer.history_search_open
            && !self.slash_popup_active()
            && self.file_token().is_some()
    }

    fn complete_file_mention(&mut self) {
        let Some((start, cursor, _)) = self.file_token() else {
            return;
        };
        let Some((label, _)) = self
            .all_completion_popup_items()
            .into_iter()
            .nth(self.completion_selection())
        else {
            return;
        };
        self.bottom_pane
            .composer
            .textarea
            .replace_range(start..cursor, &format!("{label} "));
        self.bottom_pane.composer.file_popup_suppressed = true;
        self.bottom_pane.composer.completion_selection = 0;
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

    pub(crate) fn record_history_content_height(&mut self, height: usize) {
        if self.manual_scroll && height > self.history_content_height {
            self.history_scroll = self
                .history_scroll
                .saturating_add(height - self.history_content_height);
        }
        self.history_content_height = height;
    }

    pub fn tick(&mut self) {
        self.chatwidget.tick();
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn insert_character(&mut self, character: char) {
        self.insert_character_direct(character);
    }

    fn insert_character_direct(&mut self, character: char) {
        self.bottom_pane
            .composer
            .textarea
            .insert_str(&character.to_string());
    }

    pub fn insert_text(&mut self, text: &str) {
        self.bottom_pane.composer.textarea.insert_str(text);
    }

    pub(crate) fn flush_paste_burst_if_due_at(&mut self, now: Instant) -> bool {
        match self.bottom_pane.composer.paste_burst.flush_if_due(now) {
            FlushResult::Paste(text) => {
                self.handle_paste(&text);
                true
            }
            FlushResult::Typed(character) => {
                self.insert_character_direct(character);
                true
            }
            FlushResult::None => false,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn is_in_paste_burst(&self) -> bool {
        self.bottom_pane.composer.paste_burst.is_in_progress()
    }

    /// Paste belongs to the active Codex view, never to a hidden composer.
    pub fn handle_paste(&mut self, text: &str) {
        if self.shortcuts_open()
            || self.transcript_overlay.is_open()
            || self.quit_confirmation
            || self.bottom_pane.composer.history_search_open
        {
            return;
        }
        self.bottom_pane
            .composer
            .paste_burst
            .clear_after_explicit_paste();
        self.insert_text(text);
    }

    pub fn insert_newline(&mut self) {
        self.insert_character('\n');
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

    pub fn scroll_to_top(&mut self) {
        self.history_scroll = usize::MAX;
        self.manual_scroll = true;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.history_scroll = 0;
        self.manual_scroll = false;
    }

    pub fn submit_input(&mut self) -> Option<String> {
        if self.input().trim().is_empty() {
            return None;
        }
        let command = self.input().trim().to_owned();
        if let Some(command_item) = exact(&command) {
            self.clear_input();
            self.bottom_pane.composer.slash_popup_suppressed = false;
            match command_item.name {
                "/help" => self.open_shortcuts(),
                "/quit" => self.quit(),
                _ => {}
            }
            return None;
        }
        let message = self.input().to_owned();
        if self.bottom_pane.composer.history_entries.last() != Some(&message) {
            self.bottom_pane
                .composer
                .history_entries
                .push(message.clone());
            let _ = self
                .history_store
                .save(&self.bottom_pane.composer.history_entries);
        }
        self.bottom_pane.composer.history_index = None;
        self.clear_input();
        if self.turn_active() || self.submission_pending {
            self.queued_inputs.push_back(message);
            return None;
        }
        self.chatwidget.add_user_message(message.clone());
        self.submission_pending = true;
        Some(message)
    }

    pub fn take_queued_input(&mut self) -> Option<String> {
        let input = self.queued_inputs.pop_front()?;
        self.chatwidget.add_user_message(input.clone());
        self.submission_pending = true;
        Some(input)
    }

    pub fn apply_external_editor_text(&mut self, text: &str) {
        self.bottom_pane
            .composer
            .textarea
            .set_text_clearing_elements(text);
        self.bottom_pane.composer.slash_popup_suppressed = false;
    }

    pub fn handle_runtime_event(&mut self, event: RuntimeEvent) {
        let terminal = event.is_terminal();
        self.chatwidget.handle_runtime_event(event);
        if terminal {
            self.submission_pending = false;
        }
    }

    fn complete_slash_command(&mut self) {
        let command_name = self
            .all_completion_popup_items()
            .into_iter()
            .nth(self.completion_selection())
            .map(|(name, _)| name);
        if let Some(command_name) = command_name {
            self.bottom_pane
                .composer
                .textarea
                .set_text_clearing_elements(&command_name);
            self.bottom_pane.composer.slash_popup_suppressed = true;
            self.bottom_pane.composer.completion_selection = 0;
        }
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> Option<String> {
        self.bottom_pane.composer.esc_backtrack_hint = false;
        if key.modifiers == KeyModifiers::NONE
            && matches!(key.code, KeyCode::Char(_) | KeyCode::Backspace)
        {
            self.bottom_pane.composer.slash_popup_suppressed = false;
            self.bottom_pane.composer.file_popup_suppressed = false;
            self.bottom_pane.composer.completion_selection = 0;
        }
        if matches!(key.code, KeyCode::Enter | KeyCode::Tab)
            && key.modifiers == KeyModifiers::NONE
            && !self.completion_popup_items().is_empty()
        {
            if self.slash_popup_active() {
                self.complete_slash_command();
            } else {
                self.complete_file_mention();
            }
            return None;
        }
        let modified = key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);

        if !modified && let KeyCode::Char(character) = key.code {
            match self
                .bottom_pane
                .composer
                .paste_burst
                .on_plain_char(character, Instant::now())
            {
                CharDecision::RetainFirstChar => return None,
                CharDecision::BeginBufferFromPending
                | CharDecision::BufferAppend
                | CharDecision::BeginBuffer { .. } => {
                    self.bottom_pane
                        .composer
                        .paste_burst
                        .append_char_to_buffer(character, Instant::now());
                    return None;
                }
            }
        }

        if !modified
            && matches!(key.code, KeyCode::Enter | KeyCode::Tab)
            && self
                .bottom_pane
                .composer
                .paste_burst
                .append_control_char_if_active(
                    if key.code == KeyCode::Enter {
                        '\n'
                    } else {
                        '\t'
                    },
                    Instant::now(),
                )
        {
            return None;
        }

        if let Some(text) = self
            .bottom_pane
            .composer
            .paste_burst
            .flush_before_modified_input()
        {
            self.handle_paste(&text);
        }
        self.bottom_pane
            .composer
            .paste_burst
            .clear_window_after_non_char();

        match key.code {
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.insert_newline();
                None
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.bottom_pane.composer.textarea.input(key);
                None
            }
            KeyCode::Enter | KeyCode::Tab => self.submit_input(),
            _ => {
                self.bottom_pane.composer.textarea.input(key);
                None
            }
        }
    }

    pub fn handle_global_key(&mut self, key: KeyEvent) -> bool {
        let action = crate::keymap::resolve(key);
        if self.quit_confirmation {
            match action {
                Some(Action::ConfirmQuit) | Some(Action::ClearDraft) => self.quit(),
                Some(Action::Cancel) | Some(Action::DeclineQuit) => self.close_quit_confirmation(),
                _ => {}
            }
            return true;
        }
        if self.bottom_pane.composer.esc_backtrack_hint {
            if action == Some(Action::Cancel) {
                self.edit_previous_message();
            } else {
                self.bottom_pane.composer.esc_backtrack_hint = false;
            }
            if action == Some(Action::Cancel) {
                return true;
            }
        }
        if self.all_completion_popup_items().len() > 1 {
            match (key.code, key.modifiers) {
                (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                    self.move_completion_selection(false);
                    return true;
                }
                (KeyCode::Down, KeyModifiers::NONE)
                | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                    self.move_completion_selection(true);
                    return true;
                }
                _ => {}
            }
        }
        if self.file_popup_active() && action == Some(Action::Cancel) {
            self.bottom_pane.composer.file_popup_suppressed = true;
            return true;
        }
        if self.slash_popup_active() && action == Some(Action::Cancel) {
            self.bottom_pane.composer.slash_popup_suppressed = true;
            return true;
        }
        if self.file_popup_active() || self.slash_popup_active() {
            return false;
        }
        if self.bottom_pane.composer.history_search_open {
            match key.code {
                KeyCode::Char(character)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.bottom_pane
                        .composer
                        .history_search_query
                        .push(character);
                    self.refresh_history_search();
                }
                KeyCode::Backspace => {
                    self.bottom_pane.composer.history_search_query.pop();
                    self.refresh_history_search();
                }
                _ => match action {
                    Some(Action::Cancel) => self.cancel_history_search(),
                    Some(Action::ConfirmQuit) => self.accept_history_search(),
                    Some(Action::OpenHistorySearch) | Some(Action::ScrollUp) => {
                        self.step_history_search(true)
                    }
                    Some(Action::ScrollDown) => self.step_history_search(false),
                    _ => return true,
                },
            }
            return true;
        }
        if self.shortcuts_open() {
            if action == Some(Action::Cancel) || action == Some(Action::OpenShortcuts) {
                self.close_shortcuts();
            }
            return true;
        }
        if self.transcript_overlay.is_open() {
            let _ = self.transcript_overlay.handle_action(action);
            return true;
        }

        if action == Some(Action::OpenHistorySearch)
            && !self.bottom_pane.composer.history_entries.is_empty()
        {
            self.bottom_pane.composer.history_search_open = true;
            self.bottom_pane.composer.history_search_query.clear();
            self.bottom_pane.composer.history_search_draft = self.input().to_owned();
            self.bottom_pane.composer.history_search_matches =
                (0..self.bottom_pane.composer.history_entries.len()).collect();
            self.bottom_pane.composer.history_search_match = None;
            self.bottom_pane.composer.history_index = None;
            return true;
        }
        if action == Some(Action::OpenExternalEditor) && !self.turn_active() {
            self.external_editor_requested = true;
            return true;
        }
        if action == Some(Action::OpenTranscript) {
            self.open_transcript();
            return true;
        }
        if action == Some(Action::OpenShortcuts) && self.input().is_empty() {
            self.open_shortcuts();
            return true;
        }
        match action {
            Some(Action::PageUp) => self.scroll_up(8),
            Some(Action::PageDown) => self.scroll_down(8),
            Some(Action::JumpTop) => self.scroll_to_top(),
            Some(Action::JumpBottom) => self.scroll_to_bottom(),
            Some(Action::Cancel) if self.turn_active() => {
                self.cancel_requested = true;
            }
            Some(Action::Cancel) => self.bottom_pane.composer.esc_backtrack_hint = true,
            Some(Action::ClearDraft) => {
                if self.input().is_empty() {
                    self.open_quit_confirmation();
                } else {
                    self.clear_input();
                }
            }
            _ => return false,
        }
        true
    }

    pub fn handle_mouse_event(&mut self, mouse: MouseEvent) {
        if self.file_popup_active() || self.slash_popup_active() {
            return;
        }
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_up(3),
            MouseEventKind::ScrollDown => self.scroll_down(3),
            _ => {}
        }
    }

    fn clear_input(&mut self) {
        self.bottom_pane
            .composer
            .textarea
            .set_text_clearing_elements("");
    }

    fn cancel_history_search(&mut self) {
        self.bottom_pane.composer.history_search_open = false;
        self.bottom_pane.composer.history_search_query.clear();
        self.bottom_pane.composer.history_search_matches.clear();
        self.bottom_pane.composer.history_search_match = None;
        self.bottom_pane.composer.history_index = None;
        let draft = self.bottom_pane.composer.history_search_draft.clone();
        self.bottom_pane
            .composer
            .textarea
            .set_text_clearing_elements(&draft);
    }

    fn accept_history_search(&mut self) {
        self.bottom_pane.composer.history_search_open = false;
        self.bottom_pane.composer.history_search_query.clear();
        self.bottom_pane.composer.history_search_matches.clear();
        self.bottom_pane.composer.history_search_match = None;
        self.bottom_pane.composer.history_index = None;
    }

    fn refresh_history_search(&mut self) {
        let query = self
            .bottom_pane
            .composer
            .history_search_query
            .to_lowercase();
        self.bottom_pane.composer.history_search_matches = self
            .bottom_pane
            .composer
            .history_entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                (query.is_empty() || entry.to_lowercase().contains(&query)).then_some(index)
            })
            .collect();
        self.bottom_pane.composer.history_search_match = None;
        if let Some(&index) = self.bottom_pane.composer.history_search_matches.last() {
            self.bottom_pane.composer.history_search_match =
                Some(self.bottom_pane.composer.history_search_matches.len() - 1);
            self.bottom_pane.composer.history_index = Some(index);
            self.bottom_pane
                .composer
                .textarea
                .set_text_clearing_elements(&self.bottom_pane.composer.history_entries[index]);
        } else {
            self.bottom_pane.composer.history_index = None;
            let draft = self.bottom_pane.composer.history_search_draft.clone();
            self.bottom_pane
                .composer
                .textarea
                .set_text_clearing_elements(&draft);
        }
    }

    fn step_history_search(&mut self, previous: bool) {
        if self.bottom_pane.composer.history_search_matches.is_empty() {
            return;
        }
        let next = match (self.bottom_pane.composer.history_search_match, previous) {
            (None, _) => self.bottom_pane.composer.history_search_matches.len() - 1,
            (Some(index), true) => index.saturating_sub(1),
            (Some(index), false) => {
                if index + 1 >= self.bottom_pane.composer.history_search_matches.len() {
                    0
                } else {
                    index + 1
                }
            }
        };
        self.bottom_pane.composer.history_search_match = Some(next);
        let history_index = self.bottom_pane.composer.history_search_matches[next];
        self.bottom_pane.composer.history_index = Some(history_index);
        self.bottom_pane
            .composer
            .textarea
            .set_text_clearing_elements(&self.bottom_pane.composer.history_entries[history_index]);
    }

    fn edit_previous_message(&mut self) {
        if let Some(message) = self.chatwidget.take_previous_user_message() {
            self.bottom_pane
                .composer
                .textarea
                .set_text_clearing_elements(&message);
        }
        self.bottom_pane.composer.esc_backtrack_hint = false;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;
    use std::time::SystemTime;
    use std::time::UNIX_EPOCH;

    use super::App;
    use crate::bottom_pane::BottomPaneView;
    use crate::bottom_pane::bottom_pane_view::ChatComposerView;
    use crate::runtime::RuntimeEvent;

    #[test]
    fn restores_history_per_conversation_after_reopening_the_app() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("atlas-app-history-{}-{suffix}", std::process::id()));

        let mut first = App::new_with_history_dir("conversation-a", &root);
        first.insert_text("first line\nsecond line");
        assert_eq!(
            first.submit_input().as_deref(),
            Some("first line\nsecond line")
        );

        let restored = App::new_with_history_dir("conversation-a", &root);
        assert_eq!(
            restored.bottom_pane.composer.history_entries,
            vec!["first line\nsecond line".to_owned()]
        );

        let isolated = App::new_with_history_dir("conversation-b", &root);
        assert!(isolated.bottom_pane.composer.history_entries.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn streams_into_an_active_cell_before_committing_to_history() {
        let mut app = App::new("streaming".to_owned());
        app.handle_runtime_event(RuntimeEvent::MessageDelta {
            message_id: "message-1".to_owned(),
            delta: "partial".to_owned(),
        });
        assert!(app.cells().is_empty());
        assert_eq!(app.active_cells().len(), 1);

        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-1".to_owned(),
            content: "complete".to_owned(),
        });
        assert!(app.active_cells().is_empty());
        assert_eq!(app.cells().len(), 1);
    }

    #[test]
    fn does_not_duplicate_message_completed_at_turn_completion() {
        let mut app = App::new("completed".to_owned());
        app.handle_runtime_event(RuntimeEvent::MessageDelta {
            message_id: "message-1".to_owned(),
            delta: "partial".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::MessageCompleted {
            message_id: "message-1".to_owned(),
            content: "complete".to_owned(),
        });
        app.handle_runtime_event(RuntimeEvent::TurnCompleted {
            content: "complete".to_owned(),
            message_id: Some("message-1".to_owned()),
            context: None,
        });

        assert_eq!(app.cells().len(), 1);
    }

    #[test]
    fn queues_second_submission_until_the_first_turn_finishes() {
        let mut app = App::new("queue".to_owned());
        app.insert_text("first");
        assert_eq!(app.submit_input().as_deref(), Some("first"));
        app.insert_text("second");
        assert_eq!(app.submit_input(), None);
        assert_eq!(app.cells().len(), 1);

        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        app.handle_runtime_event(RuntimeEvent::TurnCompleted {
            content: "done".to_owned(),
            message_id: None,
            context: None,
        });
        assert_eq!(app.take_queued_input().as_deref(), Some("second"));
        assert_eq!(app.cells().len(), 3);
    }

    #[test]
    fn paste_burst_commits_fast_ascii_as_one_paste_on_tick() {
        let mut app = App::new("paste-burst".to_owned());
        let plain = crossterm::event::KeyModifiers::NONE;
        app.handle_key_event(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            plain,
        ));
        app.handle_key_event(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('b'),
            plain,
        ));
        std::thread::sleep(std::time::Duration::from_millis(12));
        assert!(ChatComposerView.pre_draw_tick(&mut app, Instant::now()));

        assert_eq!(app.input(), "ab");
    }

    #[test]
    fn paste_burst_enter_becomes_newline_instead_of_submission() {
        let mut app = App::new("paste-enter".to_owned());
        let plain = crossterm::event::KeyModifiers::NONE;
        app.handle_key_event(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            plain,
        ));
        app.handle_key_event(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('b'),
            plain,
        ));
        app.handle_key_event(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            plain,
        ));
        std::thread::sleep(std::time::Duration::from_millis(12));
        assert!(ChatComposerView.pre_draw_tick(&mut app, Instant::now()));

        assert_eq!(app.input(), "ab\n");
    }

    #[test]
    fn reverse_history_search_filters_matches_and_restores_draft_on_cancel() {
        let mut app = App::new("history-search".to_owned());
        app.insert_text("first command");
        assert_eq!(app.submit_input().as_deref(), Some("first command"));
        app.insert_text("Second command");
        assert_eq!(app.submit_input(), None);
        app.insert_text("draft");

        let ctrl_r = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('r'),
            crossterm::event::KeyModifiers::CONTROL,
        );
        assert!(app.handle_global_key(ctrl_r));
        assert!(app.history_search_open());
        for character in "SECOND".chars() {
            assert!(app.handle_global_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(character),
                crossterm::event::KeyModifiers::NONE,
            )));
        }
        assert_eq!(app.input(), "Second command");
        assert_eq!(app.history_search_query(), "SECOND");

        assert!(app.handle_global_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        )));
        assert!(!app.history_search_open());
        assert_eq!(app.input(), "draft");
    }

    #[test]
    fn reverse_history_search_reports_no_match_without_losing_draft() {
        let mut app = App::new("history-search-empty".to_owned());
        app.insert_text("known command");
        assert_eq!(app.submit_input().as_deref(), Some("known command"));
        app.insert_text("draft");
        assert!(app.handle_global_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('r'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
        assert!(app.handle_global_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('z'),
            crossterm::event::KeyModifiers::NONE,
        )));
        assert!(!app.history_search_has_match());
        assert_eq!(app.input(), "draft");
    }

    #[test]
    fn slash_popup_completes_and_dispatches_help_locally() {
        let mut app = App::new("slash-help".to_owned());
        app.insert_text("/");
        assert_eq!(app.slash_popup_commands().len(), 2);

        assert!(
            app.handle_key_event(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Tab,
                crossterm::event::KeyModifiers::NONE,
            ))
            .is_none()
        );
        assert_eq!(app.input(), "/help");
        assert!(
            app.handle_key_event(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ))
            .is_none()
        );
        assert!(app.shortcuts_open());
        assert!(app.input().is_empty());
    }

    #[test]
    fn enter_confirms_the_selected_slash_completion_instead_of_submitting() {
        let mut app = App::new("slash-enter".to_owned());
        app.insert_text("/");
        assert!(
            app.handle_key_event(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ))
            .is_none()
        );
        assert_eq!(app.input(), "/help");
        assert!(!app.shortcuts_open());
        assert!(app.cells().is_empty());
    }

    #[test]
    fn completion_popup_moves_selection_before_tab_completion() {
        let mut app = App::new("slash-selection".to_owned());
        app.insert_text("/");
        assert_eq!(app.completion_selection(), 0);

        assert!(app.handle_global_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(app.completion_selection(), 1);
        assert!(app.handle_global_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('n'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
        assert_eq!(app.completion_selection(), 0);
        assert!(app.handle_global_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('p'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
        assert_eq!(app.completion_selection(), 1);
        assert!(app.handle_global_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(app.completion_selection(), 0);
        assert!(app.handle_global_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Up,
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(app.completion_selection(), 1);

        assert!(
            app.handle_key_event(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Tab,
                crossterm::event::KeyModifiers::NONE,
            ))
            .is_none()
        );
        assert_eq!(app.input(), "/quit");
    }

    #[test]
    fn slash_popup_escape_only_dismisses_the_popup() {
        let mut app = App::new("slash-escape".to_owned());
        app.insert_text("/");
        assert!(app.handle_global_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        )));
        assert!(!app.shortcuts_open());
        assert!(!app.take_cancel_requested());
        assert_eq!(app.input(), "/");
        assert!(app.slash_popup_commands().is_empty());
    }

    #[test]
    fn popup_keeps_transcript_scroll_keys_inside_the_composer() {
        let mut app = App::new("popup-scroll".to_owned());
        app.insert_text("/");
        assert!(!app.handle_global_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::PageUp,
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(app.history_scroll(), 0);
        assert!(!app.handle_global_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('t'),
            crossterm::event::KeyModifiers::CONTROL,
        )));
    }

    #[test]
    fn popup_keeps_mouse_scroll_inside_the_composer() {
        let mut app = App::new("popup-mouse-scroll".to_owned());
        app.insert_text("/");
        app.handle_mouse_event(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        });
        assert_eq!(app.history_scroll(), 0);
    }

    #[test]
    fn app_routes_control_p_and_control_n_to_multiline_composer_navigation() {
        let mut app = App::new("emacs-navigation".to_owned());
        app.insert_text("first\nsecond");
        let up = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('p'),
            crossterm::event::KeyModifiers::CONTROL,
        );
        let down = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('n'),
            crossterm::event::KeyModifiers::CONTROL,
        );

        assert!(!app.handle_global_key(up));
        app.handle_key_event(up);
        assert_eq!(app.textarea().cursor(), 5);
        app.handle_key_event(down);
        assert_eq!(app.textarea().cursor(), 12);
    }

    #[test]
    fn slash_quit_dispatches_without_sending_a_runtime_turn() {
        let mut app = App::new("slash-quit".to_owned());
        app.insert_text("/quit");
        assert!(app.submit_input().is_none());
        assert!(app.should_quit());
    }

    #[test]
    fn file_picker_completes_a_local_at_mention() {
        let mut app = App::new("file-picker".to_owned());
        app.insert_text("@Cargo.toml");
        assert!(
            app.completion_popup_items()
                .iter()
                .any(|(label, _)| label == "@Cargo.toml")
        );
        assert!(
            app.handle_key_event(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Tab,
                crossterm::event::KeyModifiers::NONE,
            ))
            .is_none()
        );
        assert_eq!(app.input(), "@Cargo.toml ");
    }

    #[test]
    fn enter_confirms_the_selected_file_completion_instead_of_submitting() {
        let mut app = App::new("file-picker-enter".to_owned());
        app.insert_text("@Cargo.toml");
        assert!(
            app.handle_key_event(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ))
            .is_none()
        );
        assert_eq!(app.input(), "@Cargo.toml ");
        assert!(app.cells().is_empty());
    }

    #[test]
    fn file_picker_uses_the_navigated_selection() {
        let mut app = App::new("file-picker-selection".to_owned());
        app.insert_text("@Cargo");
        let items = app.completion_popup_items();
        let second = items
            .get(1)
            .map(|(label, _)| label.clone())
            .expect("Cargo query should produce at least two local entries");

        assert!(app.handle_global_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(app.completion_selection(), 1);
        assert!(
            app.handle_key_event(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Tab,
                crossterm::event::KeyModifiers::NONE,
            ))
            .is_none()
        );
        assert_eq!(app.input(), format!("{second} "));
    }

    #[test]
    fn file_picker_escape_does_not_cancel_a_turn() {
        let mut app = App::new("file-picker-escape".to_owned());
        app.insert_text("@Cargo");
        assert!(!app.completion_popup_items().is_empty());
        assert!(app.handle_global_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        )));
        assert!(app.completion_popup_items().is_empty());
        assert!(!app.take_cancel_requested());
        assert_eq!(app.input(), "@Cargo");
    }

    #[test]
    fn ctrl_g_requests_editor_only_when_idle() {
        let mut app = App::new("editor-request".to_owned());
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('g'),
            crossterm::event::KeyModifiers::CONTROL,
        );
        assert!(app.handle_global_key(key));
        assert!(app.take_external_editor_requested());
        assert!(!app.take_external_editor_requested());
    }

    #[test]
    fn ctrl_c_clears_draft_before_quit_confirmation() {
        let mut app = App::new("ctrl-c".to_owned());
        app.insert_text("draft");
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('c'),
            crossterm::event::KeyModifiers::CONTROL,
        );
        assert!(app.handle_global_key(key));
        assert!(app.input().is_empty());
        assert!(!app.quit_confirmation());
    }

    #[test]
    fn manual_scroll_preserves_viewport_when_history_grows() {
        let mut app = App::new("scroll".to_owned());
        app.record_history_content_height(20);
        app.scroll_up(3);
        app.record_history_content_height(25);
        assert_eq!(app.history_scroll(), 8);
    }
}
