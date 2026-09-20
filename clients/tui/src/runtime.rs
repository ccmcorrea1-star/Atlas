//! Cliente enxuto do Atlas Runtime Protocol v1.
//!
//! Este modulo e o unico ponto da TUI que conhece IPC ou JSON. As cells recebem
//! eventos publicos ja validados e nunca veem dados do provider.

use std::fmt;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use tokio::io::AsyncBufRead;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::UnixStream;
use tokio::sync::mpsc::{self, Receiver, Sender};

const PROTOCOL: &str = "atlas-runtime";
const VERSION: u8 = 1;
const EVENT_CHANNEL_CAPACITY: usize = 256;
const MAX_RUNTIME_FRAME_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_RUNTIME_SOCKET_PATH: &str = "/tmp/atlas-runtime.sock";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    SessionUpdated {
        model: String,
        provider: String,
    },
    TurnStarted,
    ContextUpdated {
        context: ContextUsage,
    },
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
        target: Option<String>,
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
    ExecutionOutputDelta {
        execution_id: String,
        capability: String,
        channel: String,
        delta: String,
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
    TurnCompleted {
        content: String,
        message_id: Option<String>,
        context: Option<ContextUsage>,
    },
    TurnCancelled {
        content: String,
        message_id: Option<String>,
    },
    Error {
        message: String,
    },
}

impl RuntimeEvent {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::TurnCompleted { .. } | Self::TurnCancelled { .. } | Self::Error { .. }
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextUsage {
    pub used_tokens: u64,
    pub context_window: u64,
}

pub type RuntimeEventReceiver = Receiver<RuntimeEvent>;
pub type RuntimeEventSender = Sender<RuntimeEvent>;
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
        request_id: String,
        conversation_id: String,
        input: String,
        events: RuntimeEventSender,
    ) -> RuntimeFuture;

    fn cancel_turn(&self, request_id: String, conversation_id: String) -> RuntimeFuture;
}

