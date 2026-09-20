//! Equivalente do `ChatWidget` do Codex voltado ao protocolo do Atlas.
//!
//! Este modulo mantem as cells do transcript e o ciclo de vida do turno. Ele nao
//! sabe como os eventos chegam; o adaptador do Runtime converte JSON em
//! `RuntimeEvent` antes de chamar este controller.

mod command_lifecycle;
mod exec_state;
mod reasoning;
pub(crate) mod rendering;
mod streaming;

use self::streaming::MarkdownStreamState;
use crate::app::Status;
use crate::exec_cell::ExecCell;
use crate::exec_cell::RunningGroupCell;
use crate::history_cell::AgentMarkdownCell;
use crate::history_cell::AgentMessageCell;
use crate::history_cell::CancelledCell;
use crate::history_cell::ErrorCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::ThinkingCell;
use crate::history_cell::ThoughtCell;
use crate::history_cell::ToolCell;
use crate::history_cell::ToolGroupCell;
use crate::runtime::ContextUsage;
use crate::runtime::RuntimeEvent;
use std::collections::HashMap;

const MAX_CELLS: usize = 500;
const MAX_CELL_BYTES: usize = 16 * 1024 * 1024;
const TRUNCATION_MARKER: &str = "\n[output truncated]";

#[derive(Debug)]
pub(crate) struct ChatWidget {
    cells: Vec<Box<dyn HistoryCell>>,
    active_cells: Vec<Box<dyn HistoryCell>>,
    status: Status,
    turn_active: bool,
    context_usage: Option<ContextUsage>,
    active_revision: u64,
    history_revision: u64,
    stream_states: HashMap<String, MarkdownStreamState>,
}

impl ChatWidget {
    pub(crate) fn new() -> Self {
        Self {
            cells: Vec::new(),
            active_cells: Vec::new(),
            status: Status::Ready,
            turn_active: false,
            context_usage: None,
            active_revision: 0,
            history_revision: 0,
            stream_states: HashMap::new(),
        }
    }

    pub(crate) fn cells(&self) -> &[Box<dyn HistoryCell>] {
        &self.cells
    }

    pub(crate) fn active_cells(&self) -> &[Box<dyn HistoryCell>] {
        &self.active_cells
    }

    pub(crate) fn active_revision(&self) -> u64 {
        self.active_revision
    }

    pub(crate) fn history_revision(&self) -> u64 {
        self.history_revision
    }

    #[allow(dead_code)]
    pub(crate) fn status(&self) -> &Status {
        &self.status
    }

    pub(crate) fn turn_active(&self) -> bool {
        self.turn_active
    }

    pub(crate) fn context_usage(&self) -> Option<ContextUsage> {
        self.context_usage
    }

    fn active_running_execution_activities(&self) -> Vec<String> {
        let mut activities = Vec::new();
        for cell in &self.active_cells {
            if let Some(group) = cell.as_any().downcast_ref::<RunningGroupCell>() {
                activities.extend(group.running_activities());
            } else if let Some(exec) = cell.as_any().downcast_ref::<ExecCell>()
                && exec.is_running()
            {
                activities.push(exec.activity());
            }
        }
        activities
    }

