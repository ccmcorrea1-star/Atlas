use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    MessageDelta {
        message_id: String,
        delta: String,
    },
    MessageCompleted {
        message_id: String,
        content: String,
    },
    ToolStarted {
        tool_id: String,
        tool_name: String,
    },
    ToolCompleted {
        tool_id: String,
        tool_name: String,
        output: Option<String>,
    },
    ExecutionStarted {
        execution_id: String,
        capability: String,
        program: String,
        args: Vec<String>,
        cwd: Option<String>,
        target: Option<String>,
    },
    ExecutionCompleted {
        execution_id: String,
        capability: String,
        stdout: String,
        stderr: String,
        exit_code: i32,
        duration_ms: u64,
        status: String,
    },
    TurnStarted,
    TurnCompleted {
        context: Option<ContextUsage>,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextUsage {
    pub used_tokens: u64,
    pub context_window: u64,
}

pub type RuntimeEventSender = UnboundedSender<RuntimeEvent>;
pub type RuntimeEventReceiver = UnboundedReceiver<RuntimeEvent>;
pub type RuntimeFuture = Pin<Box<dyn Future<Output = Result<(), RuntimeError>> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    Transport(String),
    Protocol(String),
    Remote(String),
    EventChannelClosed,
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(message) => write!(formatter, "Runtime transport error: {message}"),
            Self::Protocol(message) => write!(formatter, "Runtime protocol error: {message}"),
            Self::Remote(message) => write!(formatter, "Runtime error: {message}"),
            Self::EventChannelClosed => write!(formatter, "Runtime event channel was closed"),
        }
    }
}

impl std::error::Error for RuntimeError {}

pub trait RuntimeTransport: Send + Sync {
    fn send_message(
        &self,
        conversation_id: String,
        input: String,
        events: RuntimeEventSender,
    ) -> RuntimeFuture;
}

pub const DEFAULT_RUNTIME_SOCKET_PATH: &str = "/tmp/atlas-runtime.sock";

#[derive(Debug, Clone)]
pub struct UnixTransport {
    socket_path: Arc<str>,
}

impl UnixTransport {
    pub fn new(socket_path: impl Into<String>) -> Self {
        Self {
            socket_path: Arc::from(socket_path.into()),
        }
    }

    fn socket_path(&self) -> &str {
        &self.socket_path
    }
}

impl Default for UnixTransport {
    fn default() -> Self {
        Self::new(
            std::env::var("ATLAS_RUNTIME_SOCKET")
                .unwrap_or_else(|_| DEFAULT_RUNTIME_SOCKET_PATH.to_owned()),
        )
    }
}

impl RuntimeTransport for UnixTransport {
    fn send_message(
        &self,
        conversation_id: String,
        input: String,
        events: RuntimeEventSender,
    ) -> RuntimeFuture {
        let socket_path = self.socket_path().to_owned();
        Box::pin(async move { send_turn(&socket_path, &conversation_id, &input, events).await })
    }
}

#[derive(Debug, Serialize)]
struct TurnRequest<'a> {
    protocol: &'static str,
    version: u8,
    #[serde(rename = "type")]
    message_type: &'static str,
    request_id: String,
    conversation_id: &'a str,
    input: &'a str,
}

#[derive(Debug, Deserialize)]
struct RuntimeEnvelope {
    protocol: String,
    version: u8,
    #[serde(rename = "type")]
    message_type: String,
    request_id: Option<String>,
    #[allow(dead_code)]
    conversation_id: Option<String>,
    #[serde(default)]
    data: Value,
}

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

