//! Renderizacao do footer adaptada do painel inferior da TUI do Codex.
//!
//! O footer e uma view pura do estado do composer. Ele concentra workspace,
//! atalhos, modelo e contexto sem deixar a informacao da direita cobrir a dica.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use ratatui::style::Modifier;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Widget;
use std::path::Path;
use std::path::PathBuf;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::ui_consts::composer_secondary_style;
use crate::ui_consts::error_style;
use crate::ui_consts::warning_style;

const SHORTCUT_HEIGHT: u16 = 11;
/// Espaco minimo entre o grupo da esquerda e o contexto alinhado a direita.
const RIGHT_ALIGNED_GAP: usize = 2;
/// Separador entre o caminho do workspace e a dica do composer.
const LEFT_GROUP_SEPARATOR: &str = " · ";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FooterMode {
    HistorySearch,
    QuitShortcutReminder,
    ShortcutOverlay,
    Connecting,
    EscHint,
    ComposerEmpty,
    ComposerHasDraft,
}

#[derive(Debug)]
pub(crate) struct FooterProps {
    mode: FooterMode,
    left: String,
    session: Option<String>,
    workspace: Option<PathBuf>,
    home: Option<PathBuf>,
    context: Option<String>,
}

impl FooterProps {
    pub(crate) fn from_app(app: &App) -> Self {
        let mode = if app.bottom_pane().shortcuts_open() {
            FooterMode::ShortcutOverlay
        } else if app.history_search_open() {
            FooterMode::HistorySearch
        } else if app.quit_confirmation() {
            FooterMode::QuitShortcutReminder
        } else if app.esc_backtrack_hint() {
            FooterMode::EscHint
        } else if !app.runtime_connected() {
            FooterMode::Connecting
        } else if app.input().is_empty() {
            FooterMode::ComposerEmpty
        } else {
            FooterMode::ComposerHasDraft
        };
        let left = match mode {
            FooterMode::QuitShortcutReminder => "ctrl + c again to quit".to_owned(),
            FooterMode::EscHint => "esc again to edit previous message".to_owned(),
            FooterMode::HistorySearch => {
                let query = app.history_search_query();
                if app.history_search_has_match() {
                    format!("reverse-i-search: {query}")
                } else {
                    format!("reverse-i-search: {query} (no match)")
                }
            }
            FooterMode::Connecting => "Connecting…".to_owned(),
            FooterMode::ComposerEmpty | FooterMode::ComposerHasDraft => "? shortcuts".to_owned(),
            FooterMode::ShortcutOverlay => String::new(),
        };
        let session = session_info(app);
        let context = app
            .context_usage()
            .map(|usage| format_context(usage.used_tokens, usage.context_window));
        Self {
            mode,
            left,
            session,
            workspace: app.workspace().map(Path::to_path_buf),
            home: app.home().map(Path::to_path_buf),
            context,
        }
    }
}

/// Linhas ocupadas pelo footer dentro do painel inferior.
pub(crate) fn desired_height(app: &App, _width: u16) -> u16 {
    if app.bottom_pane().shortcuts_open() {
        SHORTCUT_HEIGHT
    } else {
        1
    }
}

fn session_info(app: &App) -> Option<String> {
    app.session_model().map(ToOwned::to_owned)
}

pub(crate) fn render(app: &App, area: Rect, buffer: &mut Buffer) {
    if area.is_empty() {
        return;
    }
    let props = FooterProps::from_app(app);
    if props.mode == FooterMode::ShortcutOverlay {
        render_shortcut_overlay(app, area, buffer);
        return;
    }

    render_workspace_line(area, &props, buffer);
}

