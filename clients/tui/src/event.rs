use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, KeyEventKind, MouseEvent};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::runtime::{RuntimeEvent, RuntimeEventReceiver};

#[derive(Debug)]
pub enum Event {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize,
    Runtime(RuntimeEvent),
}

pub struct EventHandler {
    receiver: UnboundedReceiver<Event>,
}

impl EventHandler {
    pub fn new(runtime_events: RuntimeEventReceiver) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        spawn_terminal_events(sender.clone());
        spawn_runtime_events(sender, runtime_events);
        Self { receiver }
    }

    pub async fn next(&mut self) -> Option<Event> {
        self.receiver.recv().await
    }
}

fn spawn_terminal_events(sender: UnboundedSender<Event>) {
    thread::spawn(move || {
        loop {
            match event::poll(Duration::from_millis(100)) {
                Ok(true) => match event::read() {
                    Ok(CrosstermEvent::Key(key)) if key.kind == KeyEventKind::Press => {
                        if sender.send(Event::Key(key)).is_err() {
                            break;
                        }
                    }
                    Ok(CrosstermEvent::Resize(_, _)) => {
                        if sender.send(Event::Resize).is_err() {
                            break;
                        }
                    }
                    Ok(CrosstermEvent::Mouse(mouse)) => {
                        if sender.send(Event::Mouse(mouse)).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                },
                Ok(false) => {}
                Err(_) => break,
            }
        }
    });
}

fn spawn_runtime_events(sender: UnboundedSender<Event>, mut runtime_events: RuntimeEventReceiver) {
    tokio::spawn(async move {
        while let Some(event) = runtime_events.recv().await {
            if sender.send(Event::Runtime(event)).is_err() {
                break;
            }
        }
    });
}