    fn active_running_tool_activities(&self) -> Vec<String> {
        self.active_cells
            .iter()
            .flat_map(|cell| {
                if let Some(group) = cell.as_any().downcast_ref::<ToolGroupCell>() {
                    group.running_activities()
                } else if let Some(tool) = cell.as_any().downcast_ref::<ToolCell>() {
                    if tool.is_running() {
                        vec![tool.activity()]
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            })
            .collect()
    }

    fn has_running_exec(&self) -> bool {
        !self.active_running_execution_activities().is_empty()
    }

    pub(crate) fn tick(&mut self) {
        for cell in &mut self.active_cells {
            if let Some(thinking) = cell.as_any_mut().downcast_mut::<ThinkingCell>() {
                thinking.tick();
            }
            if let Some(thought) = cell.as_any_mut().downcast_mut::<ThoughtCell>() {
                thought.tick();
            }
        }
        let committed_parts = self
            .stream_states
            .iter_mut()
            .filter_map(|(message_id, stream)| {
                stream.commit_tick().then(|| {
                    (
                        message_id.clone(),
                        stream.source().to_owned(),
                        stream.stable_len,
                    )
                })
            })
            .collect::<Vec<_>>();
        for (message_id, source, stable_len) in committed_parts {
            let display_source = bounded_text(&source);
            self.materialize_stream_commit(&message_id, &display_source, stable_len);
        }
        if !self.stream_states.is_empty() || !self.active_cells.is_empty() {
            self.active_revision = self.active_revision.wrapping_add(1);
        }
    }

    pub(crate) fn set_stream_width(&mut self, width: u16) {
        let changed = self
            .stream_states
            .values_mut()
            .any(|stream| stream.set_width(width));
        if changed {
            self.active_revision = self.active_revision.wrapping_add(1);
        }
    }

    pub(crate) fn add_user_message(&mut self, message: String) {
        self.cells
            .push(Box::new(crate::history_cell::UserHistoryCell::new(message)));
        self.history_changed();
    }

    pub(crate) fn take_previous_user_message(&mut self) -> Option<String> {
        let index = self
            .cells
            .iter()
            .rposition(|cell| cell.as_any().is::<crate::history_cell::UserHistoryCell>())?;
        let message = self.cells[index]
            .as_any()
            .downcast_ref::<crate::history_cell::UserHistoryCell>()
            .map(|cell| cell.message.clone())?;
        self.cells.remove(index);
        self.history_changed();
        Some(message)
    }

    pub(crate) fn handle_runtime_event(&mut self, event: RuntimeEvent) {
        match &event {
            RuntimeEvent::MessageDelta { .. } | RuntimeEvent::MessageCompleted { .. } => {
                self.handle_streaming_event(event);
            }
            RuntimeEvent::ExecutionStarted { .. }
            | RuntimeEvent::ExecutionOutputDelta { .. }
            | RuntimeEvent::ExecutionCompleted { .. } => {
                self.handle_execution_event(event);
            }
            RuntimeEvent::ReasoningStart { .. }
            | RuntimeEvent::ReasoningDelta { .. }
            | RuntimeEvent::ReasoningEnd { .. } => {
                self.handle_reasoning_event(event);
            }
            RuntimeEvent::SessionUpdated { .. }
            | RuntimeEvent::TurnStarted
            | RuntimeEvent::ContextUpdated { .. }
            | RuntimeEvent::ToolStarted { .. }
            | RuntimeEvent::ToolCompleted { .. }
            | RuntimeEvent::TurnCompleted { .. }
            | RuntimeEvent::TurnCancelled { .. }
            | RuntimeEvent::Error { .. } => {
                self.handle_command_lifecycle_event(event);
            }
        }
    }
    fn find_agent(&self, id: &str) -> Option<&AgentMessageCell> {
        self.cells.iter().rev().find_map(|cell| {
            cell.as_any()
                .downcast_ref::<AgentMessageCell>()
                .filter(|message| message.message_id == id)
        })
    }

    fn find_agent_mut(&mut self, id: &str) -> Option<&mut AgentMessageCell> {
        self.cells.iter_mut().rev().find_map(|cell| {
            cell.as_any_mut()
                .downcast_mut::<AgentMessageCell>()
                .filter(|message| message.message_id == id)
        })
    }

    fn find_markdown_mut(&mut self, id: &str) -> Option<&mut AgentMarkdownCell> {
        self.cells.iter_mut().rev().find_map(|cell| {
            cell.as_any_mut()
                .downcast_mut::<AgentMarkdownCell>()
                .filter(|message| message.message_id.as_deref() == Some(id))
        })
    }

    fn find_active_agent_mut(&mut self, id: &str) -> Option<&mut AgentMessageCell> {
        self.active_cells.iter_mut().rev().find_map(|cell| {
            cell.as_any_mut()
                .downcast_mut::<AgentMessageCell>()
                .filter(|message| message.message_id == id)
        })
    }

    fn find_exec_mut(&mut self, id: &str) -> Option<&mut ExecCell> {
        self.cells.iter_mut().rev().find_map(|cell| {
            cell.as_any_mut()
                .downcast_mut::<ExecCell>()
                .filter(|exec| exec.execution_id() == id)
        })
    }

    fn find_active_exec_mut(&mut self, id: &str) -> Option<&mut ExecCell> {
        self.active_cells.iter_mut().rev().find_map(|cell| {
            let any = cell.as_any_mut();
            if any.is::<ExecCell>() {
                let exec = any.downcast_mut::<ExecCell>().expect("checked ExecCell");
                return (exec.execution_id() == id).then_some(exec);
            }
            if any.is::<RunningGroupCell>() {
                let group = any
                    .downcast_mut::<RunningGroupCell>()
                    .expect("checked RunningGroupCell");
                return group.exec_mut(id);
            }
            None
        })
    }

    fn active_exec_group_mut(&mut self) -> Option<&mut RunningGroupCell> {
        self.active_cells
            .iter_mut()
            .rev()
            .find_map(|cell| cell.as_any_mut().downcast_mut::<RunningGroupCell>())
    }

    fn find_tool_mut(&mut self, id: &str) -> Option<&mut ToolCell> {
        self.cells.iter_mut().rev().find_map(|cell| {
            let any = cell.as_any_mut();
            if any.is::<ToolGroupCell>() {
                return any
                    .downcast_mut::<ToolGroupCell>()
                    .expect("checked tool group")
                    .tool_mut(id);
            }
            any.downcast_mut::<ToolCell>()
                .filter(|tool| tool.tool_id() == id)
        })
    }

    fn find_active_tool_mut(&mut self, id: &str) -> Option<&mut ToolCell> {
        self.active_cells.iter_mut().rev().find_map(|cell| {
            let any = cell.as_any_mut();
            if any.is::<ToolGroupCell>() {
                return any
                    .downcast_mut::<ToolGroupCell>()
                    .expect("checked tool group")
                    .tool_mut(id);
            }
            any.downcast_mut::<ToolCell>()
                .filter(|tool| tool.tool_id() == id)
        })
    }

    fn active_tool_group_mut(&mut self) -> Option<&mut ToolGroupCell> {
        self.active_cells
            .last_mut()
            .and_then(|cell| cell.as_any_mut().downcast_mut::<ToolGroupCell>())
    }

    fn materialize_stream_commit(&mut self, id: &str, source: &str, stable_len: usize) {
        let stable_len = stable_len.min(source.len());
        let stable = &source[..stable_len];
        let tail = &source[stable_len..];
        let positions = self
            .active_cells
            .iter()
            .enumerate()
            .filter_map(|(index, cell)| {
                cell.as_any()
                    .downcast_ref::<AgentMessageCell>()
                    .filter(|message| message.message_id == id)
                    .map(|_| index)
            })
            .collect::<Vec<_>>();
        let Some(&first_index) = positions.first() else {
            return;
        };

        if stable.is_empty() {
            let tail_index = *positions.last().unwrap_or(&first_index);
            if let Some(message) = self.active_cells[tail_index]
                .as_any_mut()
                .downcast_mut::<AgentMessageCell>()
            {
                message.set_stream_parts(tail, 0);
                message.markdown_source = source.to_owned();
            }
            return;
        }

        if let Some(message) = self.active_cells[first_index]
            .as_any_mut()
            .downcast_mut::<AgentMessageCell>()
        {
            message.set_stream_parts(stable, stable.len());
        }

        if tail.is_empty() {
            for &index in positions.iter().skip(1).rev() {
                self.active_cells.remove(index);
            }
            return;
        }

        let tail_cell = {
            let mut cell = AgentMessageCell::new(id.to_owned(), tail, false);
            cell.set_stream_parts(tail, 0);
            cell.markdown_source = source.to_owned();
            Box::new(cell) as Box<dyn HistoryCell>
        };
        if let Some(&tail_index) = positions.last() {
            if tail_index == first_index {
                self.active_cells.insert(first_index + 1, tail_cell);
            } else {
                self.active_cells[tail_index] = tail_cell;
                for &index in positions
                    .iter()
                    .skip(1)
                    .take(positions.len().saturating_sub(2))
                    .rev()
                {
                    self.active_cells.remove(index);
                }
            }
        }
    }

    fn commit_active_agent(&mut self, id: &str) {
        let positions = self
            .active_cells
            .iter()
            .enumerate()
            .filter_map(|(index, cell)| {
                cell.as_any()
                    .downcast_ref::<AgentMessageCell>()
                    .filter(|message| message.message_id == id)
                    .map(|message| (index, message.markdown_source.clone()))
            })
            .collect::<Vec<_>>();
        let Some((_, content)) = positions.last() else {
            return;
        };
        for (index, _) in positions.iter().rev() {
            self.active_cells.remove(*index);
        }
        self.cells.push(Box::new(AgentMarkdownCell::with_message_id(
            Some(id.to_owned()),
            content.clone(),
        )));
        self.bump_active_revision();
    }

    fn commit_active_exec(&mut self, id: &str) {
        // O grupo so vai para o historico quando todas as execucoes terminam.
        let completed_group = self
            .active_cells
            .iter_mut()
            .enumerate()
            .find_map(|(index, cell)| {
                let group = cell.as_any_mut().downcast_mut::<RunningGroupCell>()?;
                group.exec_mut(id)?;
                group.all_completed().then_some(index)
            });
        if let Some(index) = completed_group {
            self.cells.push(self.active_cells.remove(index));
            self.bump_active_revision();
            return;
        }
        if let Some(index) = self.active_cells.iter().rposition(|cell| {
            cell.as_any()
                .downcast_ref::<ExecCell>()
                .is_some_and(|exec| exec.execution_id() == id)
        }) {
            self.cells.push(self.active_cells.remove(index));
            self.bump_active_revision();
        }
    }

    fn commit_active_tool(&mut self, id: &str) {
        let completed_group = self
            .active_cells
            .iter()
            .enumerate()
            .find_map(|(index, cell)| {
                let group = cell.as_any().downcast_ref::<ToolGroupCell>()?;
                (group.contains_tool(id) && group.all_completed()).then_some(index)
            });
        if let Some(index) = completed_group {
            let group = self.active_cells.remove(index);
            self.append_tool_group(group);
            self.bump_active_revision();
            return;
        }
        if let Some(index) = self.active_cells.iter().rposition(|cell| {
            cell.as_any()
                .downcast_ref::<ToolCell>()
                .is_some_and(|tool| tool.tool_id() == id)
        }) {
            self.cells.push(self.active_cells.remove(index));
            self.bump_active_revision();
        }
    }

    fn append_tool_group(&mut self, cell: Box<dyn HistoryCell>) {
        let Some(group) = cell.as_any().downcast_ref::<ToolGroupCell>() else {
            self.cells.push(cell);
            return;
        };
        let previous_index = self
            .cells
            .iter()
            .rposition(|candidate| candidate.as_any().downcast_ref::<ToolGroupCell>().is_some());
        if let Some(previous_index) = previous_index
            && self.cells[previous_index + 1..]
                .iter()
                .all(|candidate| candidate.as_any().is::<ThinkingCell>())
            && self.cells[previous_index]
                .as_any()
                .downcast_ref::<ToolGroupCell>()
                .is_some_and(|previous| previous.can_merge(group))
        {
            let previous = self.cells[previous_index]
                .as_any_mut()
                .downcast_mut::<ToolGroupCell>()
                .expect("tool group was checked");
            previous.merge(group.clone());
            return;
        }
        self.cells.push(cell);
    }

    fn commit_all_active_cells(&mut self) {
        if !self.active_cells.is_empty() {
            self.cells.append(&mut self.active_cells);
            self.bump_active_revision();
        }
    }

    fn finalize_thinking(&mut self) {
        let positions = self
            .active_cells
            .iter()
            .enumerate()
            .filter_map(|(index, cell)| cell.as_any().is::<ThinkingCell>().then_some(index))
            .collect::<Vec<_>>();
        if positions.is_empty() {
            return;
        }
        for index in positions.into_iter().rev() {
            let mut cell = self.active_cells.remove(index);
            if let Some(thinking) = cell.as_any_mut().downcast_mut::<ThinkingCell>() {
                thinking.finish();
            }
            self.cells.push(cell);
        }
        self.bump_active_revision();
        self.history_changed();
    }

    /// Descarta o Thinking generico sem commitar.
    ///
    /// O reasoning estruturado assume a vez: manter o placeholder na timeline
    /// duplicaria o mesmo estado em duas celulas.
    fn discard_thinking(&mut self) {
        if !self
            .active_cells
            .iter()
            .any(|cell| cell.as_any().is::<ThinkingCell>())
        {
            return;
        }
        self.active_cells
            .retain(|cell| !cell.as_any().is::<ThinkingCell>());
        self.bump_active_revision();
    }

    pub(crate) fn toggle_thought(&mut self, active: bool, index: usize) -> bool {
        let cells = if active {
            &mut self.active_cells
        } else {
            &mut self.cells
        };
        let Some(cell) = cells.get_mut(index) else {
            return false;
        };
        let Some(thought) = cell.as_any_mut().downcast_mut::<ThoughtCell>() else {
            return false;
        };
        thought.toggle();
        if active {
            self.bump_active_revision();
        } else {
            self.history_changed();
        }
        true
    }

    fn begin_thinking(&mut self) {
        if self
            .active_cells
            .iter()
            .any(|cell| cell.as_any().is::<ThinkingCell>())
        {
            return;
        }
        self.active_cells.push(Box::new(ThinkingCell::new()));
        self.status = Status::Thinking;
        self.bump_active_revision();
    }

    fn cancel_active_cells(&mut self) {
        for cell in &mut self.active_cells {
            command_lifecycle::abort_exec_cell(cell.as_mut());
            command_lifecycle::abort_tool_cell(cell.as_mut());
        }
    }

    fn bump_active_revision(&mut self) {
        self.active_revision = self.active_revision.wrapping_add(1);
    }

    fn history_changed(&mut self) {
        self.history_revision = self.history_revision.wrapping_add(1);
        while self.cells.len() > MAX_CELLS || self.cell_bytes() > MAX_CELL_BYTES {
            if self.cells.len() <= 1 {
                break;
            }
            self.cells.remove(0);
        }
    }

    fn cell_bytes(&self) -> usize {
        self.cells
            .iter()
            .chain(self.active_cells.iter())
            .map(|cell| {
                cell.display_lines(u16::MAX)
                    .iter()
                    .map(|line| line.width())
                    .sum::<usize>()
            })
            .sum()
    }
}

fn bounded_text(text: &str) -> String {
    truncate_text(text, 1024 * 1024)
}

fn bounded_output(text: &str) -> String {
    truncate_text(text, 128 * 1024)
}

fn bounded_metadata(text: &str) -> String {
    truncate_text(text, 8 * 1024)
}

fn truncate_text(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes
        .saturating_sub(TRUNCATION_MARKER.len())
        .min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &text[..end], TRUNCATION_MARKER)
}
