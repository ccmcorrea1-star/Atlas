use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

// O vocabulário permanece pronto para o transporte público futuro.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    MessageDelta {
        message_id: String,
        delta: String,
    },
    ToolStarted {
        tool_name: String,
    },
    ToolCompleted {
        tool_name: String,
        output: Option<String>,
    },
    TurnStarted,
    TurnCompleted,
    Error {
        message: String,
    },
}

pub type RuntimeEventSender = UnboundedSender<RuntimeEvent>;
pub type RuntimeEventReceiver = UnboundedReceiver<RuntimeEvent>;
pub type RuntimeFuture = Pin<Box<dyn Future<Output = Result<(), RuntimeError>> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    TransportUnavailable,
    EventChannelClosed,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TransportUnavailable => write!(
                formatter,
                "Runtime transport is not configured; the public IPC/API is still pending"
            ),
            Self::EventChannelClosed => write!(formatter, "Runtime event channel was closed"),
        }
    }
}

impl std::error::Error for RuntimeError {}

/// Boundary for a future public Atlas API or IPC implementation.
pub trait RuntimeTransport: Send + Sync {
    fn send_message(
        &self,
        conversation_id: String,
        input: String,
        events: RuntimeEventSender,
    ) -> RuntimeFuture;
}

#[derive(Debug, Default)]
struct UnavailableTransport;

impl RuntimeTransport for UnavailableTransport {
    fn send_message(
        &self,
        _conversation_id: String,
        _input: String,
        _events: RuntimeEventSender,
    ) -> RuntimeFuture {
        Box::pin(async { Err(RuntimeError::TransportUnavailable) })
    }
}

#[derive(Clone)]
pub struct RuntimeClient {
    conversation_id: Arc<str>,
    transport: Arc<dyn RuntimeTransport>,
    event_sender: RuntimeEventSender,
}

impl RuntimeClient {
    pub fn new(conversation_id: String) -> (Self, RuntimeEventReceiver) {
        Self::with_transport(conversation_id, UnavailableTransport)
    }

    pub fn with_transport<T>(conversation_id: String, transport: T) -> (Self, RuntimeEventReceiver)
    where
        T: RuntimeTransport + 'static,
    {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        (
            Self {
                conversation_id: Arc::from(conversation_id),
                transport: Arc::new(transport),
                event_sender,
            },
            event_receiver,
        )
    }

    pub fn conversation_id(&self) -> &str {
        &self.conversation_id
    }

    pub async fn send_message(&self, input: String) -> Result<(), RuntimeError> {
        let result = self
            .transport
            .send_message(
                self.conversation_id.to_string(),
                input,
                self.event_sender.clone(),
            )
            .await;

        if let Err(error) = &result {
            self.event_sender
                .send(RuntimeEvent::Error {
                    message: error.to_string(),
                })
                .map_err(|_| RuntimeError::EventChannelClosed)?;
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::{RuntimeClient, RuntimeEvent, RuntimeEventSender, RuntimeFuture, RuntimeTransport};

    struct TestTransport;

    impl RuntimeTransport for TestTransport {
        fn send_message(
            &self,
            _conversation_id: String,
            _input: String,
            events: RuntimeEventSender,
        ) -> RuntimeFuture {
            Box::pin(async move {
                events
                    .send(RuntimeEvent::TurnStarted)
                    .map_err(|_| super::RuntimeError::EventChannelClosed)?;
                events
                    .send(RuntimeEvent::MessageDelta {
                        message_id: "message-1".to_owned(),
                        delta: "response".to_owned(),
                    })
                    .map_err(|_| super::RuntimeError::EventChannelClosed)?;
                events
                    .send(RuntimeEvent::TurnCompleted)
                    .map_err(|_| super::RuntimeError::EventChannelClosed)?;
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn keeps_the_conversation_id_at_the_client_boundary() {
        let (client, mut events) =
            RuntimeClient::with_transport("conversation-1".to_owned(), TestTransport);

        client.send_message("hello".to_owned()).await.unwrap();

        assert_eq!(client.conversation_id(), "conversation-1");
        assert!(matches!(
            events.recv().await,
            Some(RuntimeEvent::TurnStarted)
        ));
        assert!(matches!(
            events.recv().await,
            Some(RuntimeEvent::MessageDelta { .. })
        ));
        assert!(matches!(
            events.recv().await,
            Some(RuntimeEvent::TurnCompleted)
        ));
    }
}
