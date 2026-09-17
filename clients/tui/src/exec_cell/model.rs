use std::borrow::Cow;

use super::live_output::LiveCommandOutput;

#[derive(Debug, Default)]
pub(crate) struct CommandOutput {
    pub(crate) exit_code: i32,
    aggregated_output: String,
    live_output: Option<LiveCommandOutput>,
}

impl CommandOutput {
    pub(crate) fn new(exit_code: i32, aggregated_output: String) -> Self {
        Self {
            exit_code,
            aggregated_output,
            live_output: None,
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
    pub(crate) fn append_output(&mut self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        self.live_output
            .get_or_insert_with(LiveCommandOutput::default)
            .push_str(chunk);
    }
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
            .append_output(chunk);
    }

    pub(crate) fn output(&self) -> Option<&CommandOutput> {
        self.output.as_ref()
    }

    pub(crate) fn command(&self) -> String {
        crate::markdown::format_process_command(&self.program, &self.args)
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
