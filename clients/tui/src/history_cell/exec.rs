use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use serde_json::Value;
use std::path::Path;

use super::HistoryCell;
use super::plain_lines;
use crate::capability_names::capability_activity_with_target;
use crate::capability_names::capability_label;
use crate::markdown::sanitize_terminal_text;
use crate::wrapping::display_width;
use crate::wrapping::wrap_line;
use crate::wrapping::wrap_text;

const TOOL_SUMMARY_MAX_CHARS: usize = 120;
const FILESYSTEM_DIFF_PREVIEW_MAX_CHARS: usize = 600;
const FILESYSTEM_DIFF_PREVIEW_MAX_LINES: usize = 10;

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
        let target = self.display_target();
        capability_activity_with_target(&self.tool_name, target.as_deref())
    }

    fn display_target(&self) -> Option<String> {
        self.target
            .as_deref()
            .filter(|target| !target.trim().is_empty())
            .map(|target| {
                if self.tool_name.starts_with("filesystem.") {
                    display_path(target)
                } else {
                    sanitize_terminal_text(target).trim().to_owned()
                }
            })
    }
}

impl HistoryCell for ToolCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        if self.completed
            && let Some(lines) = filesystem_change_lines(
                &self.tool_name,
                self.output.as_deref().unwrap_or_default(),
                width,
                false,
            )
        {
            return lines;
        }

        let width = width.max(1);
        let label = capability_label(&self.tool_name);
        let title = self
            .display_target()
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

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        if self.completed
            && let Some(lines) = filesystem_change_lines(
                &self.tool_name,
                self.output.as_deref().unwrap_or_default(),
                width,
                true,
            )
        {
            return lines;
        }
        self.display_lines(width)
    }

    fn background_style(&self) -> Option<Style> {
        if !self.completed {
            return None;
        }
        let object = json_object(self.output.as_deref().unwrap_or_default())?;
        if tool_status(Some(self.output.as_deref().unwrap_or_default())) == ToolStatus::Error {
            return None;
        }
        let payload = filesystem_payload(&object);
        filesystem_changes(&self.tool_name, payload)
            .is_some()
            .then(super::messages::user_message_style)
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
        "filesystem.patch" => object.as_ref().and_then(filesystem_patch_summary),
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
    let path = display_path(&string_field(object, "path")?);
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

fn filesystem_patch_summary(object: &serde_json::Map<String, Value>) -> Option<String> {
    let failed = object
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status != "success")
        || object
            .get("error")
            .and_then(Value::as_str)
            .is_some_and(|error| !error.trim().is_empty());
    failed.then_some("failed".to_owned())
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
        return None;
    }
    if serde_json::from_str::<Value>(output).is_ok() {
        return None;
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
enum FilesystemDiffItem {
    Hunk(String),
    Line {
        marker: char,
        old_number: Option<usize>,
        new_number: Option<usize>,
        text: String,
    },
}

fn filesystem_change_lines(
    tool_name: &str,
    output: &str,
    _width: u16,
    transcript: bool,
) -> Option<Vec<Line<'static>>> {
    if !matches!(tool_name, "filesystem.patch") {
        return None;
    }
    let object = json_object(output)?;
    if tool_status(Some(output)) == ToolStatus::Error {
        return None;
    }
    let payload = filesystem_payload(&object);
    let changes = filesystem_changes(tool_name, payload)?;
    let mut lines = Vec::new();
    let mut diff_lines_shown = 0;
    let mut diff_chars_shown: usize = 0;
    let mut diff_lines_omitted = 0;

    for change in changes {
        let action = change.get("action").and_then(Value::as_str)?;
        let action = match action {
            "create" | "add" => "Created",
            "edit" | "update" => "Edited",
            "delete" | "remove" => "Deleted",
            "move" => "Moved",
            _ => return None,
        };
        let path = change.get("path").and_then(Value::as_str)?;
        let path = display_path(path);
        let title = if action == "Moved" {
            let destination = change.get("moved_to").and_then(Value::as_str)?;
            format!("✓ {action} {path} → {}", display_path(destination))
        } else {
            format!("✓ {action} {path}")
        };
        lines.push(Line::from(title));

        let Some(diff) = change.get("diff").and_then(Value::as_str) else {
            continue;
        };
        let diff_items = filesystem_diff_items(diff);
        if diff_items.is_empty() {
            continue;
        }
        lines.push(Line::default());
        for diff_item in diff_items {
            let rendered = filesystem_diff_line(diff_item);
            if transcript {
                lines.push(rendered);
                continue;
            }
            let line_chars = rendered
                .spans
                .iter()
                .map(|span| span.content.chars().count())
                .sum::<usize>();
            if diff_lines_shown < FILESYSTEM_DIFF_PREVIEW_MAX_LINES
                && diff_chars_shown.saturating_add(line_chars) <= FILESYSTEM_DIFF_PREVIEW_MAX_CHARS
            {
                lines.push(rendered);
                diff_lines_shown += 1;
                diff_chars_shown = diff_chars_shown.saturating_add(line_chars);
            } else {
                diff_lines_omitted += 1;
            }
        }
    }

    if !transcript && diff_lines_omitted > 0 {
        lines.push(Line::from(format!(
            "  … {diff_lines_omitted} diff lines omitted; open transcript for full diff"
        )));
    }
    Some(lines)
}

