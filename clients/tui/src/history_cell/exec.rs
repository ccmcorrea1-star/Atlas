use ratatui::style::Stylize;
use ratatui::text::Line;
use serde_json::Value;

use super::HistoryCell;
use super::plain_lines;
use crate::capability_names::capability_activity_with_target;
use crate::capability_names::capability_label;
use crate::markdown::sanitize_terminal_text;
use crate::wrapping::display_width;
use crate::wrapping::wrap_line;
use crate::wrapping::wrap_text;

const TOOL_SUMMARY_MAX_CHARS: usize = 120;

/// Atividade generica de tool do Runtime renderizada com a margem do Codex.
#[derive(Debug)]
pub(crate) struct ToolCell {
    tool_id: String,
    tool_name: String,
    target: Option<String>,
    output: Option<String>,
    completed: bool,
}

impl ToolCell {
    pub(crate) fn new(tool_id: String, tool_name: String) -> Self {
        Self::new_with_target(tool_id, tool_name, None)
    }

    pub(crate) fn new_with_target(
        tool_id: String,
        tool_name: String,
        target: Option<String>,
    ) -> Self {
        Self {
            tool_id,
            tool_name,
            target,
            output: None,
            completed: false,
        }
    }

    pub(crate) fn tool_id(&self) -> &str {
        &self.tool_id
    }

    pub(crate) fn complete(&mut self, output: Option<String>) {
        self.output = output;
        self.completed = true;
    }

    pub(crate) fn is_running(&self) -> bool {
        !self.completed
    }

    pub(crate) fn activity(&self) -> String {
        capability_activity_with_target(&self.tool_name, self.target.as_deref())
    }
}

impl HistoryCell for ToolCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let width = width.max(1);
        let label = capability_label(&self.tool_name);
        let title = self
            .target
            .as_deref()
            .filter(|target| !target.trim().is_empty())
            .map_or(label.clone(), |target| format!("{label} {target}"));
        let status = self
            .completed
            .then(|| tool_status(self.output.as_deref()))
            .map_or("", ToolStatus::marker);
        let mut lines = wrap_text(&format!("• {title}{status}"), usize::from(width))
            .into_iter()
            .enumerate()
            .map(|(index, line)| {
                Line::from(if index == 0 {
                    line
                } else {
                    format!("  {line}")
                })
            })
            .collect::<Vec<_>>();
        if let Some(output) = &self.output {
            lines.extend(tool_summary_lines(&self.tool_name, output, width));
        }
        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        plain_lines(self.display_lines(u16::MAX))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToolStatus {
    Success,
    Error,
}

impl ToolStatus {
    fn marker(self) -> &'static str {
        match self {
            Self::Success => " ✓",
            Self::Error => " ✗",
        }
    }
}

fn tool_status(output: Option<&str>) -> ToolStatus {
    let Some(output) = output else {
        return ToolStatus::Success;
    };
    let Some(object) = json_object(output) else {
        return ToolStatus::Success;
    };
    if object
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status != "success")
        || object
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|error| !error.trim().is_empty())
    {
        ToolStatus::Error
    } else {
        ToolStatus::Success
    }
}

// Resume o resultado no cliente sem expor o JSON estruturado no transcript.
fn tool_summary_lines(tool_name: &str, output: &str, width: u16) -> Vec<Line<'static>> {
    let Some(summary) = tool_summary(tool_name, output) else {
        return Vec::new();
    };
    let output_width = usize::from(width).saturating_sub(4).max(1);
    wrap_line(Line::from(summary), output_width)
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            let content = line
                .spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>();
            Line::from(format!(
                "{}{}",
                if index == 0 { "  └ " } else { "    " },
                content
            ))
        })
        .collect()
}

fn tool_summary(tool_name: &str, output: &str) -> Option<String> {
    let object = json_object(output);
    let summary = match tool_name {
        "system.info" => object.as_ref().and_then(system_info_summary),
        "filesystem.read" => object.as_ref().and_then(filesystem_read_summary),
        "filesystem.search" => object.as_ref().and_then(filesystem_search_summary),
        "filesystem.glob" => object.as_ref().and_then(filesystem_glob_summary),
        "git.status" => object.as_ref().and_then(git_status_summary),
        "git.diff" => object.as_ref().and_then(git_diff_summary),
        _ => None,
    };
    summary.or_else(|| generic_summary(object.as_ref(), output))
}

