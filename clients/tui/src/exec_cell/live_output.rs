use std::borrow::Cow;
use std::collections::VecDeque;

const LIVE_COMMAND_OUTPUT_MAX_BYTES: usize = 1024 * 1024;
const LIVE_COMMAND_OUTPUT_MAX_LINES: usize = 50;
const LIVE_COMMAND_OUTPUT_MAX_LINE_BYTES: usize =
    LIVE_COMMAND_OUTPUT_MAX_BYTES / (2 * LIVE_COMMAND_OUTPUT_MAX_LINES + 2);
const LIVE_COMMAND_OUTPUT_LINE_HEAD_BYTES: usize = LIVE_COMMAND_OUTPUT_MAX_LINE_BYTES / 2;
const LIVE_COMMAND_OUTPUT_LINE_TAIL_BYTES: usize =
    LIVE_COMMAND_OUTPUT_MAX_LINE_BYTES - LIVE_COMMAND_OUTPUT_LINE_HEAD_BYTES;

/// Saida limitada conforme as regras de preview e transcript do Codex.
#[derive(Debug, Default)]
pub(crate) struct LiveCommandOutput {
    full_output: String,
    truncated: bool,
    head: Vec<String>,
    tail: VecDeque<String>,
    current: LiveCommandOutputLine,
    completed_lines: usize,
    has_partial_line: bool,
    pending_carriage_return: bool,
}

impl LiveCommandOutput {
    pub(crate) fn push_str(&mut self, chunk: &str) {
        if !self.truncated {
            if self.full_output.len().saturating_add(chunk.len()) <= LIVE_COMMAND_OUTPUT_MAX_BYTES {
                self.full_output.push_str(chunk);
                self.completed_lines = self
                    .completed_lines
                    .saturating_add(chunk.bytes().filter(|byte| *byte == b'\n').count());
                if !chunk.is_empty() {
                    self.has_partial_line = !chunk.ends_with('\n');
                }
                return;
            }

            self.truncated = true;
            let full_output = std::mem::take(&mut self.full_output);
            self.completed_lines = 0;
            self.has_partial_line = false;
            self.push_truncated_str(&full_output);
        }

        self.push_truncated_str(chunk);
    }

    fn push_truncated_str(&mut self, chunk: &str) {
        for part in chunk.split_inclusive('\n') {
            let Some(part) = part.strip_suffix('\n') else {
                if part.is_empty() {
                    continue;
                }
                if self.pending_carriage_return {
                    self.current.push_str("\r");
                    self.pending_carriage_return = false;
                }
                let part = if let Some(part) = part.strip_suffix('\r') {
                    self.pending_carriage_return = true;
                    part
                } else {
                    part
                };
                self.current.push_str(part);
                self.has_partial_line |= !part.is_empty() || self.pending_carriage_return;
                continue;
            };

            let has_carriage_return = part.ends_with('\r');
            let part = part.strip_suffix('\r').unwrap_or(part);
            if self.pending_carriage_return && (has_carriage_return || !part.is_empty()) {
                self.current.push_str("\r");
            }
            self.pending_carriage_return = false;
            self.current.push_str(part);
            self.completed_lines = self.completed_lines.saturating_add(1);
            self.has_partial_line = false;

            let line = std::mem::take(&mut self.current).render();
            if self.head.len() < LIVE_COMMAND_OUTPUT_MAX_LINES {
                self.head.push(line);
            } else {
                if self.tail.len() == LIVE_COMMAND_OUTPUT_MAX_LINES {
                    self.tail.pop_front();
                }
                self.tail.push_back(line);
            }
        }
    }

    pub(crate) fn line_counts(&self) -> (usize, usize) {
        let total = self.completed_lines + usize::from(self.has_partial_line);
        let retained = if self.truncated {
            self.head.len() + self.tail.len() + usize::from(self.has_partial_line)
        } else {
            total
        };
        (total, retained)
    }

    pub(crate) fn lines(&self) -> Box<dyn DoubleEndedIterator<Item = Cow<'_, str>> + '_> {
        if self.truncated {
            Box::new(
                self.head
                    .iter()
                    .chain(self.tail.iter())
                    .map(|line| Cow::Borrowed(line.as_str()))
                    .chain(
                        self.has_partial_line
                            .then(|| Cow::Owned(self.render_partial_line())),
                    ),
            )
        } else {
            Box::new(self.full_output.lines().map(|line| {
                if line.len() <= LIVE_COMMAND_OUTPUT_MAX_LINE_BYTES {
                    Cow::Borrowed(line)
                } else {
                    let mut truncated = LiveCommandOutputLine::default();
                    truncated.push_str(line);
                    Cow::Owned(truncated.render())
                }
            }))
        }
    }