fn default_runtime_socket_path() -> String {
    std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(|path| {
            Path::new(&path)
                .join("atlas-runtime.sock")
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_else(|| DEFAULT_RUNTIME_SOCKET_PATH.to_owned())
}

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
}

impl Default for UnixTransport {
    fn default() -> Self {
        Self::new(
            std::env::var("ATLAS_RUNTIME_SOCKET").unwrap_or_else(|_| default_runtime_socket_path()),
        )
    }
}

impl RuntimeTransport for UnixTransport {
    fn send_message(
        &self,
        request_id: String,
        conversation_id: String,
        input: String,
        events: RuntimeEventSender,
    ) -> RuntimeFuture {
        let socket_path = self.socket_path.to_string();
        Box::pin(async move {
            send_turn(&socket_path, &request_id, &conversation_id, &input, events).await
        })
    }

    fn cancel_turn(&self, request_id: String, conversation_id: String) -> RuntimeFuture {
        let socket_path = self.socket_path.to_string();
        Box::pin(async move { send_cancel(&socket_path, &request_id, &conversation_id).await })
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

#[derive(Debug, Serialize)]
struct TurnCancelRequest<'a> {
    protocol: &'static str,
    version: u8,
    #[serde(rename = "type")]
    message_type: &'static str,
    request_id: &'a str,
    conversation_id: &'a str,
}

#[derive(Debug, Deserialize)]
struct RuntimeEnvelope {
    protocol: String,
    version: u8,
    #[serde(rename = "type")]
    message_type: String,
    request_id: Option<String>,
    conversation_id: Option<String>,
    #[serde(default)]
    data: Value,
}

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

async fn send_turn(
    socket_path: &str,
    request_id: &str,
    conversation_id: &str,
    input: &str,
    events: RuntimeEventSender,
) -> Result<(), RuntimeError> {
    let request = TurnRequest {
        protocol: PROTOCOL,
        version: VERSION,
        message_type: "turn.request",
        request_id: request_id.to_owned(),
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
    loop {
        let line = read_runtime_frame(&mut reader).await?;
        let envelope: RuntimeEnvelope = serde_json::from_str(line.trim_end())
            .map_err(|error| RuntimeError::Protocol(error.to_string()))?;
        if envelope.protocol != PROTOCOL || envelope.version != VERSION {
            return Err(RuntimeError::Protocol(
                "unsupported Atlas Runtime protocol version".to_owned(),
            ));
        }
        if envelope.request_id.as_deref() != Some(request_id) {
            return Err(RuntimeError::Protocol(
                "Runtime event request_id does not match the active turn".to_owned(),
            ));
        }
        if envelope.conversation_id.as_deref() != Some(conversation_id) {
            return Err(RuntimeError::Protocol(
                "Runtime event conversation_id does not match the active conversation".to_owned(),
            ));
        }

        match parse_runtime_event(envelope)? {
            Some(RuntimeEvent::TurnCompleted {
                content,
                message_id,
                context,
            }) => {
                events
                    .send(RuntimeEvent::TurnCompleted {
                        content,
                        message_id,
                        context,
                    })
                    .await
                    .map_err(|_| RuntimeError::EventChannelClosed)?;
                return Ok(());
            }
            Some(RuntimeEvent::TurnCancelled {
                content,
                message_id,
            }) => {
                events
                    .send(RuntimeEvent::TurnCancelled {
                        content,
                        message_id,
                    })
                    .await
                    .map_err(|_| RuntimeError::EventChannelClosed)?;
                return Ok(());
            }
            Some(event) => events
                .send(event)
                .await
                .map_err(|_| RuntimeError::EventChannelClosed)?,
            None => {}
        }
    }
}

async fn send_cancel(
    socket_path: &str,
    request_id: &str,
    conversation_id: &str,
) -> Result<(), RuntimeError> {
    let request = TurnCancelRequest {
        protocol: PROTOCOL,
        version: VERSION,
        message_type: "turn.cancel",
        request_id,
        conversation_id,
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
        .map_err(|error| RuntimeError::Transport(error.to_string()))
}

async fn read_runtime_frame<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<String, RuntimeError> {
    let mut bytes = Vec::new();
    loop {
        let buffer = reader
            .fill_buf()
            .await
            .map_err(|error| RuntimeError::Transport(error.to_string()))?;
        if buffer.is_empty() {
            return Err(RuntimeError::Transport(
                "Runtime closed the connection before completing the turn".to_owned(),
            ));
        }
        let length = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(buffer.len(), |index| index + 1);
        let has_newline = buffer[..length].last() == Some(&b'\n');
        if bytes.len().saturating_add(length) > MAX_RUNTIME_FRAME_BYTES {
            return Err(RuntimeError::Protocol(
                "Runtime event exceeds the maximum frame size".to_owned(),
            ));
        }
        bytes.extend_from_slice(&buffer[..length]);
        reader.consume(length);
        if has_newline {
            return String::from_utf8(bytes)
                .map_err(|error| RuntimeError::Protocol(error.to_string()));
        }
    }
}

fn event_object(data: Value) -> Result<serde_json::Map<String, Value>, RuntimeError> {
    data.as_object()
        .cloned()
        .ok_or_else(|| RuntimeError::Protocol("Runtime event data must be an object".to_owned()))
}

fn required_string(
    data: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<String, RuntimeError> {
    data.get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| RuntimeError::Protocol(format!("Runtime event field {field} is missing")))
}

fn text_string(data: &serde_json::Map<String, Value>, field: &str) -> Result<String, RuntimeError> {
    data.get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            RuntimeError::Protocol(format!("Runtime event field {field} must be a string"))
        })
}

fn optional_context(
    data: &serde_json::Map<String, Value>,
) -> Result<Option<ContextUsage>, RuntimeError> {
    data.get("context")
        .map(|value| {
            let context = value.as_object().ok_or_else(|| {
                RuntimeError::Protocol("Runtime context must be an object".to_owned())
            })?;
            let used_tokens = context
                .get("used_tokens")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    RuntimeError::Protocol("Runtime context used_tokens is invalid".to_owned())
                })?;
            let context_window = context
                .get("context_window")
                .and_then(Value::as_u64)
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    RuntimeError::Protocol("Runtime context_window is invalid".to_owned())
                })?;
            Ok(ContextUsage {
                used_tokens,
                context_window,
            })
        })
        .transpose()
}