async fn send_turn(
    socket_path: &str,
    conversation_id: &str,
    input: &str,
    events: RuntimeEventSender,
) -> Result<(), RuntimeError> {
    let request_id = format!("tui-{}", NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed));
    let request = TurnRequest {
        protocol: "atlas-runtime",
        version: 1,
        message_type: "turn.request",
        request_id: request_id.clone(),
        conversation_id,
        input,
    };
    let mut stream = UnixStream::connect(socket_path)
        .await
        .map_err(|error| RuntimeError::Transport(error.to_string()))?;
    let mut payload =
        serde_json::to_vec(&request).map_err(|error| RuntimeError::Protocol(error.to_string()))?;
    payload.push(b'\n');
    stream
        .write_all(&payload)
        .await
        .map_err(|error| RuntimeError::Transport(error.to_string()))?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .await
            .map_err(|error| RuntimeError::Transport(error.to_string()))?;
        if bytes == 0 {
            return Err(RuntimeError::Transport(
                "Runtime closed the connection before completing the turn".to_owned(),
            ));
        }

        let envelope: RuntimeEnvelope = serde_json::from_str(line.trim_end())
            .map_err(|error| RuntimeError::Protocol(error.to_string()))?;
        if envelope.protocol != "atlas-runtime" || envelope.version != 1 {
            return Err(RuntimeError::Protocol(
                "unsupported Atlas Runtime protocol version".to_owned(),
            ));
        }
        if envelope.request_id.is_some()
            && envelope.request_id.as_deref() != Some(request_id.as_str())
        {
            continue;
        }

        match runtime_event(envelope)? {
            Some(RuntimeEvent::TurnCompleted { context }) => {
                events
                    .send(RuntimeEvent::TurnCompleted { context })
                    .map_err(|_| RuntimeError::EventChannelClosed)?;
                return Ok(());
            }
            Some(event) => events
                .send(event)
                .map_err(|_| RuntimeError::EventChannelClosed)?,
            None => {}
        }
    }
}