fn json_object(output: &str) -> Option<serde_json::Map<String, Value>> {
    serde_json::from_str::<Value>(output)
        .ok()
        .and_then(|value| value.as_object().cloned())
}

fn system_info_summary(object: &serde_json::Map<String, Value>) -> Option<String> {
    let os_name = string_field(object, "os_name")?;
    let os_version = string_field(object, "os_version");
    let architecture = string_field(object, "architecture");
    let platform = string_field(object, "platform").map(format_platform);
    let operating_system = os_version
        .filter(|version| !version.is_empty())
        .map_or(os_name.clone(), |version| format!("{os_name} {version}"));
    Some(join_summary([
        Some(operating_system),
        architecture,
        platform,
    ]))
}

fn filesystem_read_summary(object: &serde_json::Map<String, Value>) -> Option<String> {
    let path = string_field(object, "path")?;
    let lines = number_field(object, "total_lines")?;
    Some(format!(
        "{path} · {lines} {}",
        plural(lines, "line", "lines")
    ))
}

fn filesystem_search_summary(object: &serde_json::Map<String, Value>) -> Option<String> {
    let count =
        number_field(object, "total_matches").or_else(|| array_length(object, "matches"))?;
    let truncated = bool_field(object, "truncated");
    Some(format_count(count, "result", "results", truncated))
}

fn filesystem_glob_summary(object: &serde_json::Map<String, Value>) -> Option<String> {
    let count =
        number_field(object, "total_matches").or_else(|| array_length(object, "matches"))?;
    let truncated = bool_field(object, "truncated");
    Some(format_count(count, "file found", "files found", truncated))
}

fn git_status_summary(object: &serde_json::Map<String, Value>) -> Option<String> {
    let branch = string_field(object, "branch").unwrap_or_default();
    let clean = bool_field(object, "clean");
    if clean {
        return Some(if branch.is_empty() {
            "clean".to_owned()
        } else {
            format!("{branch} · clean")
        });
    }

    let mut parts = Vec::new();
    if !branch.is_empty() {
        parts.push(branch);
    }
    append_count(
        &mut parts,
        array_length(object, "staged"),
        "staged",
        "staged",
    );
    append_count(
        &mut parts,
        array_length(object, "unstaged"),
        "modified",
        "modified",
    );
    append_count(
        &mut parts,
        array_length(object, "untracked"),
        "untracked",
        "untracked",
    );
    if let Some(ahead) = number_field(object, "ahead") {
        parts.push(format!("↑{ahead}"));
    }
    if let Some(behind) = number_field(object, "behind") {
        parts.push(format!("↓{behind}"));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn git_diff_summary(object: &serde_json::Map<String, Value>) -> Option<String> {
    let diff = string_field(object, "diff")?;
    let (files, additions, deletions) = diff_counts(&diff);
    let suffix = if bool_field(object, "truncated") {
        " · truncated"
    } else {
        ""
    };
    Some(format!(
        "{files} {} · +{additions} -{deletions}{suffix}",
        plural(files, "file changed", "files changed")
    ))
}

fn generic_summary(
    object: Option<&serde_json::Map<String, Value>>,
    output: &str,
) -> Option<String> {
    if let Some(object) = object {
        for field in ["error", "message", "summary", "result", "output"] {
            if let Some(value) = string_field(object, field).filter(|value| !value.is_empty()) {
                return Some(short_summary(&value));
            }
        }
        return Some("Structured result".to_owned());
    }
    if serde_json::from_str::<Value>(output).is_ok() {
        return Some("Structured result".to_owned());
    }
    let summary = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(short_summary)?;
    Some(summary)
}

fn string_field(object: &serde_json::Map<String, Value>, field: &str) -> Option<String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(|value| sanitize_terminal_text(value).trim().to_owned())
}

fn number_field(object: &serde_json::Map<String, Value>, field: &str) -> Option<u64> {
    object.get(field).and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
    })
}

fn bool_field(object: &serde_json::Map<String, Value>, field: &str) -> bool {
    object.get(field).and_then(Value::as_bool).unwrap_or(false)
}

fn array_length(object: &serde_json::Map<String, Value>, field: &str) -> Option<u64> {
    object
        .get(field)
        .and_then(Value::as_array)
        .map(|items| items.len() as u64)
}

