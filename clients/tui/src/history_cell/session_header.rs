use std::path::Path;

use ratatui::text::Line;
use ratatui::text::Span;

use super::HistoryCell;
use crate::ui_consts::action_style;
use crate::ui_consts::secondary_style;
#[cfg(test)]
use unicode_width::UnicodeWidthStr;

const DEFAULT_WORKSPACE: &str = "~/projetos/Atlas";
const DEFAULT_MODEL: &str = "gpt-5.6-luna";
const DEFAULT_PROVIDER: &str = "opencode-go";
const LOGO: [&str; 6] = [
    " █████╗ ████████╗██╗      █████╗ ███████╗",
    "██╔══██╗╚══██╔══╝██║     ██╔══██╗██╔════╝",
    "███████║   ██║   ██║     ███████║███████╗",
    "██╔══██║   ██║   ██║     ██╔══██║╚════██║",
    "██║  ██║   ██║   ███████╗██║  ██║███████║",
    "╚═╝  ╚═╝   ╚═╝   ╚══════╝╚═╝  ╚═╝╚══════╝",
];

#[derive(Debug)]
pub(crate) struct SessionHeaderCell {
    workspace: String,
    model: String,
    provider: String,
}

impl SessionHeaderCell {
    pub(crate) fn new(workspace: Option<&Path>, home: Option<&Path>) -> Self {
        Self {
            workspace: workspace
                .map(|path| compact_path(path, home))
                .unwrap_or_else(|| DEFAULT_WORKSPACE.to_owned()),
            model: DEFAULT_MODEL.to_owned(),
            provider: DEFAULT_PROVIDER.to_owned(),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_workspace(&mut self, workspace: &Path, home: Option<&Path>) {
        self.workspace = compact_path(workspace, home);
    }

    pub(crate) fn set_session(&mut self, model: String, provider: String) {
        self.model = model;
        self.provider = provider;
    }
}

impl HistoryCell for SessionHeaderCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = LOGO
            .iter()
            .map(|line| Line::from(Span::styled(*line, action_style())))
            .collect::<Vec<_>>();
        lines.push(Line::from(Span::styled(
            self.workspace.clone(),
            secondary_style(),
        )));
        lines.push(Line::from(Span::styled(
            format!("{} · {}", self.model, self.provider),
            secondary_style(),
        )));
        let separator_width = usize::from(width).saturating_sub(2).max(1);
        lines.push(Line::from(Span::styled(
            format!("  {}", "─".repeat(separator_width)),
            secondary_style(),
        )));
        lines.push(Line::default());
        lines
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.display_lines(u16::MAX)
            .into_iter()
            .map(|line| {
                Line::from(
                    line.spans
                        .into_iter()
                        .map(|span| span.content)
                        .collect::<String>(),
                )
            })
            .collect()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn compact_path(path: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home
        && let Ok(relative) = path.strip_prefix(home)
    {
        if relative.as_os_str().is_empty() {
            return "~".to_owned();
        }
        return format!("~/{}", relative.display());
    }
    if path.is_absolute() {
        return path.to_string_lossy().into_owned();
    }
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_the_session_header_as_transcript_content() {
        let header = SessionHeaderCell::new(
            Some(Path::new("/home/kyle/projetos/Atlas")),
            Some(Path::new("/home/kyle")),
        );
        let lines = header.display_lines(80);
        let output = text(&lines);

        assert!(output.starts_with(" █████╗ ████████╗"));
        assert!(output.contains("~/projetos/Atlas"));
        assert!(output.contains("gpt-5.6-luna · opencode-go"));
        assert!(output.ends_with("\n"));
        assert_eq!(lines.len(), 10);
        assert_eq!(lines[0].spans[0].style, action_style());
        assert_eq!(lines[6].spans[0].style, secondary_style());
    }

    #[test]
    fn keeps_the_separator_within_the_transcript_width() {
        let header = SessionHeaderCell::new(None, None);
        let line = &header.display_lines(20)[8];
        assert!(line.width() <= 20);
        assert_eq!(UnicodeWidthStr::width(line.spans[0].content.as_ref()), 20);
    }
}
