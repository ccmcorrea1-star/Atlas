use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, Message, MessageRole};

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let areas = Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).split(frame.area());
    draw_history(frame, app, areas[0]);
    draw_input(frame, app, areas[1]);
}

fn draw_history(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let title = format!(
        " Atlas | {} | {} ",
        app.status_label(),
        app.conversation_id()
    );
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    let lines = app.messages().iter().map(message_line).collect::<Vec<_>>();
    let scroll = history_line_count(app, inner.width)
        .saturating_sub(inner.height as usize)
        .min(u16::MAX as usize) as u16;
    let history = Paragraph::new(lines)
        .block(block)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(history, area);
}

fn draw_input(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Input | Enter send | Esc quit ");
    let inner = block.inner(area);
    let input = Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Cyan)),
        Span::raw(app.input().to_owned()),
    ]);

    frame.render_widget(Paragraph::new(input).block(block), area);

    let cursor_x = inner
        .x
        .saturating_add(2)
        .saturating_add(app.cursor_position() as u16)
        .min(inner.x.saturating_add(inner.width.saturating_sub(1)));
    frame.set_cursor_position(Position::new(cursor_x, inner.y));
}

fn message_line(message: &Message) -> Line<'static> {
    let style = match message.role {
        MessageRole::User => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        MessageRole::Atlas => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        MessageRole::Tool => Style::default().fg(Color::Yellow),
        MessageRole::System => Style::default().fg(Color::Red),
    };
    Line::from(vec![
        Span::styled(format!("{}: ", message.role.label()), style),
        Span::raw(message.content.clone()),
    ])
}

fn history_line_count(app: &App, width: u16) -> usize {
    let width = usize::from(width.max(1));
    app.messages()
        .iter()
        .map(|message| {
            let text = format!("{}: {}", message.role.label(), message.content);
            text.split('\n')
                .map(|line| line.chars().count().max(1).div_ceil(width))
                .sum::<usize>()
        })
        .sum()
}