fn format_platform(platform: String) -> String {
    match platform.to_ascii_lowercase().as_str() {
        "darwin" => "macOS".to_owned(),
        "linux" => "Linux".to_owned(),
        "windows" => "Windows".to_owned(),
        _ => platform,
    }
}

fn join_summary<const N: usize>(parts: [Option<String>; N]) -> String {
    parts
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn plural<'a>(count: u64, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}

fn format_count(count: u64, singular: &str, plural_form: &str, truncated: bool) -> String {
    let suffix = if truncated { " · truncated" } else { "" };
    format!("{count} {}{suffix}", plural(count, singular, plural_form))
}

fn append_count(parts: &mut Vec<String>, count: Option<u64>, singular: &str, plural_form: &str) {
    if let Some(count) = count.filter(|count| *count > 0) {
        parts.push(format!("{count} {}", plural(count, singular, plural_form)));
    }
}

fn diff_counts(diff: &str) -> (u64, u64, u64) {
    let mut files = 0;
    let mut additions = 0;
    let mut deletions = 0;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            files += 1;
        } else if line.starts_with("+") && !line.starts_with("+++") {
            additions += 1;
        } else if line.starts_with("-") && !line.starts_with("---") {
            deletions += 1;
        }
    }
    (files, additions, deletions)
}

fn short_summary(value: &str) -> String {
    let value = sanitize_terminal_text(value)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut end = value.len().min(TOOL_SUMMARY_MAX_CHARS);
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    if end == value.len() {
        value
    } else {
        format!("{}…", &value[..end.saturating_sub(1)])
    }
}

#[derive(Debug)]
pub(crate) struct ErrorCell {
    pub(crate) message: String,
}

impl ErrorCell {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl HistoryCell for ErrorCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let prefix = "! ";
        let usable = usize::from(width)
            .saturating_sub(display_width(prefix))
            .max(1);
        let mut result = Vec::new();
        for (line_index, source) in self.message.lines().enumerate() {
            for (part_index, line) in wrap_text(source, usable).into_iter().enumerate() {
                result.push(Line::from(vec![
                    (if line_index == 0 && part_index == 0 {
                        prefix
                    } else {
                        "  "
                    })
                    .to_string()
                    .red()
                    .dim(),
                    line.into(),
                ]));
            }
        }
        result
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        plain_lines(self.message.lines().map(|line| Line::from(line.to_owned())))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_text(line: &Line<'static>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn rendered(cell: &ToolCell, width: u16) -> Vec<String> {
        cell.display_lines(width)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>()
    }

    #[test]
    fn uses_short_capability_labels_instead_of_raw_ids() {
        let mut cell = ToolCell::new("tool-1".to_owned(), "filesystem.read".to_owned());
        cell.complete(None);
        assert_eq!(rendered(&cell, 80), ["• Read ✓"]);

        let mut search = ToolCell::new("tool-2".to_owned(), "filesystem.search".to_owned());
        search.complete(None);
        assert_eq!(rendered(&search, 80), ["• Search ✓"]);
    }

    #[test]
    fn includes_a_tool_target_when_the_runtime_provides_one() {
        let cell = ToolCell::new_with_target(
            "tool-1".to_owned(),
            "filesystem.read".to_owned(),
            Some("tsconfig.json".to_owned()),
        );
        assert_eq!(cell.activity(), "Reading tsconfig.json");
        assert_eq!(rendered(&cell, 80), ["• Read tsconfig.json"]);
    }