fn runtime_event(envelope: RuntimeEnvelope) -> Result<Option<RuntimeEvent>, RuntimeError> {
    if !matches!(
        envelope.message_type.as_str(),
        "turn.started"
            | "message.delta"
            | "message.completed"
            | "tool.started"
            | "tool.completed"
            | "execution.started"
            | "execution.completed"
            | "turn.completed"
            | "error"
    ) {
        return Ok(None);
    }

    let data = envelope
        .data
        .as_object()
        .ok_or_else(|| RuntimeError::Protocol("Runtime event data must be an object".to_owned()))?;
    let string_field = |field: &str| {
        data.get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                RuntimeError::Protocol(format!("Runtime event field {field} is missing"))
            })
    };
    let process_capability = || {
        let capability = string_field("capability")?;
        if capability != "process.exec" {
            return Err(RuntimeError::Protocol(
                "Runtime execution capability must be process.exec".to_owned(),
            ));
        }
        Ok(capability)
    };

    match envelope.message_type.as_str() {
        "turn.started" => Ok(Some(RuntimeEvent::TurnStarted)),
        "message.delta" => Ok(Some(RuntimeEvent::MessageDelta {
            message_id: string_field("message_id")?,
            delta: string_field("delta")?,
        })),
        "message.completed" => Ok(Some(RuntimeEvent::MessageCompleted {
            message_id: string_field("message_id")?,
            content: string_field("content")?,
        })),
        "tool.started" => Ok(Some(RuntimeEvent::ToolStarted {
            tool_id: string_field("tool_id")?,
            tool_name: string_field("name")?,
        })),
        "tool.completed" => Ok(Some(RuntimeEvent::ToolCompleted {
            tool_id: string_field("tool_id")?,
            tool_name: string_field("name")?,
            output: data.get("output").and_then(|value| match value {
                Value::String(text) => Some(text.clone()),
                Value::Null => None,
                value => serde_json::to_string(value).ok(),
            }),
        })),
        "execution.started" => Ok(Some(RuntimeEvent::ExecutionStarted {
            execution_id: string_field("execution_id")?,
            capability: process_capability()?,
            program: string_field("program")?,
            args: data
                .get("args")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    RuntimeError::Protocol("Runtime event field args must be an array".to_owned())
                })?
                .iter()
                .map(|argument| {
                    argument.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                        RuntimeError::Protocol(
                            "Runtime event field args must contain only strings".to_owned(),
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            cwd: data
                .get("cwd")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            target: data
                .get("target")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        })),
        "execution.completed" => Ok(Some(RuntimeEvent::ExecutionCompleted {
            execution_id: string_field("execution_id")?,
            capability: process_capability()?,
            stdout: data
                .get("stdout")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    RuntimeError::Protocol("Runtime event field stdout must be a string".to_owned())
                })?
                .to_owned(),
            stderr: data
                .get("stderr")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    RuntimeError::Protocol("Runtime event field stderr must be a string".to_owned())
                })?
                .to_owned(),
            exit_code: data
                .get("exit_code")
                .and_then(Value::as_i64)
                .and_then(|code| i32::try_from(code).ok())
                .ok_or_else(|| {
                    RuntimeError::Protocol(
                        "Runtime event field exit_code must be an integer".to_owned(),
                    )
                })?,
            duration_ms: data
                .get("duration_ms")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    RuntimeError::Protocol(
                        "Runtime event field duration_ms must be an unsigned integer".to_owned(),
                    )
                })?,
            status: string_field("status")?,
        })),
        "turn.completed" => Ok(Some(RuntimeEvent::TurnCompleted {
            context: data
                .get("context")
                .map(|value| -> Result<ContextUsage, RuntimeError> {
                    let context = value.as_object().ok_or_else(|| {
                        RuntimeError::Protocol(
                            "Runtime event field context must be an object".to_owned(),
                        )
                    })?;
                    let used_tokens = context
                        .get("used_tokens")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| {
                            RuntimeError::Protocol(
                                "Runtime context field used_tokens must be an unsigned integer"
                                    .to_owned(),
                            )
                        })?;
                    let context_window = context
                        .get("context_window")
                        .and_then(Value::as_u64)
                        .filter(|value| *value > 0)
                        .ok_or_else(|| {
                            RuntimeError::Protocol(
                                "Runtime context field context_window must be a positive unsigned integer"
                                    .to_owned(),
                            )
                        })?;
                    Ok(ContextUsage {
                        used_tokens,
                        context_window,
                    })
                })
                .transpose()?,
        })),
        "error" => Err(RuntimeError::Remote(string_field("message")?)),
        _ => Ok(None),
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
        Self::with_transport(conversation_id, UnixTransport::default())
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
    use super::{
        RuntimeClient, RuntimeEnvelope, RuntimeEvent, RuntimeEventSender, RuntimeFuture,
        RuntimeTransport, runtime_event,
    };
    use serde_json::Value;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

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
                    .send(RuntimeEvent::TurnCompleted { context: None })
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
            Some(RuntimeEvent::TurnCompleted { context: None })
        ));
    }

    #[test]
    fn parses_tool_identity_and_structured_output() {
        let envelope = RuntimeEnvelope {
            protocol: "atlas-runtime".to_owned(),
            version: 1,
            message_type: "tool.completed".to_owned(),
            request_id: None,
            conversation_id: None,
            data: serde_json::json!({
                "tool_id": "tool-1",
                "name": "process.exec",
                "output": {"stdout": "ready", "exit_code": 0}
            }),
        };

        assert_eq!(
            runtime_event(envelope).unwrap(),
            Some(RuntimeEvent::ToolCompleted {
                tool_id: "tool-1".to_owned(),
                tool_name: "process.exec".to_owned(),
                output: Some(r#"{"exit_code":0,"stdout":"ready"}"#.to_owned()),
            })
        );
    }

    #[test]
    fn parses_process_execution_lifecycle_fields() {
        let started = RuntimeEnvelope {
            protocol: "atlas-runtime".to_owned(),
            version: 1,
            message_type: "execution.started".to_owned(),
            request_id: None,
            conversation_id: None,
            data: serde_json::json!({
                "execution_id": "call-1",
                "capability": "process.exec",
                "program": "node",
                "args": ["--version"],
                "cwd": "/tmp",
                "target": "local"
            }),
        };
        assert_eq!(
            runtime_event(started).unwrap(),
            Some(RuntimeEvent::ExecutionStarted {
                execution_id: "call-1".to_owned(),
                capability: "process.exec".to_owned(),
                program: "node".to_owned(),
                args: vec!["--version".to_owned()],
                cwd: Some("/tmp".to_owned()),
                target: Some("local".to_owned()),
            })
        );

        let completed = RuntimeEnvelope {
            protocol: "atlas-runtime".to_owned(),
            version: 1,
            message_type: "execution.completed".to_owned(),
            request_id: None,
            conversation_id: None,
            data: serde_json::json!({
                "execution_id": "call-1",
                "capability": "process.exec",
                "stdout": "v22.x.x",
                "stderr": "",
                "exit_code": 0,
                "duration_ms": 120,
                "status": "success"
            }),
        };
        assert_eq!(
            runtime_event(completed).unwrap(),
            Some(RuntimeEvent::ExecutionCompleted {
                execution_id: "call-1".to_owned(),
                capability: "process.exec".to_owned(),
                stdout: "v22.x.x".to_owned(),
                stderr: String::new(),
                exit_code: 0,
                duration_ms: 120,
                status: "success".to_owned(),
            })
        );
    }

    #[test]
    fn parses_context_usage_from_a_completed_turn() {
        let envelope = RuntimeEnvelope {
            protocol: "atlas-runtime".to_owned(),
            version: 1,
            message_type: "turn.completed".to_owned(),
            request_id: None,
            conversation_id: None,
            data: serde_json::json!({
                "content": "done",
                "context": {"used_tokens": 6600, "context_window": 256000}
            }),
        };

        assert_eq!(
            runtime_event(envelope).unwrap(),
            Some(RuntimeEvent::TurnCompleted {
                context: Some(super::ContextUsage {
                    used_tokens: 6600,
                    context_window: 256000,
                }),
            })
        );
    }

    #[tokio::test]
    async fn sends_a_turn_over_the_public_unix_protocol() {
        let socket_path = std::env::temp_dir().join(format!(
            "atlas-tui-runtime-{}-{}.sock",
            std::process::id(),
            super::NEXT_REQUEST_ID.fetch_add(1, super::Ordering::Relaxed)
        ));
        let listener = UnixListener::bind(&socket_path).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut request_line = String::new();
            reader.read_line(&mut request_line).await.unwrap();
            let request: Value = serde_json::from_str(request_line.trim()).unwrap();
            assert_eq!(request["type"], "turn.request");
            assert_eq!(request["conversation_id"], "conversation-1");
            assert_eq!(request["input"], "node --version");
            let request_id = request["request_id"].as_str().unwrap();
            let events = [
                serde_json::json!({
                    "protocol": "atlas-runtime",
                    "version": 1,
                    "type": "turn.started",
                    "request_id": request_id,
                    "conversation_id": "conversation-1",
                    "data": {}
                }),
                serde_json::json!({
                    "protocol": "atlas-runtime",
                    "version": 1,
                    "type": "message.completed",
                    "request_id": request_id,
                    "conversation_id": "conversation-1",
                    "data": {"message_id": "message-1", "content": "v22.x.x"}
                }),
                serde_json::json!({
                    "protocol": "atlas-runtime",
                    "version": 1,
                    "type": "future.event",
                    "request_id": request_id,
                    "conversation_id": "conversation-1",
                    "data": ["future"]
                }),
                serde_json::json!({
                    "protocol": "atlas-runtime",
                    "version": 1,
                    "type": "turn.completed",
                    "request_id": request_id,
                    "conversation_id": "conversation-1",
                    "data": {"content": "v22.x.x"}
                }),
            ];
            let mut stream = reader.into_inner();
            for event in events {
                let mut line = serde_json::to_vec(&event).unwrap();
                line.push(b'\n');
                stream.write_all(&line).await.unwrap();
            }
        });

        let (client, mut events) = RuntimeClient::with_transport(
            "conversation-1".to_owned(),
            super::UnixTransport::new(socket_path.to_string_lossy().to_string()),
        );
        client
            .send_message("node --version".to_owned())
            .await
            .unwrap();
        assert!(matches!(
            events.recv().await,
            Some(RuntimeEvent::TurnStarted)
        ));
        assert!(matches!(
            events.recv().await,
            Some(RuntimeEvent::MessageCompleted { .. })
        ));
        assert!(matches!(
            events.recv().await,
            Some(RuntimeEvent::TurnCompleted { context: None })
        ));
        server.await.unwrap();
        std::fs::remove_file(socket_path).unwrap();
    }
}
