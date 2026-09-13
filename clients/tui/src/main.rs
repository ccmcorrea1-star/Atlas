mod app;
mod event;
mod presentation;
mod runtime;
mod ui;

use std::io::{self, stdout};

use clap::Parser;
use crossterm::{
    event::EnableMouseCapture,
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use app::App;
use event::{Event, EventHandler};
use runtime::RuntimeClient;

#[derive(Debug, Parser)]
#[command(name = "atlas", version, about = "Atlas terminal client")]
struct Cli {
    /// Conversation ID reused by the Runtime for the current chat.
    #[arg(long, default_value = "atlas-tui")]
    conversation_id: String,
}

type AtlasTerminal = Terminal<CrosstermBackend<io::Stdout>>;

#[tokio::main]
async fn main() -> io::Result<()> {
    let cli = Cli::parse();
    let (runtime, runtime_events) = RuntimeClient::new(cli.conversation_id);
    let mut events = EventHandler::new(runtime_events);
    let mut app = App::new(runtime.conversation_id().to_owned());
    let mut terminal = setup_terminal()?;

    let result = run(&mut terminal, &mut app, runtime, &mut events).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn run(
    terminal: &mut AtlasTerminal,
    app: &mut App,
    runtime: RuntimeClient,
    events: &mut EventHandler,
) -> io::Result<()> {
    while !app.should_quit() {
        terminal.draw(|frame| ui::draw(frame, app))?;

        let Some(event) = events.next().await else {
            break;
        };

        match event {
            Event::Key(key) => handle_key(app, &runtime, key),
            Event::Runtime(runtime_event) => app.handle_runtime_event(runtime_event),
            Event::Mouse(mouse) => handle_mouse(app, mouse),
            Event::Tick => {}
            Event::Resize => {}
        }
    }

    Ok(())
}

fn handle_key(app: &mut App, runtime: &RuntimeClient, key: crossterm::event::KeyEvent) {
    use crossterm::event::{KeyCode, KeyModifiers};

    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => app.quit(),
        KeyCode::Esc => app.quit(),
        KeyCode::Enter => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.insert_newline();
            } else if !app.turn_active()
                && let Some(message) = app.submit_input()
            {
                let runtime = runtime.clone();
                let _send_task = tokio::spawn(async move {
                    let _ = runtime.send_message(message).await;
                });
            }
        }
        KeyCode::Backspace => app.backspace(),
        KeyCode::Delete => app.delete_forward(),
        KeyCode::Left => app.move_cursor_left(),
        KeyCode::Right => app.move_cursor_right(),
        KeyCode::Up => app.move_cursor_up(),
        KeyCode::Down => app.move_cursor_down(),
        KeyCode::Home => app.move_cursor_home(),
        KeyCode::End => app.move_cursor_end(),
        KeyCode::PageUp => app.scroll_up(5),
        KeyCode::PageDown => app.scroll_down(5),
        KeyCode::Char(character) => app.insert_character(character),
        _ => {}
    }
}

fn handle_mouse(app: &mut App, mouse: crossterm::event::MouseEvent) {
    use crossterm::event::MouseEventKind;

    match mouse.kind {
        MouseEventKind::ScrollUp => app.scroll_up(3),
        MouseEventKind::ScrollDown => app.scroll_down(3),
        _ => {}
    }
}

fn setup_terminal() -> io::Result<AtlasTerminal> {
    enable_raw_mode()?;
    let mut output = stdout();
    execute!(output, EnterAlternateScreen, EnableMouseCapture)?;
    Terminal::new(CrosstermBackend::new(output))
}

fn restore_terminal(terminal: &mut AtlasTerminal) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()
}