fn parse_runtime_event(envelope: RuntimeEnvelope) -> Result<Option<RuntimeEvent>, RuntimeError> {
    let known = matches!(
        envelope.message_type.as_str(),
        "session.updated"
            | "turn.started"
            | "context.updated"
            | "message.delta"
            | "message.completed"
            | "tool.started"
            | "tool.completed"
            | "execution.started"
            | "execution.output.delta"
            | "execution.completed"
            | "turn.completed"
            | "turn.cancelled"
            | "error"
    );
    if !known {
        return Ok(None);
    }
    let data = event_object(envelope.data)?;
    match envelope.message_type.as_str() {
        "session.updated" => Ok(Some(RuntimeEvent::SessionUpdated {
            model: required_string(&data, "model")?,
            provider: required_string(&data, "provider")?,
        })),
        "turn.started" => Ok(Some(RuntimeEvent::TurnStarted)),
        "context.updated" => Ok(Some(RuntimeEvent::ContextUpdated {
            context: required_context(&data)?,
        })),
        "message.delta" => Ok(Some(RuntimeEvent::MessageDelta {
            message_id: required_string(&data, "message_id")?,
            delta: text_string(&data, "delta")?,
        })),
        "message.completed" => Ok(Some(RuntimeEvent::MessageCompleted {
            message_id: required_string(&data, "message_id")?,
            content: text_string(&data, "content")?,
        })),
        "tool.started" => Ok(Some(RuntimeEvent::ToolStarted {
            tool_id: required_string(&data, "tool_id")?,
            tool_name: required_string(&data, "name")?,
            target: data
                .get("target")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        })),
        "tool.completed" => Ok(Some(RuntimeEvent::ToolCompleted {
            tool_id: required_string(&data, "tool_id")?,
            tool_name: required_string(&data, "name")?,
            output: data
                .get("output")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        })),
        "execution.started" => {
            let capability = required_string(&data, "capability")?;
            if capability != "shell.exec" {
                return Err(RuntimeError::Protocol(
                    "Runtime execution capability must be shell.exec".to_owned(),
                ));
            }
            let args = data
                .get("args")
                .and_then(Value::as_array)
                .ok_or_else(|| RuntimeError::Protocol("Runtime args must be an array".to_owned()))?
                .iter()
                .map(|argument| {
                    argument.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                        RuntimeError::Protocol("Runtime args must contain only strings".to_owned())
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Some(RuntimeEvent::ExecutionStarted {
                execution_id: required_string(&data, "execution_id")?,
                capability,
                program: required_string(&data, "program")?,
                args,
                cwd: data
                    .get("cwd")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                target: data
                    .get("target")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            }))
        }
        "execution.output.delta" => {
            let capability = required_string(&data, "capability")?;
            if capability != "shell.exec" {
                return Err(RuntimeError::Protocol(
                    "Runtime execution capability must be shell.exec".to_owned(),
                ));
            }
            let channel = required_string(&data, "channel")?;
            if channel != "stdout" && channel != "stderr" {
                return Err(RuntimeError::Protocol(
                    "Runtime execution output channel must be stdout or stderr".to_owned(),
                ));
            }
            Ok(Some(RuntimeEvent::ExecutionOutputDelta {
                execution_id: required_string(&data, "execution_id")?,
                capability,
                channel,
                delta: text_string(&data, "delta")?,
            }))
        }
        "execution.completed" => {
            let capability = required_string(&data, "capability")?;
            if capability != "shell.exec" {
                return Err(RuntimeError::Protocol(
                    "Runtime execution capability must be shell.exec".to_owned(),
                ));
            }
            let exit_code = data
                .get("exit_code")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| RuntimeError::Protocol("Runtime exit_code is invalid".to_owned()))?;
            let duration_ms = data
                .get("duration_ms")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    RuntimeError::Protocol("Runtime duration_ms is invalid".to_owned())
                })?;
            Ok(Some(RuntimeEvent::ExecutionCompleted {
                execution_id: required_string(&data, "execution_id")?,
                capability,
                stdout: text_string(&data, "stdout")?,
                stderr: text_string(&data, "stderr")?,
                exit_code,
                duration_ms,
                status: required_string(&data, "status")?,
            }))
        }
        "turn.completed" => Ok(Some(RuntimeEvent::TurnCompleted {
            content: text_string(&data, "content")?,
            message_id: data
                .get("message_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            context: optional_context(&data)?,
        })),
        "turn.cancelled" => Ok(Some(RuntimeEvent::TurnCancelled {
            content: text_string(&data, "content")?,
            message_id: data
                .get("message_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        })),
        "error" => Err(RuntimeError::Remote(required_string(&data, "message")?)),
        _ => Ok(None),
    }
}

fn required_context(data: &serde_json::Map<String, Value>) -> Result<ContextUsage, RuntimeError> {
    let used_tokens = data
        .get("used_tokens")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            RuntimeError::Protocol("Runtime context used_tokens is invalid".to_owned())
        })?;
    let context_window = data
        .get("context_window")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or_else(|| RuntimeError::Protocol("Runtime context_window is invalid".to_owned()))?;
    Ok(ContextUsage {
        used_tokens,
        context_window,
    })
}

#[derive(Clone)]
/// Fronteira de backend do modelo de apresentacao da TUI.
///
/// A UI nunca consome JSON ou estado especifico de provider. Este adaptador
/// transforma frames do Runtime Protocol v1 no vocabulario usado por
/// `ChatWidget`, history cells e execution cells.
pub struct AtlasRuntimeClient {
    conversation_id: Arc<str>,
    transport: Arc<dyn RuntimeTransport>,
    event_sender: RuntimeEventSender,
    active_request_id: Arc<Mutex<Option<String>>>,
}

impl AtlasRuntimeClient {
    pub fn new(conversation_id: String) -> (Self, RuntimeEventReceiver) {
        Self::with_transport(conversation_id, UnixTransport::default())
    }

    pub fn with_transport<T>(conversation_id: String, transport: T) -> (Self, RuntimeEventReceiver)
    where
        T: RuntimeTransport + 'static,
    {
        let (event_sender, event_receiver) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        (
            Self {
                conversation_id: Arc::from(conversation_id),
                transport: Arc::new(transport),
                event_sender,
                active_request_id: Arc::new(Mutex::new(None)),
            },
            event_receiver,
        )
    }

    pub fn conversation_id(&self) -> &str {
        &self.conversation_id
    }

    pub async fn send_message(&self, input: String) -> Result<(), RuntimeError> {
        let request_id = format!("tui-{}", NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed));
        {
            let mut active = self
                .active_request_id
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *active = Some(request_id.clone());
        }
        let result = self
            .transport
            .send_message(
                request_id.clone(),
                self.conversation_id.to_string(),
                input,
                self.event_sender.clone(),
            )
            .await;
        {
            let mut active = self
                .active_request_id
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if active.as_deref() == Some(request_id.as_str()) {
                *active = None;
            }
        }
        if let Err(error) = &result {
            self.event_sender
                .send(RuntimeEvent::Error {
                    message: error.to_string(),
                })
                .await
                .map_err(|_| RuntimeError::EventChannelClosed)?;
        }
        result
    }

    pub async fn cancel_turn(&self) -> Result<(), RuntimeError> {
        let request_id = self
            .active_request_id
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or_else(|| RuntimeError::Remote("No active turn to cancel".to_owned()))?;
        self.transport
            .cancel_turn(request_id, self.conversation_id.to_string())
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn envelope(message_type: &str, data: Value) -> RuntimeEnvelope {
        RuntimeEnvelope {
            protocol: PROTOCOL.to_owned(),
            version: VERSION,
            message_type: message_type.to_owned(),
            request_id: Some("request".to_owned()),
            conversation_id: Some("conversation".to_owned()),
            data,
        }
    }

    #[test]
    fn parses_tool_lifecycle_payload_without_dropping_identity_or_output() {
        let started = parse_runtime_event(envelope(
            "tool.started",
            json!({"tool_id": "tool-1", "name": "search", "target": "src"}),
        ))
        .expect("tool.started should parse")
        .expect("known event");
        assert_eq!(
            started,
            RuntimeEvent::ToolStarted {
                tool_id: "tool-1".to_owned(),
                tool_name: "search".to_owned(),
                target: Some("src".to_owned()),
            }
        );

        let completed = parse_runtime_event(envelope(
            "tool.completed",
            json!({"tool_id": "tool-1", "name": "search", "output": "result"}),
        ))
        .expect("tool.completed should parse")
        .expect("known event");
        assert_eq!(
            completed,
            RuntimeEvent::ToolCompleted {
                tool_id: "tool-1".to_owned(),
                tool_name: "search".to_owned(),
                output: Some("result".to_owned()),
            }
        );

        let delta = parse_runtime_event(envelope(
            "execution.output.delta",
            json!({
                "execution_id": "exec-1",
                "capability": "shell.exec",
                "channel": "stderr",
                "delta": "warning\n"
            }),
        ))
        .expect("execution output delta should parse")
        .expect("known event");
        assert_eq!(
            delta,
            RuntimeEvent::ExecutionOutputDelta {
                execution_id: "exec-1".to_owned(),
                capability: "shell.exec".to_owned(),
                channel: "stderr".to_owned(),
                delta: "warning\n".to_owned(),
            }
        );
    }

    #[test]
    fn ignores_unknown_future_events_without_breaking_the_active_turn() {
        let event = parse_runtime_event(envelope("future.event", json!({"new_field": "ignored"})))
            .expect("unknown additive events are compatible with v1");
        assert_eq!(event, None);
    }

    #[test]
    fn parses_session_metadata_without_dropping_model_or_provider() {
        let event = parse_runtime_event(envelope(
            "session.updated",
            json!({"model": "gpt-5.6-luna", "provider": "opencode-go"}),
        ))
        .expect("session.updated should parse")
        .expect("known event");
        assert_eq!(
            event,
            RuntimeEvent::SessionUpdated {
                model: "gpt-5.6-luna".to_owned(),
                provider: "opencode-go".to_owned(),
            }
        );
    }

    #[test]
    fn parses_turn_cancellation_as_a_terminal_event_with_partial_content() {
        let event = parse_runtime_event(envelope(
            "turn.cancelled",
            json!({"content": "partial", "message_id": "message-1"}),
        ))
        .expect("turn.cancelled should parse")
        .expect("known event");

        assert_eq!(
            event,
            RuntimeEvent::TurnCancelled {
                content: "partial".to_owned(),
                message_id: Some("message-1".to_owned()),
            }
        );
        assert!(event.is_terminal());
    }

    #[test]
    fn keeps_the_public_protocol_schema_versioned() {
        let schema: Value =
            serde_json::from_str(include_str!("../../../protocol/runtime/v1/schema.json"))
                .expect("public Runtime Protocol schema should be valid JSON");

        assert_eq!(
            schema["$defs"]["protocolHeader"]["properties"]["protocol"]["const"],
            PROTOCOL
        );
        assert_eq!(
            schema["$defs"]["protocolHeader"]["properties"]["version"]["const"],
            VERSION
        );
    }
}
