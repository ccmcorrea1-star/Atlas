use std::env;
use std::path::Component;
use std::path::Path;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::ui_consts::action_style;
use crate::ui_consts::elevated_surface_style;
use crate::ui_consts::secondary_style;

pub(crate) const HEIGHT: u16 = 3;

pub(crate) fn desired_height(width: u16) -> u16 {
    if width > 0 { HEIGHT } else { 0 }
}

pub(crate) fn render(area: Rect, buffer: &mut Buffer, _app: &App) {
    if area.is_empty() {
        return;
    }

    let header_area = Rect::new(area.x, area.y, area.width, HEIGHT.min(area.height));
    Clear.render(header_area, buffer);
    buffer.set_style(header_area, elevated_surface_style());

    let title = Span::styled("Atlas", action_style().add_modifier(Modifier::BOLD));
    let title_width = UnicodeWidthStr::width("Atlas");
    let directory = env::current_dir().ok().map(|path| compact_path(&path));
    let available_path_width = usize::from(header_area.width).saturating_sub(title_width + 1);
    let directory = directory
        .filter(|_| available_path_width > 0)
        .map(|path| truncate_path(&path, available_path_width));
    let directory_width = directory.as_deref().map_or(0, UnicodeWidthStr::width);
    let gap = usize::from(header_area.width)
        .saturating_sub(title_width + directory_width)
        .max(1);
    let mut line = Line::from(vec![title, Span::raw(" ".repeat(gap))]);
    if let Some(directory) = directory {
        line.push_span(Span::styled(directory, secondary_style()));
    }
    let title_area = Rect::new(header_area.x, header_area.y, header_area.width, 1);
    line.render(title_area, buffer);
}

fn compact_path(path: &Path) -> String {
    let display = path.to_string_lossy();
    let Some(home) = env::var_os("HOME") else {
        return display.into_owned();
    };
    let home = Path::new(&home);
    if let Ok(relative) = path.strip_prefix(home) {
        if relative.as_os_str().is_empty() {
            "~".to_owned()
        } else {
            format!("~/{}", relative.display())
        }
    } else if path.is_absolute() {
        let components = path
            .components()
            .filter_map(|component| match component {
                Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let suffix = components.iter().rev().take(2).cloned().collect::<Vec<_>>();
        if suffix.is_empty() {
            "…".to_owned()
        } else {
            format!(
                "…/{}",
                suffix.into_iter().rev().collect::<Vec<_>>().join("/")
            )
        }
    } else {
        display.into_owned()
    }
}

fn truncate_path(path: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(path) <= max_width {
        return path.to_owned();
    }
    if max_width <= 1 {
        return path.chars().take(max_width).collect::<String>();
    }

    let marker = "…/";
    let suffix_width = max_width.saturating_sub(UnicodeWidthStr::width(marker));
    let mut suffix = String::new();
    for component in path.split('/').rev() {
        let candidate = if suffix.is_empty() {
            component.to_owned()
        } else {
            format!("{component}/{suffix}")
        };
        if UnicodeWidthStr::width(candidate.as_str()) > suffix_width {
            break;
        }
        suffix = candidate;
    }
    if suffix.is_empty() {
        let mut width: usize = 0;
        for character in path.chars().rev() {
            let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
            if width.saturating_add(character_width) > suffix_width {
                break;
            }
            width += character_width;
            suffix.insert(0, character);
        }
    }
    format!("{marker}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_consts::COLOR_ACTION;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn renders_minimal_atlas_and_directory_header() {
        let mut terminal = Terminal::new(TestBackend::new(80, HEIGHT)).unwrap();
        let app = App::new("header".to_owned());
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let output = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(output.contains("Atlas"));
        assert!(output.contains(&compact_path(&env::current_dir().unwrap())));
        assert_eq!(terminal.backend().buffer()[(0, 0)].fg, COLOR_ACTION);
        assert_eq!(terminal.backend().buffer()[(0, 0)].fg, COLOR_ACTION);
        assert!(!output.contains("model:"));
        assert!(!output.contains("provider:"));
        assert!(!output.contains("unavailable"));
        assert!(!output.contains("v0.1.0"));
    }

    #[test]
    fn omits_header_when_terminal_is_too_narrow() {
        assert_eq!(desired_height(0), 0);
        assert_eq!(desired_height(1), HEIGHT);
    }

    #[test]
    fn compacts_home_and_long_paths() {
        let home = env::var("HOME").unwrap_or_else(|_| "/home/user".to_owned());
        assert_eq!(
            compact_path(&Path::new(&home).join("projetos/Atlas")),
            "~/projetos/Atlas"
        );
        assert_eq!(truncate_path("~/projetos/Atlas", 12), "…/Atlas");
    }
}
