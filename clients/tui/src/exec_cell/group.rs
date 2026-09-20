//! Agrupamento compacto de execucoes independentes em andamento.
//!
//! Varias validacoes podem rodar em paralelo. Em vez de repetir uma linha
//! `Running ...` por execucao, o grupo mostra um unico cabecalho com os
//! comandos em arvore.

use ratatui::text::Line;
use ratatui::text::Span;

use super::model::ExecCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::plain_lines;
use crate::ui_consts::error_style;
use crate::ui_consts::running_style;
use crate::ui_consts::secondary_style;
use crate::ui_consts::success_style;

const MAX_GROUP_COMMANDS: usize = 8;

#[derive(Debug)]
pub(crate) struct RunningGroupCell {
    execs: Vec<ExecCell>,
}

impl RunningGroupCell {
    pub(crate) fn from_exec(exec: ExecCell) -> Self {
        Self { execs: vec![exec] }
    }

    pub(crate) fn push(&mut self, exec: ExecCell) {
        self.execs.push(exec);
    }

    pub(crate) fn all_completed(&self) -> bool {
        !self.execs.is_empty() && self.execs.iter().all(|exec| !exec.is_running())
    }

    pub(crate) fn exec_mut(&mut self, id: &str) -> Option<&mut ExecCell> {
        self.execs.iter_mut().find(|exec| exec.execution_id() == id)
    }

    pub(crate) fn abort_all(&mut self) {
        for exec in &mut self.execs {
            exec.abort();
        }
    }

    pub(crate) fn running_activities(&self) -> Vec<String> {
        self.execs
            .iter()
            .filter(|exec| exec.is_running())
            .map(ExecCell::activity)
            .collect()
    }

    fn display_lines_inner(&self, width: u16) -> Vec<Line<'static>> {
        match self.execs.as_slice() {
            [] => Vec::new(),
            [exec] => exec.display_lines(width),
            execs => group_lines(execs, width),
        }
    }

    fn transcript_lines_inner(&self, width: u16) -> Vec<Line<'static>> {
        match self.execs.as_slice() {
            [] => Vec::new(),
            [exec] => exec.transcript_lines(width),
            execs => group_lines(execs, width),
        }
    }
}

impl HistoryCell for RunningGroupCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.display_lines_inner(width)
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        self.transcript_lines_inner(width)
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        plain_lines(self.display_lines_inner(u16::MAX))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn group_lines(execs: &[ExecCell], width: u16) -> Vec<Line<'static>> {
    let width = width.max(1);
    let total = execs.len();
    let shown = total.min(MAX_GROUP_COMMANDS);
    let running = execs.iter().filter(|exec| exec.is_running()).count();
    let title = if running == 0 {
        format!("Ran {total} commands")
    } else if running == total {
        format!("Running {total} commands")
    } else {
        format!("Running {running} of {total} commands")
    };
    let marker_style = if running == 0 {
        success_style()
    } else {
        running_style()
    };
    let mut lines = vec![Line::from(vec![
        Span::styled("•", marker_style),
        Span::raw(" "),
        Span::styled(title, marker_style),
    ])];
    for (index, exec) in execs.iter().take(shown).enumerate() {
        let last = index + 1 == shown && total <= MAX_GROUP_COMMANDS;
        let branch = if last { "  └ " } else { "  ├ " };
        let continuation = if last { "    " } else { "  │ " };
        let (result, result_style) = if exec.is_running() {
            ("•", running_style())
        } else if exec.succeeded() {
            ("✓", success_style())
        } else {
            ("✗", error_style())
        };
        let command = if let Some(duration_ms) = exec.duration_ms() {
            format!(
                "{result} {} • {}",
                exec.command(),
                format_duration(duration_ms)
            )
        } else {
            format!("{result} {}", exec.command())
        };
        let available = usize::from(width).saturating_sub(branch.len()).max(1);
        for (part_index, part) in crate::wrapping::wrap_plain_no_hyphenation(&command, available)
            .into_iter()
            .enumerate()
        {
            let prefix = if part_index == 0 {
                branch
            } else {
                continuation
            };
            lines.push(Line::from(vec![
                Span::styled(prefix, secondary_style()),
                Span::styled(part, result_style),
            ]));
        }
    }
    let omitted = total.saturating_sub(shown);
    if omitted > 0 {
        lines.push(Line::from(Span::styled(
            format!("  └ … +{omitted} commands"),
            secondary_style(),
        )));
    }
    lines
}

fn format_duration(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms}ms")
    } else if duration_ms < 60_000 {
        format!("{:.1}s", duration_ms as f64 / 1_000.0)
    } else {
        format!("{}m {:02}s", duration_ms / 60_000, duration_ms / 1_000 % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_cell::HistoryCell;

    fn line_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn exec(id: &str, command: &str) -> ExecCell {
        ExecCell::new(
            id.to_owned(),
            "sh".to_owned(),
            vec!["-c".to_owned(), command.to_owned()],
        )
    }

    #[test]
    fn single_child_delegates_to_the_original_exec_cell() {
        let group = RunningGroupCell::from_exec(exec("exec-1", "npm test"));
        let rendered = group
            .display_lines(80)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>();
        assert_eq!(rendered, ["• Running npm test"]);
    }

    #[test]
    fn multiple_children_render_one_grouped_header() {
        let mut group = RunningGroupCell::from_exec(exec("exec-1", "npm test"));
        group.push(exec("exec-2", "npm run lint"));
        group.push(exec("exec-3", "npm run typecheck"));

        let rendered = group
            .display_lines(80)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>();
        assert_eq!(
            rendered,
            [
                "• Running 3 commands",
                "  ├ • npm test",
                "  ├ • npm run lint",
                "  └ • npm run typecheck",
            ]
        );
        assert_eq!(
            rendered
                .iter()
                .filter(|line| line.contains("Running"))
                .count(),
            1
        );
    }

    #[test]
    fn completed_children_keep_one_compact_group() {
        let mut group = RunningGroupCell::from_exec(exec("exec-1", "npm test"));
        group.push(exec("exec-2", "npm run lint"));
        group.exec_mut("exec-1").expect("first child").complete(
            String::new(),
            String::new(),
            0,
            42,
            "success".to_owned(),
        );
        group.exec_mut("exec-2").expect("second child").complete(
            String::new(),
            String::new(),
            0,
            73,
            "success".to_owned(),
        );

        let rendered = group
            .display_lines(80)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>();
        assert!(group.all_completed());
        assert_eq!(rendered[0], "• Ran 2 commands");
        assert_eq!(
            rendered
                .iter()
                .filter(|line| line.contains("commands"))
                .count(),
            1
        );
        assert!(rendered.iter().any(|line| line.contains("npm run lint")));
    }
}