fn filesystem_diff_items(diff: &str) -> Vec<FilesystemDiffItem> {
    let mut old_number = 1;
    let mut new_number = 1;
    let mut items = Vec::new();
    for source in diff.lines() {
        if source.starts_with("@@") {
            if let Some((old_start, new_start)) = diff_hunk_starts(source) {
                old_number = old_start;
                new_number = new_start;
            }
            items.push(FilesystemDiffItem::Hunk(source.to_owned()));
            continue;
        }

        let Some(marker) = source.chars().next() else {
            continue;
        };
        if !matches!(marker, ' ' | '+' | '-') {
            continue;
        }
        let text = source[marker.len_utf8()..].to_owned();
        let item = match marker {
            ' ' => {
                let old = (old_number > 0).then_some(old_number);
                let new = (new_number > 0).then_some(new_number);
                old_number = old_number.saturating_add(1);
                new_number = new_number.saturating_add(1);
                FilesystemDiffItem::Line {
                    marker,
                    old_number: old,
                    new_number: new,
                    text,
                }
            }
            '-' => {
                let old = (old_number > 0).then_some(old_number);
                old_number = old_number.saturating_add(1);
                FilesystemDiffItem::Line {
                    marker,
                    old_number: old,
                    new_number: None,
                    text,
                }
            }
            '+' => {
                let new = (new_number > 0).then_some(new_number);
                new_number = new_number.saturating_add(1);
                FilesystemDiffItem::Line {
                    marker,
                    old_number: None,
                    new_number: new,
                    text,
                }
            }
            _ => unreachable!(),
        };
        items.push(item);
    }
    items
}

fn diff_hunk_starts(header: &str) -> Option<(usize, usize)> {
    let ranges = header.strip_prefix("@@")?.split("@@").next()?;
    let mut parts = ranges.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    Some((diff_range_start(old)?, diff_range_start(new)?))
}

fn diff_range_start(range: &str) -> Option<usize> {
    let start = range.split(',').next()?;
    start.parse().ok()
}

fn filesystem_diff_line(item: FilesystemDiffItem) -> Line<'static> {
    match item {
        FilesystemDiffItem::Hunk(header) => Line::from(Span::styled(
            format!("  {header}"),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM),
        )),
        FilesystemDiffItem::Line {
            marker,
            old_number,
            new_number,
            text,
        } => {
            let old = old_number.map_or_else(String::new, |number| number.to_string());
            let new = new_number.map_or_else(String::new, |number| number.to_string());
            let gutter = format!("  {old:>3} {new:>3} │ ");
            let content = if marker == ' ' {
                text
            } else {
                format!("{marker}{text}")
            };
            let content_style = match marker {
                '+' => Style::default().fg(Color::Green),
                '-' => Style::default().fg(Color::Red),
                _ => Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
            };
            Line::from(vec![
                Span::styled(gutter, Style::default().fg(Color::Gray).dim()),
                Span::styled(content, content_style),
            ])
        }
    }
}

fn filesystem_payload(object: &serde_json::Map<String, Value>) -> &serde_json::Map<String, Value> {
    object
        .get("output")
        .and_then(Value::as_object)
        .filter(|output| output.contains_key("action") || output.contains_key("changes"))
        .unwrap_or(object)
}

fn filesystem_changes<'a>(
    tool_name: &str,
    object: &'a serde_json::Map<String, Value>,
) -> Option<Vec<&'a serde_json::Map<String, Value>>> {
    if tool_name == "filesystem.patch" {
        return object
            .get("changes")
            .and_then(Value::as_array)
            .filter(|changes| !changes.is_empty())
            .map(|changes| changes.iter().filter_map(Value::as_object).collect())
            .filter(|changes: &Vec<&serde_json::Map<String, Value>>| !changes.is_empty());
    }
    object
        .get("action")
        .and_then(Value::as_str)
        .map(|_| vec![object])
}

