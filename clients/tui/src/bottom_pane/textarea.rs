//! Buffer editavel do composer adaptado do `TextArea` da TUI do Codex.
//!
//! O Runtime permanece agnostico ao provider; este tipo cuida apenas do texto,
//! movimento do cursor, quebra visual e buffer de descarte dos atalhos.

use std::cell::Cell;
use std::ops::Range;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use unicode_segmentation::UnicodeSegmentation;

use crate::wrapping::cursor_position;
use crate::wrapping::display_width;
use crate::wrapping::position_at_display_column;
use crate::wrapping::wrap_text;

const MAX_INPUT_BYTES: usize = 64 * 1024;
const WORD_SEPARATORS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";

fn is_word_separator(ch: char) -> bool {
    WORD_SEPARATORS.contains(ch)
}

fn split_word_pieces(run: &str) -> Vec<(usize, &str)> {
    let mut pieces = Vec::new();
    for (segment_start, segment) in run.split_word_bound_indices() {
        let mut piece_start = 0;
        let mut chars = segment.char_indices();
        let Some((_, first_char)) = chars.next() else {
            continue;
        };
        let mut in_separator = is_word_separator(first_char);
        for (index, character) in chars {
            let separator = is_word_separator(character);
            if separator == in_separator {
                continue;
            }
            pieces.push((segment_start + piece_start, &segment[piece_start..index]));
            piece_start = index;
            in_separator = separator;
        }
        pieces.push((segment_start + piece_start, &segment[piece_start..]));
    }
    pieces
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TextAreaState {
    /// Indice da primeira linha quebrada visivel no viewport do textarea.
    pub(crate) scroll: u16,
}

#[derive(Debug, Default)]
pub(crate) struct TextArea {
    text: String,
    cursor_pos: usize,
    preferred_col: Option<usize>,
    kill_buffer: String,
    viewport_scroll: Cell<u16>,
}

impl TextArea {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cursor(&self) -> usize {
        self.cursor_pos
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub(crate) fn set_text_clearing_elements(&mut self, text: &str) {
        self.text.clear();
        self.cursor_pos = 0;
        self.preferred_col = None;
        self.insert_str(text);
        self.cursor_pos = self.text.len();
    }

    pub(crate) fn insert_str(&mut self, text: &str) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let available = MAX_INPUT_BYTES.saturating_sub(self.text.len());
        let mut end = normalized.len().min(available);
        while end > 0 && !normalized.is_char_boundary(end) {
            end -= 1;
        }
        if end == 0 {
            return;
        }
        self.replace_range(self.cursor_pos..self.cursor_pos, &normalized[..end]);
    }

    pub(crate) fn replace_range(&mut self, range: Range<usize>, replacement: &str) {
        let start = self.clamp_boundary(range.start.min(self.text.len()));
        let end = self.clamp_boundary(range.end.min(self.text.len()));
        if start > end {
            return;
        }
        let available = MAX_INPUT_BYTES.saturating_sub(self.text.len().saturating_sub(end - start));
        let mut replacement_end = replacement.len().min(available);
        while replacement_end > 0 && !replacement.is_char_boundary(replacement_end) {
            replacement_end -= 1;
        }
        self.text
            .replace_range(start..end, &replacement[..replacement_end]);
        self.cursor_pos = start + replacement_end;
        self.preferred_col = None;
    }

    pub(crate) fn delete_backward(&mut self) {
        let Some((start, _)) = self.text[..self.cursor_pos]
            .grapheme_indices(true)
            .next_back()
        else {
            return;
        };
        self.replace_range(start..self.cursor_pos, "");
    }

    pub(crate) fn delete_forward(&mut self) {
        let Some(grapheme) = self.text[self.cursor_pos..].graphemes(true).next() else {
            return;
        };
        self.replace_range(self.cursor_pos..self.cursor_pos + grapheme.len(), "");
    }

    pub(crate) fn delete_backward_word(&mut self) {
        let end = self.cursor_pos;
        let start = self.beginning_of_previous_word();
        if start < end {
            self.kill_buffer = self.text[start..end].to_owned();
            self.replace_range(start..end, "");
        }
    }

    pub(crate) fn delete_forward_word(&mut self) {
        let start = self.cursor_pos;
        let end = self.end_of_next_word();
        if start < end {
            self.kill_buffer = self.text[start..end].to_owned();
            self.replace_range(start..end, "");
        }
    }

    pub(crate) fn kill_line_end(&mut self) {
        let end = self.text[self.cursor_pos..]
            .find('\n')
            .map_or(self.text.len(), |offset| self.cursor_pos + offset);
        if self.cursor_pos < end {
            self.kill_buffer = self.text[self.cursor_pos..end].to_owned();
            self.replace_range(self.cursor_pos..end, "");
        } else if end < self.text.len() {
            self.kill_buffer = "\n".to_owned();
            self.replace_range(end..end + 1, "");
        }
    }

    pub(crate) fn kill_line_start(&mut self) {
        let start = self.text[..self.cursor_pos]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        if start < self.cursor_pos {
            self.kill_buffer = self.text[start..self.cursor_pos].to_owned();
            self.replace_range(start..self.cursor_pos, "");
        }
    }

    pub(crate) fn yank(&mut self) {
        let kill = self.kill_buffer.clone();
        if !kill.is_empty() {
            self.insert_str(&kill);
        }
    }

    pub(crate) fn move_cursor_left(&mut self) {
        if let Some((start, _)) = self.text[..self.cursor_pos]
            .grapheme_indices(true)
            .next_back()
        {
            self.cursor_pos = start;
        }
        self.preferred_col = None;
    }

    pub(crate) fn move_cursor_right(&mut self) {
        if let Some(grapheme) = self.text[self.cursor_pos..].graphemes(true).next() {
            self.cursor_pos += grapheme.len();
        }
        self.preferred_col = None;
    }

    pub(crate) fn move_cursor_to_beginning_of_line(&mut self) {
        self.cursor_pos = self.text[..self.cursor_pos]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        self.preferred_col = None;
    }

    pub(crate) fn move_cursor_to_end_of_line(&mut self) {
        self.cursor_pos = self.text[self.cursor_pos..]
            .find('\n')
            .map_or(self.text.len(), |index| self.cursor_pos + index);
        self.preferred_col = None;
    }

    pub(crate) fn move_cursor_up(&mut self) {
        let current_start = self.beginning_of_line(self.cursor_pos);
        if current_start == 0 {
            return;
        }
        let previous_end = current_start - 1;
        let previous_start = self.beginning_of_line(previous_end);
        let target = self
            .preferred_col
            .unwrap_or_else(|| display_width(&self.text[current_start..self.cursor_pos]));
        self.cursor_pos = position_at_display_column(
            &self.text[previous_start..previous_end],
            previous_start,
            target,
        );
        self.preferred_col = Some(target);
    }

    pub(crate) fn move_cursor_down(&mut self) {
        let current_end = self.end_of_line(self.cursor_pos);
        if current_end == self.text.len() {
            return;
        }
        let next_start = current_end + 1;
        let next_end = self.end_of_line(next_start);
        let current_start = self.beginning_of_line(self.cursor_pos);
        let target = self
            .preferred_col
            .unwrap_or_else(|| display_width(&self.text[current_start..self.cursor_pos]));
        self.cursor_pos =
            position_at_display_column(&self.text[next_start..next_end], next_start, target);
        self.preferred_col = Some(target);
    }

    pub(crate) fn input(&mut self, event: KeyEvent) {
        if !matches!(event.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return;
        }
        let control = event.modifiers.contains(KeyModifiers::CONTROL);
        let alt = event.modifiers.contains(KeyModifiers::ALT);
        match event.code {
            KeyCode::Char('j') if control => self.insert_str("\n"),
            KeyCode::Char('k') if control => self.kill_line_end(),
            KeyCode::Char('u') if control => self.kill_line_start(),
            KeyCode::Char('w') if control => self.delete_backward_word(),
            KeyCode::Char('y') if control => self.yank(),
            KeyCode::Char('d') if alt => self.delete_forward_word(),
            KeyCode::Char('a') if control => self.move_cursor_to_beginning_of_line(),
            KeyCode::Char('e') if control => self.move_cursor_to_end_of_line(),
            KeyCode::Char('b') if control => self.move_cursor_left(),
            KeyCode::Char('f') if control => self.move_cursor_right(),
            KeyCode::Char('p') if control => self.move_cursor_up(),
            KeyCode::Char('n') if control => self.move_cursor_down(),
            KeyCode::Backspace => self.delete_backward(),
            KeyCode::Delete => self.delete_forward(),
            KeyCode::Left => self.move_cursor_left(),
            KeyCode::Right => self.move_cursor_right(),
            KeyCode::Up => self.move_cursor_up(),
            KeyCode::Down => self.move_cursor_down(),
            KeyCode::Home => self.move_cursor_to_beginning_of_line(),
            KeyCode::End => self.move_cursor_to_end_of_line(),
            KeyCode::Char(character)
                if (!control && !alt || control && alt) && !character.is_control() =>
            {
                self.insert_str(&character.to_string());
            }
            _ => {}
        }
    }

    pub(crate) fn desired_height(&self, width: u16) -> u16 {
        wrap_text(&self.text, usize::from(width).max(1))
            .len()
            .try_into()
            .unwrap_or(u16::MAX)
    }

    pub(crate) fn state_for_viewport(&self, area: Rect) -> TextAreaState {
        if area.is_empty() {
            return TextAreaState::default();
        }
        let (row, _) = cursor_position(&self.text, self.cursor_pos, usize::from(area.width).max(1));
        let height = usize::from(area.height.max(1));
        let max_scroll = row.saturating_sub(height.saturating_sub(1));
        let current = usize::from(self.viewport_scroll.get());
        let next = if row < current {
            row
        } else if row >= current.saturating_add(height) {
            max_scroll
        } else {
            current.min(max_scroll)
        };
        let scroll = u16::try_from(next).unwrap_or(u16::MAX);
        self.viewport_scroll.set(scroll);
        TextAreaState { scroll }
    }

    pub(crate) fn cursor_pos_with_state(
        &self,
        area: Rect,
        state: TextAreaState,
    ) -> Option<(u16, u16)> {
        if area.is_empty() {
            return None;
        }
        let (row, column) =
            cursor_position(&self.text, self.cursor_pos, usize::from(area.width).max(1));
        let row = row.saturating_sub(usize::from(state.scroll));
        if row >= usize::from(area.height) {
            return None;
        }
        Some((
            area.x + u16::try_from(column).unwrap_or(u16::MAX),
            area.y + u16::try_from(row).unwrap_or(u16::MAX),
        ))
    }

    pub(crate) fn render(&self, area: Rect, buffer: &mut Buffer, state: TextAreaState) {
        if area.is_empty() {
            return;
        }
        let lines = wrap_text(&self.text, usize::from(area.width).max(1))
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>();
        Paragraph::new(lines)
            .style(Style::default())
            .scroll((state.scroll, 0))
            .render(area, buffer);
    }

    fn beginning_of_previous_word(&self) -> usize {
        let prefix = &self.text[..self.cursor_pos];
        let Some((first_non_ws, character)) = prefix
            .char_indices()
            .rev()
            .find(|&(_, character)| !character.is_whitespace())
        else {
            return 0;
        };
        let run_start = prefix[..first_non_ws]
            .char_indices()
            .rev()
            .find(|&(_, character)| character.is_whitespace())
            .map_or(0, |(index, character)| index + character.len_utf8());
        let run_end = first_non_ws + character.len_utf8();
        let mut pieces = split_word_pieces(&prefix[run_start..run_end])
            .into_iter()
            .rev()
            .peekable();
        let Some((piece_start, piece)) = pieces.next() else {
            return run_start;
        };
        let mut start = run_start + piece_start;
        if piece.chars().all(is_word_separator) {
            while let Some((index, piece)) = pieces.peek() {
                if !piece.chars().all(is_word_separator) {
                    break;
                }
                start = run_start + *index;
                pieces.next();
            }
        }
        start
    }

    fn end_of_next_word(&self) -> usize {
        let suffix = &self.text[self.cursor_pos..];
        let Some(first_non_ws) = suffix.find(|character: char| !character.is_whitespace()) else {
            return self.text.len();
        };
        let run = &suffix[first_non_ws..];
        let run = &run[..run.find(char::is_whitespace).unwrap_or(run.len())];
        let mut pieces = split_word_pieces(run).into_iter().peekable();
        let Some((start, piece)) = pieces.next() else {
            return self.cursor_pos + first_non_ws;
        };
        let word_start = self.cursor_pos + first_non_ws + start;
        let mut end = word_start + piece.len();
        if piece.chars().all(is_word_separator) {
            while let Some((index, piece)) = pieces.peek() {
                if !piece.chars().all(is_word_separator) {
                    break;
                }
                end = self.cursor_pos + first_non_ws + *index + piece.len();
                pieces.next();
            }
        }
        end
    }

    fn beginning_of_line(&self, position: usize) -> usize {
        self.text[..position]
            .rfind('\n')
            .map_or(0, |index| index + 1)
    }

    fn end_of_line(&self, position: usize) -> usize {
        self.text[position..]
            .find('\n')
            .map_or(self.text.len(), |index| position + index)
    }

    fn clamp_boundary(&self, position: usize) -> usize {
        if self.text.is_char_boundary(position) {
            position
        } else {
            position.saturating_sub(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TextArea;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    #[test]
    fn edits_graphemes_instead_of_utf8_bytes() {
        let mut area = TextArea::new();
        area.insert_str("e\u{301}");
        area.delete_backward();
        assert!(area.is_empty());
    }

    #[test]
    fn inserts_altgr_characters_instead_of_treating_them_as_control_input() {
        let mut area = TextArea::new();
        area.input(KeyEvent::new(
            KeyCode::Char('@'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        ));
        assert_eq!(area.text(), "@");
    }

    #[test]
    fn word_deletion_follows_codex_separator_and_unicode_boundaries() {
        let mut area = TextArea::new();
        area.insert_str("alpha::βeta");
        area.delete_backward_word();
        assert_eq!(area.text(), "alpha::");
        area.delete_backward_word();
        assert_eq!(area.text(), "alpha");

        let mut forward = TextArea::new();
        forward.insert_str("::βeta");
        for _ in 0..6 {
            forward.move_cursor_left();
        }
        forward.delete_forward_word();
        assert_eq!(forward.text(), "βeta");
    }

    #[test]
    fn kill_and_yank_follow_codex_single_entry_behavior() {
        let mut area = TextArea::new();
        area.insert_str("alpha beta");
        area.move_cursor_to_beginning_of_line();
        area.kill_line_end();
        assert!(area.is_empty());
        area.yank();
        assert_eq!(area.text(), "alpha beta");
    }

    #[test]
    fn vertical_navigation_preserves_display_column() {
        let mut area = TextArea::new();
        area.insert_str("12345\n12\n12345");
        area.move_cursor_to_beginning_of_line();
        area.move_cursor_right();
        area.move_cursor_right();
        area.move_cursor_down();
        assert_eq!(area.cursor(), 11);
        area.move_cursor_down();
        assert_eq!(area.cursor(), 11);
    }

    #[test]
    fn emacs_vertical_navigation_uses_control_p_and_control_n() {
        let mut area = TextArea::new();
        area.insert_str("12345\n12\n12345");
        area.move_cursor_to_beginning_of_line();
        area.move_cursor_right();
        area.move_cursor_right();

        area.input(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        assert_eq!(area.cursor(), 8);
        area.input(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        assert_eq!(area.cursor(), 11);
    }

    #[test]
    fn viewport_scroll_persists_until_cursor_leaves_the_visible_window() {
        let mut area = TextArea::new();
        area.insert_str("a\na\na");
        let viewport = ratatui::layout::Rect::new(0, 0, 4, 2);

        assert_eq!(area.state_for_viewport(viewport).scroll, 1);
        assert_eq!(area.state_for_viewport(viewport).scroll, 1);
        area.move_cursor_up();
        area.move_cursor_up();
        assert_eq!(area.state_for_viewport(viewport).scroll, 0);
    }
}
