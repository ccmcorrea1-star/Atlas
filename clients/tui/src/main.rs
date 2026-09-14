mod app;
mod composer;
mod event;
mod presentation;
mod runtime;
mod status;
mod transcript;
mod ui;
mod wrapping;

use std::io::{self, stdout};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use crossterm::{
    event::EnableMouseCapture,
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::task::JoinHandle;

use app::App;
use event::{Event, EventHandler};
use runtime::RuntimeClient;

#[derive(Debug, Parser)]
#[command(name = "atlas", version, about = "Atlas terminal client")]
struct Cli {
    /// Conversation ID reused by the Runtime for the current chat.
    #[arg(long)]
    conversation_id: Option<String>,
}

type AtlasTerminal = Terminal<CrosstermBackend<io::Stdout>>;

#[tokio::main]
async fn main() -> io::Result<()> {
    let cli = Cli::parse();
    let conversation_id = cli.conversation_id.unwrap_or_else(default_conversation_id);
    if conversation_id.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "conversation ID cannot be empty",
        ));
    }
    let (runtime, runtime_events) = RuntimeClient::new(conversation_id);
    let mut events = EventHandler::new(runtime_events);
    let mut app = App::new(runtime.conversation_id().to_owned());
    let mut terminal = TerminalGuard::new(setup_terminal()?);

    let result = run(&mut terminal.terminal, &mut app, runtime, &mut events).await;
    let restore_result = terminal.restore();
    result.and(restore_result)
}

async fn run(
    terminal: &mut AtlasTerminal,
    app: &mut App,
    runtime: RuntimeClient,
    events: &mut EventHandler,
) -> io::Result<()> {
    let mut send_task: Option<JoinHandle<()>> = None;
    terminal.draw(|frame| ui::draw(frame, app))?;
    while !app.should_quit() {
        let Some(event) = events.next().await else {
            break;
        };

        let should_redraw = match event {
            Event::Key(key) => {
                if let Some(task) = handle_key(app, &runtime, key) {
                    send_task = Some(task);
                }
                true
            }
            Event::Runtime(runtime_event) => {
                app.handle_runtime_event(runtime_event);
                true
            }
            Event::Mouse(mouse) => {
                handle_mouse(app, mouse);
                true
            }
            Event::Resize => true,
            Event::Tick => {
                app.tick();
                true
            }
        };

        if should_redraw && !app.should_quit() {
            terminal.draw(|frame| ui::draw(frame, app))?;
        }
    }

    if let Some(task) = send_task {
        task.abort();
    }
    Ok(())
}

fn handle_key(
    app: &mut App,
    runtime: &RuntimeClient,
    key: crossterm::event::KeyEvent,
) -> Option<JoinHandle<()>> {
    use crossterm::event::{KeyCode, KeyModifiers};

    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.quit();
        return None;
    }

    if key.code == KeyCode::Home && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.scroll_to_top();
        return None;
    }

    if key.code == KeyCode::End && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.scroll_to_bottom();
        return None;
    }

    if app.quit_confirmation() {
        match key.code {
            KeyCode::Char('y') => app.quit(),
            KeyCode::Char('n') | KeyCode::Esc => app.close_quit_confirmation(),
            _ => {}
        }
        return None;
    }

    if app.shortcuts_open() {
        if key.code == KeyCode::Esc {
            app.close_shortcuts();
        }
        return None;
    }

    match key.code {
        KeyCode::Esc if app.turn_active() => {
            app.open_quit_confirmation();
            None
        }
        KeyCode::Esc => {
            app.quit();
            None
        }
        KeyCode::Char(character)
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && character.eq_ignore_ascii_case(&'p') =>
        {
            app.open_shortcuts();
            None
        }
        KeyCode::Enter => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.insert_newline();
                None
            } else if !app.turn_active()
                && let Some(message) = app.submit_input()
            {
                let runtime = runtime.clone();
                Some(tokio::spawn(async move {
                    let _ = runtime.send_message(message).await;
                }))
            } else {
                None
            }
        }
        KeyCode::Backspace => {
            app.backspace();
            None
        }
        KeyCode::Delete => {
            app.delete_forward();
            None
        }
        KeyCode::Left => {
            app.move_cursor_left();
            None
        }
        KeyCode::Right => {
            app.move_cursor_right();
            None
        }
        KeyCode::Up => {
            app.move_cursor_up();
            None
        }
        KeyCode::Down => {
            app.move_cursor_down();
            None
        }
        KeyCode::Home => {
            app.move_cursor_home();
            None
        }
        KeyCode::End => {
            app.move_cursor_end();
            None
        }
        KeyCode::PageUp => {
            app.scroll_up(5);
            None
        }
        KeyCode::PageDown => {
            app.scroll_down(5);
            None
        }
        KeyCode::Char(character) => {
            app.insert_character(character);
            None
        }
        _ => None,
    }
}