fn display_path(path: &str) -> String {
    let path = Path::new(path);
    if !path.is_absolute() {
        return sanitize_terminal_text(path.to_string_lossy().as_ref());
    }
    if let Ok(current_dir) = std::env::current_dir()
        && let Ok(relative) = path.strip_prefix(current_dir)
    {
        return sanitize_terminal_text(relative.to_string_lossy().as_ref());
    }
    path.file_name().map_or_else(
        || "<workspace>".to_owned(),
        |name| sanitize_terminal_text(name.to_string_lossy().as_ref()),
    )
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
    fn renders_filesystem_create_edit_delete_and_move_operations() {
        let cases = [
            (
                serde_json::json!({
                    "status": "success",
                    "changes": [{
                        "action": "create",
                        "path": "teste.txt",
                        "diff": "@@\n+Arquivo de teste.\n"
                    }]
                }),
                vec![
                    "✓ Created teste.txt",
                    "",
                    "  @@",
                    "        1 │ +Arquivo de teste.",
                ],
            ),
            (
                serde_json::json!({
                    "status": "success",
                    "changes": [{
                        "action": "edit",
                        "path": "teste.txt",
                        "diff": "@@ -1,1 +1,2 @@\n Arquivo de teste.\n+Segunda edição realizada.\n"
                    }]
                }),
                vec![
                    "✓ Edited teste.txt",
                    "",
                    "  @@ -1,1 +1,2 @@",
                    "    1   1 │ Arquivo de teste.",
                    "        2 │ +Segunda edição realizada.",
                ],
            ),
            (
                serde_json::json!({
                    "status": "success",
                    "changes": [{
                        "action": "delete",
                        "path": "teste.txt",
                        "diff": "@@\n-Arquivo de teste.\n-Segunda edição realizada.\n"
                    }]
                }),
                vec![
                    "✓ Deleted teste.txt",
                    "",
                    "  @@",
                    "    1     │ -Arquivo de teste.",
                    "    2     │ -Segunda edição realizada.",
                ],
            ),
            (
                serde_json::json!({
                    "status": "success",
                    "changes": [{
                        "action": "move",
                        "path": "teste.txt",
                        "moved_to": "docs/teste.txt",
                        "diff": ""
                    }]
                }),
                vec!["✓ Moved teste.txt → docs/teste.txt"],
            ),
        ];

        for (output, expected) in cases {
            let mut cell = ToolCell::new("tool-1".to_owned(), "filesystem.patch".to_owned());
            cell.complete(Some(output.to_string()));
            assert_eq!(rendered(&cell, 100), expected);
        }
    }

    #[test]
    fn keeps_filesystem_paths_relative_in_the_conversation() {
        let mut cell = ToolCell::new("tool-1".to_owned(), "filesystem.patch".to_owned());
        cell.complete(Some(
            serde_json::json!({
                "status": "success",
                "changes": [{
                    "action": "create",
                    "path": "/workspace/project/teste.txt",
                    "diff": "@@\n+content\n"
                }]
            })
            .to_string(),
        ));

        let lines = rendered(&cell, 100);
        assert!(!lines.join("\n").contains("/workspace/project"));
        assert!(lines[0].contains("teste.txt"));
    }

    #[test]
    fn styles_filesystem_diff_blocks_and_line_kinds() {
        let mut cell = ToolCell::new("tool-1".to_owned(), "filesystem.patch".to_owned());
        cell.complete(Some(
            serde_json::json!({
                "status": "success",
                "changes": [{
                    "action": "edit",
                    "path": "teste.txt",
                    "diff": "@@ -2,2 +2,2 @@\n context\n-old\n+new\n"
                }]
            })
            .to_string(),
        ));

        assert_eq!(
            cell.background_style().and_then(|style| style.bg),
            Some(Color::Rgb(51, 51, 51))
        );
        let lines = cell.display_lines(100);
        assert_eq!(lines[4].spans[1].style.fg, Some(Color::Red));
        assert_eq!(lines[5].spans[1].style.fg, Some(Color::Green));
        assert!(lines[2].spans[0].style.add_modifier.contains(Modifier::DIM));
        assert!(lines[3].spans[1].style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn hides_patch_error_payloads_and_absolute_paths() {
        let mut cell = ToolCell::new("tool-1".to_owned(), "filesystem.patch".to_owned());
        cell.complete(Some(
            serde_json::json!({
                "status": "failed",
                "error": "cannot update /workspace/project/teste.txt"
            })
            .to_string(),
        ));

        let lines = rendered(&cell, 100);
        assert_eq!(lines, ["• Patch ✗", "  └ failed"]);
        assert!(!lines.join("\n").contains("/workspace/project"));
        assert!(!lines.join("\n").contains("status"));
    }

    #[test]
    fn compacts_large_filesystem_diffs_but_keeps_the_full_transcript() {
        let diff = format!(
            "@@\n{}",
            (0..40).map(|i| format!("+line-{i}\n")).collect::<String>()
        );
        let mut cell = ToolCell::new("tool-1".to_owned(), "filesystem.patch".to_owned());
        cell.complete(Some(
            serde_json::json!({
                "status": "success",
                "changes": [{
                    "action": "create",
                    "path": "large.txt",
                    "diff": diff
                }]
            })
            .to_string(),
        ));

        let preview = rendered(&cell, 100);
        assert!(preview.iter().any(|line| line.contains("diff lines omitted")));
        assert!(!preview.iter().any(|line| line.contains("line-39")));

        let transcript = cell
            .transcript_lines(100)
            .iter()
            .map(line_text)
            .collect::<Vec<_>>();
        assert!(transcript.iter().any(|line| line.contains("line-39")));
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
        assert_eq!(success_lines, ["• Report ✓"]);
        assert!(!success_lines.iter().any(|line| line.contains("secret")));
        assert!(!success_lines.iter().any(|line| line.contains("status")));

        let mut array_result = ToolCell::new("tool-3".to_owned(), "custom.report".to_owned());
        array_result.complete(Some("[\"first\",\"second\"]".to_owned()));
        assert_eq!(rendered(&array_result, 100), ["• Report ✓"]);

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