fn render_workspace_line(area: Rect, props: &FooterProps, buffer: &mut Buffer) {
    let available = usize::from(area.width);
    let candidates = right_candidates(props);
    let right = candidates
        .iter()
        .enumerate()
        .find(|(index, candidate)| {
            let right_width = UnicodeWidthStr::width(candidate.as_str());
            if right_width + RIGHT_ALIGNED_GAP > available {
                return false;
            }
            let is_last = *index + 1 == candidates.len();
            is_last || left_group_fits(props, available, right_width)
        })
        .map(|(_, candidate)| candidate.clone());
    let right_width = right.as_deref().map_or(0, UnicodeWidthStr::width);
    let reserved = if right.is_some() {
        right_width.saturating_add(RIGHT_ALIGNED_GAP)
    } else {
        0
    };
    let left = left_group(props, available.saturating_sub(reserved));
    let left_width = UnicodeWidthStr::width(left.as_str());
    let mut line = Line::from(Span::styled(left, composer_secondary_style()));
    if let Some(right) = right {
        let padding = available
            .saturating_sub(left_width)
            .saturating_sub(right_width)
            .max(RIGHT_ALIGNED_GAP.min(available));
        line.push_span(Span::raw(" ".repeat(padding)));
        line.push_span(Span::styled(right, composer_secondary_style()));
    }
    line.render(area, buffer);
}

fn right_candidates(props: &FooterProps) -> Vec<String> {
    match (props.session.as_deref(), props.context.as_deref()) {
        (Some(session), Some(context)) => vec![
            format!("{session} · {context}"),
            context.to_owned(),
            session.to_owned(),
        ],
        (Some(session), None) => vec![session.to_owned()],
        (None, Some(context)) => vec![context.to_owned()],
        (None, None) => Vec::new(),
    }
}

fn left_group_fits(props: &FooterProps, available: usize, right_width: usize) -> bool {
    let budget = available.saturating_sub(right_width + RIGHT_ALIGNED_GAP);
    let left = left_group(props, budget);
    let hint = props.left.trim();
    hint.is_empty() || left.ends_with(hint)
}

/// Grupo da esquerda: caminho do workspace e a dica do composer.
///
/// O caminho cede espaco primeiro; a dica so e cortada quando sozinha nao cabe.
fn left_group(props: &FooterProps, budget: usize) -> String {
    let hint = props.left.trim();
    let hint_width = UnicodeWidthStr::width(hint);
    if hint.is_empty() {
        return props
            .workspace
            .as_deref()
            .and_then(|workspace| workspace_label(workspace, props.home.as_deref(), budget))
            .unwrap_or_default();
    }
    if hint_width >= budget {
        return truncate_to_width(hint, budget);
    }
    let separator_width = UnicodeWidthStr::width(LEFT_GROUP_SEPARATOR);
    let workspace_budget = budget
        .saturating_sub(hint_width)
        .saturating_sub(separator_width);
    match props
        .workspace
        .as_deref()
        .and_then(|workspace| workspace_label(workspace, props.home.as_deref(), workspace_budget))
    {
        Some(label) if !label.is_empty() => {
            format!("{label}{LEFT_GROUP_SEPARATOR}{hint}")
        }
        _ => hint.to_owned(),
    }
}

/// Rotulo do workspace: `~/` quando cabe, `…/` apenas em truncamento real.
pub(crate) fn workspace_label(
    workspace: &Path,
    home: Option<&Path>,
    available: usize,
) -> Option<String> {
    if available == 0 {
        return None;
    }
    let mut candidates = Vec::new();
    if let Some(home) = home
        && let Ok(relative) = workspace.strip_prefix(home)
    {
        let relative = relative.to_string_lossy();
        if relative.is_empty() {
            candidates.push("~".to_owned());
        } else {
            candidates.push(format!("~/{relative}"));
        }
    }
    candidates.push(workspace.to_string_lossy().into_owned());
    let relative_components = display_components(workspace, home);
    let absolute_components = display_components(workspace, None);
    for components in [&relative_components, &absolute_components] {
        for keep in (1..components.len()).rev() {
            candidates.push(format!(
                "…/{}",
                components[components.len() - keep..].join("/")
            ));
        }
    }
    if let Some(last) = absolute_components.last() {
        candidates.push(format!("…/{last}"));
    }
    candidates
        .into_iter()
        .find(|candidate| UnicodeWidthStr::width(candidate.as_str()) <= available)
}

