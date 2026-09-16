use std::env;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::app::App;

pub(crate) const HEIGHT: u16 = 6;
const MAX_WIDTH: u16 = 72;

pub(crate) fn desired_height(width: u16) -> u16 {
    if width >= 24 { HEIGHT } else { 0 }
}

pub(crate) fn render(area: Rect, buffer: &mut Buffer, app: &App) {
    let width = area.width.min(MAX_WIDTH);
    let height = area.height.min(HEIGHT);
    if width < 4 || height < 3 {
        return;
    }

    let header_area = Rect::new(area.x, area.y, width, height);
    let directory = env::current_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "unavailable".to_owned());
    let title = Line::from(vec![
        Span::styled(">_ ", Style::default().dim()),
        Span::styled("Atlas", Style::default().bold()),
        Span::styled(
            format!(" (v{})", env!("CARGO_PKG_VERSION")),
            Style::default().dim(),
        ),
    ]);
    let directory = Line::from(vec![
        Span::styled("directory: ", Style::default().dim()),
        Span::raw(directory),
    ]);
    let model = app.session_model().unwrap_or("unavailable");
    let provider = app.session_provider().unwrap_or("unavailable");
    let model_and_provider = Line::from(vec![
        Span::styled("model: ", Style::default().dim()),
        Span::raw(model.to_owned()),
        Span::styled("  provider: ", Style::default().dim()),
        Span::raw(provider.to_owned()),
    ]);

    Clear.render(header_area, buffer);
    Paragraph::new(vec![title, Line::default(), model_and_provider, directory])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().dim()),
        )
        .render(header_area, buffer);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::RuntimeEvent;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn renders_atlas_version_and_directory_header() {
        let mut terminal = Terminal::new(TestBackend::new(80, HEIGHT)).unwrap();
        let mut app = App::new("header".to_owned());
        app.handle_runtime_event(RuntimeEvent::SessionUpdated {
            model: "gpt-5.6-luna".to_owned(),
            provider: "opencode-go".to_owned(),
        });
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
        assert!(output.contains("model: gpt-5.6-luna  provider: opencode-go"));
        assert!(output.contains("directory:"));
    }

    #[test]
    fn omits_header_when_terminal_is_too_narrow() {
        assert_eq!(desired_height(23), 0);
        assert_eq!(desired_height(24), HEIGHT);
    }
}