    pub(crate) fn transcript_lines(&self) -> Box<dyn Iterator<Item = Cow<'_, str>> + '_> {
        let (total, retained) = self.line_counts();
        let omitted = total.saturating_sub(retained);
        if self.truncated {
            Box::new(
                self.head
                    .iter()
                    .map(|line| Cow::Borrowed(line.as_str()))
                    .chain((omitted > 0).then(|| Cow::Owned(format!("… +{omitted} lines"))))
                    .chain(self.tail.iter().map(|line| Cow::Borrowed(line.as_str())))
                    .chain(
                        self.has_partial_line
                            .then(|| Cow::Owned(self.render_partial_line())),
                    ),
            )
        } else {
            Box::new(self.full_output.lines().map(Cow::Borrowed))
        }
    }

    fn render_partial_line(&self) -> String {
        let mut line = self.current.render();
        if self.pending_carriage_return {
            line.push('\r');
        }
        line
    }
}

#[derive(Debug, Default)]
struct LiveCommandOutputLine {
    head: String,
    tail: String,
    omitted_bytes: usize,
}

impl LiveCommandOutputLine {
    fn push_str(&mut self, chunk: &str) {
        let head_remaining = if self.tail.is_empty() && self.omitted_bytes == 0 {
            LIVE_COMMAND_OUTPUT_LINE_HEAD_BYTES.saturating_sub(self.head.len())
        } else {
            0
        };
        let mut head_end = head_remaining.min(chunk.len());
        while !chunk.is_char_boundary(head_end) {
            head_end -= 1;
        }
        self.head.push_str(&chunk[..head_end]);
        let chunk = &chunk[head_end..];
        if chunk.is_empty() {
            return;
        }

        if chunk.len() >= LIVE_COMMAND_OUTPUT_LINE_TAIL_BYTES {
            let mut tail_start = chunk.len() - LIVE_COMMAND_OUTPUT_LINE_TAIL_BYTES;
            while !chunk.is_char_boundary(tail_start) {
                tail_start += 1;
            }
            self.omitted_bytes = self
                .omitted_bytes
                .saturating_add(self.tail.len())
                .saturating_add(tail_start);
            self.tail.clear();
            self.tail.push_str(&chunk[tail_start..]);
            return;
        }

        self.tail.push_str(chunk);
        let mut tail_start = self
            .tail
            .len()
            .saturating_sub(LIVE_COMMAND_OUTPUT_LINE_TAIL_BYTES);
        while !self.tail.is_char_boundary(tail_start) {
            tail_start += 1;
        }
        if tail_start > 0 {
            self.tail.drain(..tail_start);
            self.omitted_bytes = self.omitted_bytes.saturating_add(tail_start);
        }
    }

    fn render(&self) -> String {
        let omission_marker = (self.omitted_bytes > 0)
            .then(|| format!("... {} bytes omitted ...", self.omitted_bytes));
        let mut line = String::with_capacity(
            self.head
                .len()
                .saturating_add(self.tail.len())
                .saturating_add(omission_marker.as_ref().map_or(0, String::len))
                .saturating_add(1),
        );
        line.push_str(&self.head);
        if let Some(omission_marker) = omission_marker {
            let terminator = self.head.rfind('\x1b').and_then(|start| {
                let escape = &self.head.as_bytes()[start + 1..];
                match escape.first() {
                    Some(b']') if !escape.contains(&b'\x07') => Some('\x07'),
                    Some(b'[') if !escape[1..].iter().any(u8::is_ascii_alphabetic) => Some('m'),
                    _ => None,
                }
            });
            if let Some(terminator) = terminator {
                line.push(terminator);
            }
            line.push_str("\x1b[0m");
            line.push_str(&omission_marker);
        }
        line.push_str(&self.tail);
        line
    }
}

#[cfg(test)]
mod tests {
    use super::LiveCommandOutput;

    #[test]
    fn preserves_partial_lines_across_chunks() {
        let mut output = LiveCommandOutput::default();
        output.push_str("one\n");
        output.push_str("two");

        assert_eq!(output.line_counts(), (2, 2));
        let lines = output
            .lines()
            .map(|line| line.into_owned())
            .collect::<Vec<_>>();
        assert_eq!(lines, ["one", "two"]);
    }

    #[test]
    fn transcript_includes_omitted_line_marker_after_truncation() {
        let mut output = LiveCommandOutput::default();
        for index in 0..120 {
            output.push_str(&format!("line-{index}-{}\n", "x".repeat(12_000)));
        }

        let (total, retained) = output.line_counts();
        assert!(total > retained);
        let lines = output
            .transcript_lines()
            .map(|line| line.into_owned())
            .collect::<Vec<_>>();
        assert!(lines.iter().any(|line| line.starts_with("… +")));
        assert!(lines.iter().any(|line| line.starts_with("line-0-")));
        assert!(lines.iter().any(|line| line.starts_with("line-119-")));
    }
}