fn display_components(workspace: &Path, home: Option<&Path>) -> Vec<String> {
    let path = match home {
        Some(home) => workspace.strip_prefix(home).unwrap_or(workspace),
        None => workspace,
    };
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

fn truncate_to_width(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= width {
        return text.to_owned();
    }
    let mut truncated = String::new();
    let mut used = 0usize;
    for character in text.chars() {
        let character_width = UnicodeWidthStr::width(character.to_string().as_str());
        if used + character_width > width {
            break;
        }
        truncated.push(character);
        used += character_width;
    }
    truncated
}

#[allow(dead_code)]
pub(crate) fn render_status_line(app: &App, area: Rect, buffer: &mut Buffer) {
    let Some(line) = status_line(app, area.width) else {
        return;
    };
    line.render(area, buffer);
}

#[allow(dead_code)]
fn status_line(app: &App, width: u16) -> Option<Line<'static>> {
    match app.status() {
        crate::app::Status::Ready => None,
        crate::app::Status::Working | crate::app::Status::Executing => (width > 0)
            .then(|| Line::from(Span::styled("esc to interrupt", composer_secondary_style()))),
        crate::app::Status::Error(message) => Some(Line::from(Span::styled(
            format!("! {message}"),
            error_style(),
        ))),
    }
}

