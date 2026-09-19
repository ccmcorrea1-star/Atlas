use std::borrow::Cow;

use super::live_output::LiveCommandOutput;

const MAX_STREAM_CHUNKS_BYTES: usize = 1024 * 1024;

#[derive(Debug, Default)]
pub(crate) struct CommandOutput {
    pub(crate) exit_code: i32,
    aggregated_output: String,
    live_output: Option<LiveCommandOutput>,
    stream_chunks: Vec<OutputChunk>,
    stream_chunks_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutputChunk {
    pub(crate) channel: String,
    pub(crate) text: String,
}

impl CommandOutput {
    pub(crate) fn new(exit_code: i32, aggregated_output: String) -> Self {
        Self {
            exit_code,
            aggregated_output,
            live_output: None,
            stream_chunks: Vec::new(),
            stream_chunks_bytes: 0,
        }
    }

    pub(crate) fn line_counts(&self) -> (usize, usize) {
        self.live_output.as_ref().map_or_else(
            || {
                let total = self.aggregated_output.lines().count();
                (total, total)
            },
            LiveCommandOutput::line_counts,
        )
    }

    pub(crate) fn lines(&self) -> Box<dyn DoubleEndedIterator<Item = Cow<'_, str>> + '_> {
        if let Some(output) = &self.live_output {
            output.lines()
        } else {
            Box::new(self.aggregated_output.lines().map(Cow::Borrowed))
        }
    }

    fn has_live_output(&self) -> bool {
        self.live_output.is_some()
    }

    pub(crate) fn transcript_lines(&self) -> Box<dyn Iterator<Item = Cow<'_, str>> + '_> {
        if let Some(output) = &self.live_output {
            output.transcript_lines()
        } else {
            Box::new(self.aggregated_output.lines().map(Cow::Borrowed))
        }
    }

    #[allow(dead_code)]
    pub(crate) fn append_output(&mut self, channel: &str, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        let text = bounded_stream_chunk(chunk);
        self.stream_chunks_bytes = self.stream_chunks_bytes.saturating_add(text.len());
        self.stream_chunks.push(OutputChunk {
            channel: channel.to_owned(),
            text,
        });
        while self.stream_chunks_bytes > MAX_STREAM_CHUNKS_BYTES {
            let removed = self.stream_chunks.remove(0);
            self.stream_chunks_bytes = self.stream_chunks_bytes.saturating_sub(removed.text.len());
        }
        self.live_output
            .get_or_insert_with(LiveCommandOutput::default)
            .push_str(chunk);
    }
}

fn bounded_stream_chunk(chunk: &str) -> String {
    if chunk.len() <= MAX_STREAM_CHUNKS_BYTES {
        return chunk.to_owned();
    }
    let mut start = chunk.len() - MAX_STREAM_CHUNKS_BYTES;
    while !chunk.is_char_boundary(start) {
        start += 1;
    }
    chunk[start..].to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecState {
    Running,
    Ran,
}

#[derive(Debug)]
pub(crate) struct ExecCell {
    execution_id: String,
    program: String,
    args: Vec<String>,
    status: Option<String>,
    output: Option<CommandOutput>,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
    state: ExecState,
}

impl ExecCell {
    pub(crate) fn new(execution_id: String, program: String, args: Vec<String>) -> Self {
        Self {
            execution_id,
            program,
            args,
            status: None,
            output: None,
            exit_code: None,
            duration_ms: None,
            state: ExecState::Running,
        }
    }

    pub(crate) fn execution_id(&self) -> &str {
        &self.execution_id
    }

    pub(crate) fn state(&self) -> ExecState {
        self.state
    }

    pub(crate) fn is_running(&self) -> bool {
        self.state == ExecState::Running
    }

    pub(crate) fn complete(
        &mut self,
        stdout: String,
        stderr: String,
        exit_code: i32,
        duration_ms: u64,
        status: String,
    ) {
        let streamed_output = self
            .output
            .as_ref()
            .is_some_and(CommandOutput::has_live_output);
        if !streamed_output {
            let output = interleave_output(stdout, stderr);
            self.output = Some(CommandOutput::new(exit_code, output));
        }
        self.exit_code = Some(exit_code);
        self.duration_ms = Some(duration_ms);
        self.status = Some(status);
        self.state = ExecState::Ran;
    }

    pub(crate) fn abort(&mut self) {
        if self.state == ExecState::Running {
            self.complete(String::new(), String::new(), 1, 0, "aborted".to_owned());
        }
    }

    #[allow(dead_code)]
    pub(crate) fn append_output(&mut self, chunk: &str) {
        self.output
            .get_or_insert_with(CommandOutput::default)
            .append_output("stdout", chunk);
    }

    pub(crate) fn append_output_channel(&mut self, channel: &str, chunk: &str) {
        self.output
            .get_or_insert_with(CommandOutput::default)
            .append_output(channel, chunk);
    }

    pub(crate) fn output(&self) -> Option<&CommandOutput> {
        self.output.as_ref()
    }

    pub(crate) fn command(&self) -> String {
        crate::markdown::format_process_command(&self.program, &self.args)
    }

    pub(crate) fn activity(&self) -> String {
        crate::capability_names::execution_activity(&self.program, &self.args)
    }

    pub(crate) fn succeeded(&self) -> bool {
        self.exit_code == Some(0)
            && !matches!(self.status.as_deref(), Some("error" | "failed" | "aborted"))
    }

    pub(crate) fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    pub(crate) fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
    }
}

fn interleave_output(stdout: String, stderr: String) -> String {
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout,
        (true, false) => stderr,
        (false, false) => format!("{stdout}{stderr}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamed_chunk_history_is_bounded_without_changing_small_chunks() {
        let mut output = CommandOutput::default();
        output.append_output("stdout", &"x".repeat(MAX_STREAM_CHUNKS_BYTES));
        output.append_output("stderr", "tail");

        assert!(output.stream_chunks_bytes <= MAX_STREAM_CHUNKS_BYTES);
        assert_eq!(
            output.stream_chunks.last().map(|chunk| chunk.text.as_str()),
            Some("tail")
        );
    }

    #[test]
    fn streamed_output_preserves_channel_and_arrival_order() {
        let mut cell = ExecCell::new("exec".to_owned(), "bash".to_owned(), Vec::new());
        cell.append_output_channel("stderr", "warning\n");
        cell.append_output_channel("stdout", "result\n");
        cell.complete(String::new(), String::new(), 0, 1, "success".to_owned());

        let chunks = &cell.output().expect("streamed output").stream_chunks;
        assert_eq!(
            chunks,
            &[
                OutputChunk {
                    channel: "stderr".to_owned(),
                    text: "warning\n".to_owned(),
                },
                OutputChunk {
                    channel: "stdout".to_owned(),
                    text: "result\n".to_owned(),
                },
            ]
        );
        let rendered = cell
            .output()
            .expect("streamed output")
            .lines()
            .map(|line| line.into_owned())
            .collect::<Vec<_>>();
        assert_eq!(rendered, ["warning", "result"]);
    }

    #[test]
    fn aggregate_only_output_documents_stdout_then_stderr_fallback() {
        let mut cell = ExecCell::new("exec".to_owned(), "bash".to_owned(), Vec::new());
        cell.complete(
            "stdout\n".to_owned(),
            "stderr\n".to_owned(),
            0,
            1,
            "success".to_owned(),
        );

        let output = cell.output().expect("aggregate output");
        assert!(output.stream_chunks.is_empty());
        assert_eq!(
            output
                .lines()
                .map(|line| line.into_owned())
                .collect::<Vec<_>>(),
            ["stdout", "stderr"]
        );
    }
}