fn default_conversation_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("atlas-tui-{}-{timestamp}", std::process::id())
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
    if let Err(error) = execute!(output, EnterAlternateScreen, EnableMouseCapture) {
        let _ = disable_raw_mode();
        let _ = execute!(
            output,
            LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture
        );
        return Err(error);
    }
    Terminal::new(CrosstermBackend::new(output))
}

struct TerminalGuard {
    terminal: AtlasTerminal,
    restored: bool,
}

impl TerminalGuard {
    fn new(terminal: AtlasTerminal) -> Self {
        Self {
            terminal,
            restored: false,
        }
    }

    fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }
        self.restored = true;
        restore_terminal(&mut self.terminal)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
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

#[cfg(test)]
mod tests {
    use super::handle_key;
    use crate::app::App;
    use crate::runtime::{RuntimeClient, RuntimeEventSender, RuntimeFuture, RuntimeTransport};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    struct NoopTransport;

    impl RuntimeTransport for NoopTransport {
        fn send_message(
            &self,
            _conversation_id: String,
            _input: String,
            _events: RuntimeEventSender,
        ) -> RuntimeFuture {
            Box::pin(async { Ok(()) })
        }
    }

    #[test]
    fn ctrl_p_opens_shortcuts_and_escape_only_closes_it() {
        let (runtime, _events) =
            RuntimeClient::with_transport("conversation".to_owned(), NoopTransport);
        let mut app = App::new("conversation".to_owned());

        let _ = handle_key(
            &mut app,
            &runtime,
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
        );
        assert!(app.shortcuts_open());

        let _ = handle_key(
            &mut app,
            &runtime,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert!(!app.shortcuts_open());
        assert!(!app.should_quit());
    }

    #[test]
    fn question_mark_is_inserted_when_the_composer_is_empty() {
        let (runtime, _events) =
            RuntimeClient::with_transport("conversation".to_owned(), NoopTransport);
        let mut app = App::new("conversation".to_owned());

        let _ = handle_key(
            &mut app,
            &runtime,
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::SHIFT),
        );

        assert_eq!(app.input(), "?");
        assert!(!app.shortcuts_open());
    }

    #[test]
    fn other_keys_do_not_close_the_shortcuts_overlay() {
        let (runtime, _events) =
            RuntimeClient::with_transport("conversation".to_owned(), NoopTransport);
        let mut app = App::new("conversation".to_owned());
        app.open_shortcuts();

        let _ = handle_key(
            &mut app,
            &runtime,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
        );

        assert!(app.shortcuts_open());
        assert!(app.input().is_empty());
    }

    #[test]
    fn escape_requests_confirmation_while_a_turn_is_active() {
        let (runtime, _events) =
            RuntimeClient::with_transport("conversation".to_owned(), NoopTransport);
        let mut app = App::new("conversation".to_owned());
        app.insert_character('h');
        app.submit_input();

        let _ = handle_key(
            &mut app,
            &runtime,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert!(app.quit_confirmation());
        assert!(!app.should_quit());

        let _ = handle_key(
            &mut app,
            &runtime,
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
        );
        assert!(!app.quit_confirmation());
    }

    #[test]
    fn ctrl_c_quits_even_when_the_shortcuts_overlay_is_open() {
        let (runtime, _events) =
            RuntimeClient::with_transport("conversation".to_owned(), NoopTransport);
        let mut app = App::new("conversation".to_owned());
        app.open_shortcuts();

        let _ = handle_key(
            &mut app,
            &runtime,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );

        assert!(app.should_quit());
    }

    #[test]
    fn ctrl_home_and_end_route_to_transcript_navigation() {
        let (runtime, _events) =
            RuntimeClient::with_transport("conversation".to_owned(), NoopTransport);
        let mut app = App::new("conversation".to_owned());

        let _ = handle_key(
            &mut app,
            &runtime,
            KeyEvent::new(KeyCode::Home, KeyModifiers::CONTROL),
        );
        assert!(app.history_scroll() > 0);

        let _ = handle_key(
            &mut app,
            &runtime,
            KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL),
        );
        assert_eq!(app.history_scroll(), 0);
    }
}