    #[test]
    fn fallback_summarizes_plain_output_instead_of_dumping_all_lines() {
        let mut cell = ToolCell::new("tool-1".to_owned(), "filesystem.read".to_owned());
        let output = (0..50)
            .map(|index| format!("line-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        cell.complete(Some(output));

        let lines = rendered(&cell, 80);
        assert_eq!(lines[0], "• Read ✓");
        assert_eq!(lines[1], "  └ line-0");
        assert_eq!(lines.len(), 2);
        assert!(!lines.iter().any(|line| line.contains("line-49")));
    }

    #[test]
    fn bounds_plain_fallback_summary_before_rendering() {
        let mut cell = ToolCell::new("tool-1".to_owned(), "filesystem.read".to_owned());
        cell.complete(Some("x".repeat(8 * 1024)));
        let lines = rendered(&cell, 80);
        assert!(lines.len() <= 3);
        assert!(lines.join(" ").chars().count() < 140);
    }

    #[test]
    fn formats_system_info_as_a_compact_platform_summary() {
        let mut cell = ToolCell::new("tool-1".to_owned(), "system.info".to_owned());
        let output = serde_json::json!({
            "status": "success",
            "error": "",
            "os_name": "Ubuntu",
            "os_version": "26.04",
            "architecture": "x86_64",
            "platform": "linux",
            "hostname": "atlas"
        })
        .to_string();
        cell.complete(Some(output));

        assert_eq!(
            rendered(&cell, 80),
            ["• System info ✓", "  └ Ubuntu 26.04 · x86_64 · Linux"]
        );
    }

    #[test]
    fn formats_filesystem_results_without_showing_structured_payloads() {
        let cases = [
            (
                "filesystem.read",
                serde_json::json!({"status": "success", "path": "README.md", "total_lines": 42}),
                "  └ README.md · 42 lines",
            ),
            (
                "filesystem.search",
                serde_json::json!({"status": "success", "total_matches": 3, "truncated": false}),
                "  └ 3 results",
            ),
            (
                "filesystem.glob",
                serde_json::json!({"status": "success", "total_matches": 5, "truncated": true}),
                "  └ 5 files found · truncated",
            ),
        ];

        for (tool_name, output, summary) in cases {
            let mut cell = ToolCell::new("tool-1".to_owned(), tool_name.to_owned());
            cell.complete(Some(output.to_string()));
            let lines = rendered(&cell, 100);
            assert_eq!(lines[1], summary);
            assert!(!lines.iter().any(|line| line.contains("\"status\"")));
        }
    }

    #[test]
    fn formats_git_status_and_diff_counts() {
        let mut status = ToolCell::new("tool-1".to_owned(), "git.status".to_owned());
        status.complete(Some(
            serde_json::json!({
                "status": "success",
                "branch": "main",
                "clean": false,
                "staged": [{"path": "a.rs"}],
                "unstaged": [{"path": "b.rs"}, {"path": "c.rs"}],
                "untracked": ["d.rs"]
            })
            .to_string(),
        ));
        assert_eq!(
            rendered(&status, 100),
            [
                "• Git status ✓",
                "  └ main · 1 staged · 2 modified · 1 untracked"
            ]
        );

        let mut diff = ToolCell::new("tool-2".to_owned(), "git.diff".to_owned());
        diff.complete(Some(
            serde_json::json!({
                "status": "success",
                "diff": "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n+added\n-removed\ndiff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n+another\n",
                "truncated": false
            })
            .to_string(),
        ));
        assert_eq!(
            rendered(&diff, 100),
            ["• Git diff ✓", "  └ 2 files changed · +2 -1"]
        );
    }

    #[test]
    fn uses_a_short_status_and_summary_for_unknown_tools() {
        let mut success = ToolCell::new("tool-1".to_owned(), "custom.report".to_owned());
        success.complete(Some(
            serde_json::json!({
                "status": "success",
                "target": "local",
                "value": {"secret": "do not render"}
            })
            .to_string(),
        ));
        let success_lines = rendered(&success, 100);
        assert_eq!(success_lines, ["• Report ✓", "  └ Structured result"]);
        assert!(!success_lines.iter().any(|line| line.contains("secret")));
        assert!(!success_lines.iter().any(|line| line.contains("status")));

        let mut array_result = ToolCell::new("tool-3".to_owned(), "custom.report".to_owned());
        array_result.complete(Some("[\"first\",\"second\"]".to_owned()));
        assert_eq!(
            rendered(&array_result, 100),
            ["• Report ✓", "  └ Structured result"]
        );

        let mut failure = ToolCell::new("tool-2".to_owned(), "custom.report".to_owned());
        failure.complete(Some(
            serde_json::json!({
                "status": "failed",
                "error": "permission denied while reading the report"
            })
            .to_string(),
        ));
        assert_eq!(
            rendered(&failure, 100),
            [
                "• Report ✗",
                "  └ permission denied while reading the report"
            ]
        );
    }

    #[test]
    fn retains_the_complete_output_for_the_client_cell() {
        let output = r#"{"status":"success","data":[1,2,3]}"#;
        let mut cell = ToolCell::new("tool-1".to_owned(), "custom.report".to_owned());
        cell.complete(Some(output.to_owned()));

        assert_eq!(cell.output.as_deref(), Some(output));
    }
}