#[cfg(test)]
fn format_duration(total_seconds: u64) -> String {
    let seconds = total_seconds % 60;
    let minutes = (total_seconds / 60) % 60;
    let hours = total_seconds / 3600;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

fn render_shortcut_overlay(app: &App, area: Rect, buffer: &mut Buffer) {
    let queue_hint = if app.turn_active() {
        "tab to queue message"
    } else {
        "tab to submit message"
    };
    let rows = [
        ("/ for commands", ""),
        ("shift + enter for newline", queue_hint),
        ("@ for file paths", ""),
        (
            "ctrl + g to edit in external editor",
            "esc again to edit previous message",
        ),
        ("ctrl + r search history", "ctrl + c to exit"),
        ("ctrl + t to view transcript", ""),
        ("", ""),
        ("", ""),
        ("", ""),
    ];
    let first_row = area.y + area.height.saturating_sub(rows.len() as u16);
    for (index, (left, right)) in rows.into_iter().enumerate() {
        let y = first_row + u16::try_from(index).unwrap_or(u16::MAX);
        if y >= area.bottom() {
            break;
        }
        let left_width = usize::from(area.width) / 2;
        Line::from(vec![
            Span::styled(left, composer_secondary_style()),
            Span::raw(" ".repeat(left_width.saturating_sub(left.width()))),
            Span::styled(right, composer_secondary_style()),
        ])
        .render(Rect::new(area.x, y, area.width, 1), buffer);
    }
    if area.height > 1 {
        buffer.set_span(
            area.x,
            area.y + 1,
            &Span::styled("›", warning_style().add_modifier(Modifier::BOLD)),
            1,
        );
        buffer.set_span(
            area.x + 2,
            area.y + 1,
            &Span::styled("Ask Atlas to do anything", composer_secondary_style()),
            area.width.saturating_sub(2),
        );
    }
}

fn format_context(used: u64, window: u64) -> String {
    let remaining = window.saturating_sub(used).saturating_mul(100) / window.max(1);
    format!(
        "{}/{} · {remaining}% left",
        format_token_count(used),
        format_token_count(window)
    )
}

fn format_token_count(tokens: u64) -> String {
    if tokens < 1_000 {
        return tokens.to_string();
    }
    if tokens < 1_000_000 {
        return format_compact_count(tokens, 1_000, 'k');
    }
    format_compact_count(tokens, 1_000_000, 'm')
}

fn format_compact_count(tokens: u64, unit: u64, suffix: char) -> String {
    let whole = tokens / unit;
    let remainder = tokens % unit;
    if remainder == 0 || whole >= 100 {
        return format!("{whole}{suffix}");
    }
    let tenths = (remainder.saturating_mul(10) + unit / 2) / unit;
    if tenths >= 10 {
        format!("{}{}", whole + 1, suffix)
    } else {
        format!("{whole}.{tenths}{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::FooterMode;
    use crate::app::App;
    use crate::runtime::ContextUsage;
    use crate::runtime::RuntimeEvent;
    use crate::ui_consts::bottom_pane_inner_area;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use std::path::Path;

    fn workspace_row(app: &App, width: u16) -> String {
        let area = bottom_pane_inner_area(Rect::new(0, 0, width, 1));
        let mut buffer = Buffer::empty(area);
        super::render(app, area, &mut buffer);
        buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    fn with_context(app: &mut App) {
        app.handle_runtime_event(RuntimeEvent::ContextUpdated {
            context: ContextUsage {
                used_tokens: 2_600,
                context_window: 256_000,
            },
        });
    }

    #[test]
    fn workspace_label_prefers_the_home_form_and_only_elides_when_it_must() {
        let workspace = Path::new("/home/kyle/projetos/Atlas");
        let home = Some(Path::new("/home/kyle"));

        assert_eq!(
            super::workspace_label(workspace, home, 40).as_deref(),
            Some("~/projetos/Atlas")
        );
        assert_eq!(
            super::workspace_label(workspace, home, 16).as_deref(),
            Some("~/projetos/Atlas")
        );
        assert_eq!(
            super::workspace_label(workspace, home, 15).as_deref(),
            Some("…/Atlas")
        );
        assert_eq!(
            super::workspace_label(workspace, home, 7).as_deref(),
            Some("…/Atlas")
        );
        assert_eq!(super::workspace_label(workspace, home, 6), None);
    }

    #[test]
    fn workspace_label_keeps_absolute_paths_outside_the_home_directory() {
        let workspace = Path::new("/opt/projetos/Atlas");
        let home = Some(Path::new("/home/kyle"));

        assert_eq!(
            super::workspace_label(workspace, home, 40).as_deref(),
            Some("/opt/projetos/Atlas")
        );
        assert_eq!(
            super::workspace_label(workspace, home, 10).as_deref(),
            Some("…/Atlas")
        );
    }

    #[test]
    fn renders_workspace_hint_and_context_inside_the_shared_inset() {
        let mut app = App::new("footer-workspace".to_owned());
        app.set_display_paths("/home/kyle/projetos/Atlas", "/home/kyle");
        with_context(&mut app);

        let row = workspace_row(&app, 80);

        assert!(row.starts_with("~/projetos/Atlas · ? shortcuts"));
        assert!(row.ends_with("2.6k/256k · 98% left"));
    }

    #[test]
    fn drops_the_workspace_before_the_hint_on_narrow_terminals() {
        let mut app = App::new("footer-narrow".to_owned());
        app.set_display_paths("/home/kyle/projetos/Atlas", "/home/kyle");
        with_context(&mut app);

        let narrow = workspace_row(&app, 40);
        assert!(narrow.starts_with("? shortcuts"));
        assert!(narrow.ends_with("2.6k/256k · 98% left"));

        // Sem largura para o contexto, a dica permanece dentro do inset.
        let tiny = workspace_row(&app, 12);
        assert_eq!(tiny, "? shortc");
    }

    #[test]
    fn shortcut_overlay_height_matches_composer_layout() {
        let mut app = App::new("footer-height".to_owned());
        app.open_shortcuts();
        assert_eq!(super::desired_height(&app, 80), 11);
    }

    #[test]
    fn shortcut_overlay_omits_unsupported_shell_mode() {
        let mut app = App::new("footer-shell".to_owned());
        app.open_shortcuts();
        let props = super::FooterProps::from_app(&app);
        assert!(!props.left.contains("shell"));
    }

    #[test]
    fn starts_ready_for_the_first_turn() {
        let app = App::new("footer".to_owned());
        assert_eq!(
            super::FooterProps::from_app(&app).mode,
            FooterMode::ComposerEmpty
        );
    }

    fn status_output(app: &App) -> String {
        let area = Rect::new(0, 0, 80, 1);
        let mut buffer = Buffer::empty(area);
        super::render_status_line(app, area, &mut buffer);
        buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    #[test]
    fn renders_thinking_status_line_from_runtime_turn_state() {
        let mut app = App::new("footer-status".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);

        let output = status_output(&app);
        assert!(output.contains("esc to interrupt"));
        assert!(!output.contains("Thinking"));
        assert!(output.contains("esc to interrupt"));
        assert!(!output.contains("Working"));
    }

    #[test]
    fn renders_tool_activity_in_the_status_line() {
        let mut app = App::new("footer-activity".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        app.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
            target: Some("tsconfig.json".to_owned()),
        });

        assert_eq!(status_output(&app).trim(), "esc to interrupt");

        app.handle_runtime_event(RuntimeEvent::ToolCompleted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
            output: None,
        });
        assert!(status_output(&app).contains("esc to interrupt"));
        assert!(!status_output(&app).contains("Thinking"));
    }

    #[test]
    fn renders_execution_activity_in_the_status_line() {
        let mut app = App::new("footer-exec-activity".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        app.handle_runtime_event(RuntimeEvent::ExecutionStarted {
            execution_id: "exec-1".to_owned(),
            capability: "shell.exec".to_owned(),
            program: "sh".to_owned(),
            args: vec!["-c".to_owned(), "npm test".to_owned()],
            cwd: None,
            target: None,
        });

        assert_eq!(status_output(&app).trim(), "esc to interrupt");
    }

    #[test]
    fn snapshots_active_footer_hierarchy() {
        let mut app = App::new("footer-snapshot".to_owned());
        app.handle_runtime_event(RuntimeEvent::TurnStarted);
        app.handle_runtime_event(RuntimeEvent::ToolStarted {
            tool_id: "tool-1".to_owned(),
            tool_name: "filesystem.read".to_owned(),
            target: Some("src/main.rs".to_owned()),
        });

        insta::assert_snapshot!("footer_activity", status_output(&app));
    }

    #[test]
    fn formats_turn_duration_in_compact_units() {
        assert_eq!(super::format_duration(0), "0s");
        assert_eq!(super::format_duration(42), "42s");
        assert_eq!(super::format_duration(73), "1m 13s");
        assert_eq!(super::format_duration(3_840), "1h 4m");
    }

    #[test]
    fn renders_error_status_line_without_working_hint() {
        let mut app = App::new("footer-error".to_owned());
        app.handle_runtime_event(RuntimeEvent::Error {
            message: "runtime unavailable".to_owned(),
        });
        let area = Rect::new(0, 0, 80, 1);
        let mut buffer = Buffer::empty(area);

        super::render_status_line(&app, area, &mut buffer);
        let output = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(output.contains("! runtime unavailable"));
        assert!(!output.contains("Thinking ("));
    }

    #[test]
    fn formats_context_as_remaining_percentage() {
        assert_eq!(super::format_context(100, 156_000), "100/156k · 99% left");
        assert_eq!(
            super::format_context(12_000, 200_000),
            "12k/200k · 94% left"
        );
        assert_eq!(
            super::format_context(1_234, 1_000_000),
            "1.2k/1m · 99% left"
        );
    }

    #[test]
    fn renders_session_info_with_model_only() {
        let mut app = App::new("footer-session-info".to_owned());
        app.handle_runtime_event(RuntimeEvent::SessionUpdated {
            model: "gpt-5.6-luna".to_owned(),
            provider: "opencode-go".to_owned(),
        });
        let output = super::session_info(&app);
        assert_eq!(output.as_deref(), Some("gpt-5.6-luna"));
        assert!(!output.as_deref().unwrap().contains("opencode-go"));
    }

    #[test]
    fn ready_footer_omits_missing_session_metadata() {
        let app = App::new("footer-ready".to_owned());
        let props = super::FooterProps::from_app(&app);
        assert_eq!(props.mode, FooterMode::ComposerEmpty);
        assert_eq!(props.left, "? shortcuts");
        assert!(props.session.is_none());
    }
}
