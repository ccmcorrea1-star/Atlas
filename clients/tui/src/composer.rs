use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::wrapping::{cursor_position, wrap_text};

const PLACEHOLDER: &str = "Ask Atlas to do anything";

pub(crate) struct ComposerPanel;

impl ComposerPanel {
    pub(crate) fn height(app: &App, width: u16) -> u16 {
        let content_width = usize::from(width).saturating_sub(4).max(1);
        wrap_text(app.input(), content_width).len() as u16 + 2
    }

    pub(crate) fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
        if area.is_empty() {
            return;
        }
        let block = Block::default()
            .title(" Message ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let input_rows = inner.height;
        if input_rows == 0 {
            return;
        }

        let content_width = usize::from(inner.width).saturating_sub(2).max(1);
        let rows = wrap_text(app.input(), content_width);
        let first_visible = rows.len().saturating_sub(usize::from(input_rows));
        for (index, text) in rows.iter().skip(first_visible).enumerate() {
            let row = first_visible + index;
            let prompt = if row == 0 { "› " } else { "  " };
            let prompt_style = if row == 0 {
                Style::default().fg(Color::Cyan).bold()
            } else {
                Style::default().fg(Color::Cyan)
            };
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(prompt, prompt_style),
                    Span::raw(text.clone()),
                ])),
                Rect {
                    x: inner.x,
                    y: inner.y + index as u16,
                    width: inner.width,
                    height: 1,
                },
            );
        }

        if app.input().is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("› ", Style::default().fg(Color::Cyan).bold()),
                    Span::styled(PLACEHOLDER, Style::default().dim()),
                ])),
                Rect {
                    x: inner.x,
                    y: inner.y,
                    width: inner.width,
                    height: 1,
                },
            );
        }

        let (cursor_row, cursor_column) =
            cursor_position(app.input(), app.cursor_byte_position(), content_width);
        if cursor_row >= first_visible && cursor_row - first_visible < usize::from(input_rows) {
            let cursor_x = inner
                .x
                .saturating_add(2)
                .saturating_add(cursor_column as u16)
                .min(inner.x.saturating_add(inner.width.saturating_sub(1)));
            frame.set_cursor_position(Position::new(
                cursor_x,
                inner.y + (cursor_row - first_visible) as u16,
            ));
        }
    }
}
