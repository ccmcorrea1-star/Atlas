//! Estado de execução do processo recebido pelo Runtime Atlas.

use super::*;

impl ChatWidget {
    pub(super) fn handle_execution_event(&mut self, event: RuntimeEvent) {
        match event {
            RuntimeEvent::ExecutionStarted {
                execution_id,
                capability: _,
                program,
                args,
                cwd: _,
                target: _,
            } => {
                self.finalize_thinking();
                self.status = Status::Executing;
                if self.find_active_exec_mut(&execution_id).is_none() {
                    let exec = ExecCell::new(
                        bounded_metadata(&execution_id),
                        bounded_metadata(&program),
                        args.iter().map(|arg| bounded_metadata(arg)).collect(),
                    );
                    // Validacoes concorrentes compartilham um unico grupo compacto.
                    match self.active_exec_group_mut() {
                        Some(group) => group.push(exec),
                        None => self
                            .active_cells
                            .push(Box::new(RunningGroupCell::from_exec(exec))),
                    }
                }
                self.bump_active_revision();
                self.history_changed();
            }
            RuntimeEvent::ExecutionOutputDelta {
                execution_id,
                capability: _,
                channel,
                delta,
            } => {
                self.status = Status::Executing;
                if let Some(cell) = self.find_active_exec_mut(&execution_id) {
                    cell.append_output_channel(&channel, &delta);
                    self.bump_active_revision();
                    self.history_changed();
                }
            }
            RuntimeEvent::ExecutionCompleted {
                execution_id,
                capability: _,
                stdout,
                stderr,
                exit_code,
                duration_ms,
                status,
            } => {
                if let Some(cell) = self.find_active_exec_mut(&execution_id) {
                    cell.complete(
                        bounded_output(&stdout),
                        bounded_output(&stderr),
                        exit_code,
                        duration_ms,
                        bounded_metadata(&status),
                    );
                    self.commit_active_exec(&execution_id);
                } else if let Some(cell) = self.find_exec_mut(&execution_id) {
                    cell.complete(
                        bounded_output(&stdout),
                        bounded_output(&stderr),
                        exit_code,
                        duration_ms,
                        bounded_metadata(&status),
                    );
                } else {
                    let mut cell =
                        ExecCell::new(bounded_metadata(&execution_id), String::new(), Vec::new());
                    cell.complete(
                        bounded_output(&stdout),
                        bounded_output(&stderr),
                        exit_code,
                        duration_ms,
                        bounded_metadata(&status),
                    );
                    self.cells.push(Box::new(cell));
                }
                self.status = if self.has_running_exec() {
                    Status::Executing
                } else {
                    Status::Thinking
                };
                if !self.has_running_exec() && self.active_running_tool_activities().is_empty() {
                    self.begin_thinking();
                }
                self.history_changed();
            }
            _ => {}
        }
    }
}
