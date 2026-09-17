mod app;
mod bottom_pane;
mod chatwidget;
#[allow(dead_code)]
mod custom_terminal;
mod event;
mod exec_cell;
mod external_editor;
mod file_search;
mod history_cell;
mod history_store;
mod keymap;
mod markdown;
mod markdown_render;
mod pager_overlay;
mod render;
mod runtime;
mod server;
mod session_header;
mod table_detect;
mod ui_consts;
mod wrapping;

use std::error::Error;
use std::io::stdout;
use std::time::Instant;

use clap::Parser;
use clap::Subcommand;
use crossterm::cursor::Hide;
use crossterm::cursor::Show;
use crossterm::event::DisableBracketedPaste;
use crossterm::event::DisableMouseCapture;
use crossterm::event::EnableBracketedPaste;
use crossterm::event::EnableMouseCapture;
use crossterm::execute;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Position;
use tokio::task::JoinHandle;

use crate::app::App;
use crate::bottom_pane::ActiveBottomPaneView;
use crate::bottom_pane::BottomPaneView;
use crate::event::Event;
use crate::event::EventHandler;
use crate::runtime::AtlasRuntimeClient;
use crate::runtime::RuntimeError;
use crate::runtime::UnixTransport;

#[derive(Debug, Parser)]
#[command(name = "atlas", version, about = "Atlas terminal client")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Identificador da conversa reutilizado em todos os turnos deste processo.
    #[arg(long, default_value = "default")]
    conversation_id: String,

    /// Substitui o caminho do Unix Socket do Atlas Runtime.
    #[arg(long, env = "ATLAS_RUNTIME_SOCKET")]
    socket: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Gerencia o servidor do Atlas Runtime.
    Server {
        #[command(subcommand)]
        command: server::ServerCommand,
    },
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self, Box<dyn Error>> {
        enable_raw_mode()?;
        execute!(
            stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste,
            Hide
        )?;
        Ok(Self)
    }
    fn suspend(&self) -> Result<(), Box<dyn Error>> {
        disable_raw_mode()?;
        execute!(
            stdout(),
            Show,
            DisableBracketedPaste,
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        Ok(())
    }

    fn resume(&self) -> Result<(), Box<dyn Error>> {
        enable_raw_mode()?;
        execute!(
            stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste,
            Hide
        )?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            stdout(),
            Show,
            DisableBracketedPaste,
            LeaveAlternateScreen,
            DisableMouseCapture
        );
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    if let Some(Command::Server { command }) = cli.command {
        return server::execute(command, cli.socket.as_deref());
    }

    let (runtime, runtime_events) = match cli.socket {
        Some(socket) => {
            AtlasRuntimeClient::with_transport(cli.conversation_id, UnixTransport::new(socket))
        }
        None => AtlasRuntimeClient::new(cli.conversation_id),
    };
    run(runtime, runtime_events).await
}

async fn run(
    runtime: AtlasRuntimeClient,
    runtime_events: runtime::RuntimeEventReceiver,
) -> Result<(), Box<dyn Error>> {
    let _terminal_guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = custom_terminal::Terminal::with_options(backend)?;
    let size = terminal.size()?;
    terminal.set_viewport_area(ratatui::layout::Rect::new(0, 0, size.width, size.height));
    terminal.clear()?;

    let mut app = App::new(runtime.conversation_id().to_owned());
    let mut bottom_pane = ActiveBottomPaneView::new();
    let mut events = EventHandler::new(runtime_events);
    let mut active_send: Option<JoinHandle<Result<(), RuntimeError>>> = None;

    while !app.should_quit() {
        let _ = bottom_pane.sync(&app);
        terminal.draw(|frame| {
            if let Some(position) = chatwidget::rendering::render(
                frame.area(),
                frame.buffer_mut(),
                &mut app,
                &bottom_pane,
            ) {
                frame.set_cursor_position(Position::new(position.0, position.1));
            }
        })?;
        let Some(event) = events.next().await else {
            break;
        };

        match event {
            Event::Key(key) => {
                let handled_globally = app.handle_global_key(key);
                if app.take_cancel_requested() {
                    let cancel_runtime = runtime.clone();
                    tokio::spawn(async move {
                        let _ = cancel_runtime.cancel_turn().await;
                    });
                }
                if app.take_external_editor_requested() {
                    let editor_command = match external_editor::resolve_editor_command() {
                        Ok(command) => command,
                        Err(error) => {
                            eprintln!("Failed to resolve external editor: {error}");
                            continue;
                        }
                    };
                    let draft = app.input().to_owned();
                    events.pause_terminal().await;
                    if let Err(error) = _terminal_guard.suspend() {
                        events.resume_terminal();
                        return Err(error);
                    }
                    let edited = external_editor::run_editor(&draft, &editor_command).await;
                    let resume_result = _terminal_guard.resume();
                    events.resume_terminal();
                    resume_result?;
                    terminal.clear()?;
                    match edited {
                        Ok(text) => app.apply_external_editor_text(&text),
                        Err(error) => eprintln!("Failed to open external editor: {error}"),
                    }
                }
                if !handled_globally
                    && let Some(input) = bottom_pane.handle_key_event(&mut app, key)
                {
                    active_send = Some(spawn_send(runtime.clone(), input));
                }
            }
            Event::Paste(text) => {
                bottom_pane.handle_paste(&mut app, &text);
            }
            Event::Mouse(mouse) => {
                bottom_pane.handle_mouse_event(&mut app, mouse);
            }
            Event::Resize(size) => {
                app.on_resize();
                let screen_size = ratatui::layout::Size::new(size.width, size.height);
                terminal.resize(screen_size)?;
                terminal.set_viewport_area(ratatui::layout::Rect::new(
                    0,
                    0,
                    size.width,
                    size.height,
                ));
                terminal.clear()?;
            }
            Event::FocusGained | Event::FocusLost => {}
            Event::Tick => {
                app.tick();
                bottom_pane.pre_draw_tick(&mut app, Instant::now());
            }
            Event::Runtime(runtime_event) => {
                let completed = runtime_event.is_terminal();
                app.handle_runtime_event(runtime_event);
                if completed {
                    if let Some(input) = app.take_queued_input() {
                        active_send = Some(spawn_send(runtime.clone(), input));
                    } else {
                        active_send = None;
                    }
                }
            }
        }
    }

    if let Some(handle) = active_send {
        handle.abort();
    }
    Ok(())
}

fn spawn_send(runtime: AtlasRuntimeClient, input: String) -> JoinHandle<Result<(), RuntimeError>> {
    tokio::spawn(async move { runtime.send_message(input).await })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_server_subcommands() {
        let run = Cli::try_parse_from(["atlas", "server", "run"]).unwrap();
        assert!(matches!(
            run.command,
            Some(Command::Server {
                command: server::ServerCommand::Run
            })
        ));

        let stop = Cli::try_parse_from(["atlas", "server", "stop"]).unwrap();
        assert!(matches!(
            stop.command,
            Some(Command::Server {
                command: server::ServerCommand::Stop
            })
        ));

        let restart = Cli::try_parse_from(["atlas", "server", "restart"]).unwrap();
        assert!(matches!(
            restart.command,
            Some(Command::Server {
                command: server::ServerCommand::Restart
            })
        ));
    }
}
