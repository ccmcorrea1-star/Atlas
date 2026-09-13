# Atlas Runtime Protocol v1

The Atlas Runtime Protocol is the public boundary between the Runtime and
clients such as the TUI, desktop, web, and mobile applications. It is
transport-independent and contains user intent and public execution events,
not Agent SDK, discovery, or capability implementation details.

## Encoding

Messages are UTF-8 JSON objects delimited by one newline (`JSON Lines`). The
first transport is a Unix Domain Socket. Other transports may carry the same
messages later without changing this contract.

Every message includes:

```json
{
  "protocol": "atlas-runtime",
  "version": 1
}
```

## Turn Request

Clients send only the user's intention. `request_id` correlates all events
belonging to this request. Reusing `conversation_id` preserves conversation
continuity in the Runtime.

```json
{
  "protocol": "atlas-runtime",
  "version": 1,
  "type": "turn.request",
  "request_id": "request-1",
  "conversation_id": "minha-conversa",
  "input": "execute node --version"
}
```

## Events

The Runtime sends zero or more events for the request. Each event includes the
same `request_id` and `conversation_id`:

```json
{
  "protocol": "atlas-runtime",
  "version": 1,
  "type": "turn.started",
  "request_id": "request-1",
  "conversation_id": "minha-conversa",
  "data": {}
}
```

For `process.exec`, the Runtime publishes a structured lifecycle instead of
the generic tool events. Both events use the Agent tool `call_id` as
`execution_id`, so clients update one execution cell:

```json
{
  "protocol": "atlas-runtime",
  "version": 1,
  "type": "execution.started",
  "request_id": "request-1",
  "conversation_id": "minha-conversa",
  "data": {
    "execution_id": "call-1",
    "capability": "process.exec",
    "program": "node",
    "args": ["--version"],
    "target": "local"
  }
}
```

```json
{
  "protocol": "atlas-runtime",
  "version": 1,
  "type": "execution.completed",
  "request_id": "request-1",
  "conversation_id": "minha-conversa",
  "data": {
    "execution_id": "call-1",
    "capability": "process.exec",
    "stdout": "v22.x.x\n",
    "stderr": "",
    "exit_code": 0,
    "duration_ms": 120,
    "status": "success"
  }
}
```

Supported event types are:

- `turn.started`: the Runtime accepted the turn.
- `message.delta`: a real streamed text fragment, with `data.message_id` and `data.delta`.
- `message.completed`: a completed public assistant message, with `data.message_id` and `data.content`.
- `tool.started`: a Runtime-executed tool started, with `data.tool_id` and `data.name`.
- `tool.completed`: a Runtime-executed tool finished, with `data.tool_id`, `data.name`, and optional `data.output`.
- `execution.started`: a `process.exec` invocation started, with `data.execution_id`, `data.capability`, `data.program`, `data.args`, and optional `data.cwd` and `data.target`.
- `execution.completed`: the matching `process.exec` invocation finished, with `data.execution_id`, `data.capability`, `data.stdout`, `data.stderr`, `data.exit_code`, `data.duration_ms`, and `data.status`.
- `turn.completed`: the turn finished, with `data.content` and optional `data.message_id`.
- `error`: the turn failed, with `data.code` and `data.message`.

Clients must handle `message.delta` or `message.completed`; a Runtime may emit
both when streaming is available. A Runtime only emits an event when the
underlying Agent execution provides that information. Clients must ignore event
types added in later protocol versions when they can continue safely.

Generic tool event data intentionally does not contain tool arguments, discovery
requests, or SDK objects. `process.exec` is the explicit exception: its public
execution lifecycle contains only the structured process invocation fields
needed by clients to render and correlate one execution cell. The Runtime
remains responsible for discovery, capabilities, approval policy, and execution.

## Versioning

`version` is an integer major version. Additive event data and new event types
are compatible with v1. A breaking change requires a new version directory and
version value; v1 clients must never be required to understand internal
Runtime changes.
