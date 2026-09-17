//! Equivalente do `ChatWidget` do Codex voltado ao protocolo do Atlas.
//!
//! Este modulo mantem as cells do transcript e o ciclo de vida do turno. Ele nao
//! sabe como os eventos chegam; o adaptador do Runtime converte JSON em
//! `RuntimeEvent` antes de chamar este controller.

mod command_lifecycle;
mod exec_state;
pub(crate) mod rendering;
mod streaming;

use std::time::Instant;

use crate::app::Status;
use crate::exec_cell::ExecCell;
use crate::history_cell::AgentMarkdownCell;
use crate::history_cell::AgentMessageCell;
use crate::history_cell::ErrorCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::ToolCell;
use crate::runtime::ContextUsage;
use crate::runtime::RuntimeEvent;

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
    turn_started_at: Option<Instant>,
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
            turn_started_at: None,
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

    pub(crate) fn status(&self) -> &Status {
        &self.status
    }

    pub(crate) fn turn_active(&self) -> bool {
        self.turn_active
    }

    pub(crate) fn context_usage(&self) -> Option<ContextUsage> {
        self.context_usage
    }

    pub(crate) fn working_seconds(&self) -> u64 {
        self.turn_started_at
            .map(|started| started.elapsed().as_secs())
            .unwrap_or(0)
    }

    pub(crate) fn tick(&mut self) {
        if !self.active_cells.is_empty() {
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
            RuntimeEvent::SessionUpdated { .. }
            | RuntimeEvent::TurnStarted
            | RuntimeEvent::ContextUpdated { .. }
            | RuntimeEvent::ToolStarted { .. }
            | RuntimeEvent::ToolCompleted { .. }
            | RuntimeEvent::TurnCompleted { .. }
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
            cell.as_any_mut()
                .downcast_mut::<ExecCell>()
                .filter(|exec| exec.execution_id() == id)
        })
    }

    fn find_tool_mut(&mut self, id: &str) -> Option<&mut ToolCell> {
        self.cells.iter_mut().rev().find_map(|cell| {
            cell.as_any_mut()
                .downcast_mut::<ToolCell>()
                .filter(|tool| tool.tool_id() == id)
        })
    }

    fn find_active_tool_mut(&mut self, id: &str) -> Option<&mut ToolCell> {
        self.active_cells.iter_mut().rev().find_map(|cell| {
            cell.as_any_mut()
                .downcast_mut::<ToolCell>()
                .filter(|tool| tool.tool_id() == id)
        })
    }

    fn commit_active_agent(&mut self, id: &str) {
        if let Some(index) = self.active_cells.iter().rposition(|cell| {
            cell.as_any()
                .downcast_ref::<AgentMessageCell>()
                .is_some_and(|message| message.message_id == id)
        }) {
            let cell = self.active_cells.remove(index);
            if let Some(message) = cell.as_any().downcast_ref::<AgentMessageCell>() {
                self.cells.push(Box::new(AgentMarkdownCell::with_message_id(
                    Some(id.to_owned()),
                    message.markdown_source.clone(),
                )));
            } else {
                self.cells.push(cell);
            }
            self.bump_active_revision();
        }
    }

    fn commit_active_exec(&mut self, id: &str) {
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
        if let Some(index) = self.active_cells.iter().rposition(|cell| {
            cell.as_any()
                .downcast_ref::<ToolCell>()
                .is_some_and(|tool| tool.tool_id() == id)
        }) {
            self.cells.push(self.active_cells.remove(index));
            self.bump_active_revision();
        }
    }

    fn commit_all_active_cells(&mut self) {
        if !self.active_cells.is_empty() {
            self.cells.append(&mut self.active_cells);
            self.bump_active_revision();
        }
    }

    fn bump_active_revision(&mut self) {
        self.active_revision = self.active_revision.wrapping_add(1);
    }

    fn history_changed(&mut self) {
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

fn truncate_delta(delta: &str, current_bytes: usize) -> String {
    let available = 1024
        * 1024usize
            .saturating_sub(current_bytes)
            .saturating_sub(TRUNCATION_MARKER.len());
    if delta.len() <= available {
        return delta.to_owned();
    }
    let mut end = available.min(delta.len());
    while end > 0 && !delta.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &delta[..end], TRUNCATION_MARKER)
}
