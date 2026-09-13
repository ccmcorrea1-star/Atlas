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

Supported event types are:

- `turn.started`: the Runtime accepted the turn.
- `message.delta`: a real streamed text fragment, with `data.message_id` and `data.delta`.
- `message.completed`: a completed public assistant message, with `data.message_id` and `data.content`.
- `tool.started`: a Runtime-executed tool started, with `data.tool_id` and `data.name`.
- `tool.completed`: a Runtime-executed tool finished, with `data.tool_id`, `data.name`, and optional `data.output`.
- `turn.completed`: the turn finished, with `data.content` and optional `data.message_id`.
- `error`: the turn failed, with `data.code` and `data.message`.

Clients must handle `message.delta` or `message.completed`; a Runtime may emit
both when streaming is available. A Runtime only emits an event when the
underlying Agent execution provides that information. Clients must ignore event
types added in later protocol versions when they can continue safely.

Tool event data intentionally does not contain tool arguments, discovery
requests, or SDK objects. The Runtime remains responsible for discovery,
capabilities, approval policy, and execution.

## Versioning

`version` is an integer major version. Additive event data and new event types
are compatible with v1. A breaking change requires a new version directory and
version value; v1 clients must never be required to understand internal
Runtime changes.
