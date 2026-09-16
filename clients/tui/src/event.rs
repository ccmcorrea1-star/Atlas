use std::time::Duration;

use crossterm::event;
use crossterm::event::Event as CrosstermEvent;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::MouseEvent;
use futures_core::Stream;
use ratatui::layout::Size;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::task::JoinHandle;

use crate::runtime::RuntimeEvent;
use crate::runtime::RuntimeEventReceiver;

#[derive(Debug)]
pub enum Event {
    Key(KeyEvent),
    Paste(String),
    Mouse(MouseEvent),
    Resize(Size),
    FocusGained,
    FocusLost,
    Tick,
    Runtime(RuntimeEvent),
}

pub struct EventHandler {
    receiver: Receiver<Event>,
    terminal_sender: Sender<Event>,
    terminal_task: JoinHandle<()>,
}

impl EventHandler {
    pub fn new(runtime_events: RuntimeEventReceiver) -> Self {
        let (sender, receiver) = mpsc::channel(256);
        let terminal_task = spawn_terminal_events(sender.clone());
        spawn_runtime_events(sender.clone(), runtime_events);
        Self {
            receiver,
            terminal_sender: sender,
            terminal_task,
        }
    }

    pub async fn next(&mut self) -> Option<Event> {
        self.receiver.recv().await
    }

    pub async fn pause_terminal(&mut self) {
        self.terminal_task.abort();
        let _ = (&mut self.terminal_task).await;
    }

    pub fn resume_terminal(&mut self) {
        self.terminal_task = spawn_terminal_events(self.terminal_sender.clone());
    }
}

fn spawn_terminal_events(sender: Sender<Event>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut stream = event::EventStream::new();
        let mut ticker = tokio::time::interval(Duration::from_millis(80));
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if sender.send(Event::Tick).await.is_err() {
                        break;
                    }
                }
                event = std::future::poll_fn(|context| {
                    std::pin::Pin::new(&mut stream).poll_next(context)
                }) => {
                    let Some(event) = event else { break };
                    let Ok(event) = event else { break };
                    let mapped = match event {
                        CrosstermEvent::Key(key)
                            if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                            Some(Event::Key(key))
                        }
                        CrosstermEvent::Resize(width, height) => Some(Event::Resize(Size { width, height })),
                        CrosstermEvent::Mouse(mouse) => Some(Event::Mouse(mouse)),
                        CrosstermEvent::Paste(text) => Some(Event::Paste(text)),
                        CrosstermEvent::FocusGained => Some(Event::FocusGained),
                        CrosstermEvent::FocusLost => Some(Event::FocusLost),
                        _ => None,
                    };
                    if let Some(event) = mapped
                        && sender.send(event).await.is_err()
                    {
                        break;
                    }
                }
            }
        }
    })
}

fn spawn_runtime_events(sender: Sender<Event>, mut runtime_events: RuntimeEventReceiver) {
    tokio::spawn(async move {
        while let Some(event) = runtime_events.recv().await {
            if sender.send(Event::Runtime(event)).await.is_err() {
                break;
            }
        }
    });
}
