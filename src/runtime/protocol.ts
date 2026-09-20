export const RUNTIME_PROTOCOL = 'atlas-runtime';
export const RUNTIME_PROTOCOL_VERSION = 1;

export type RuntimeTurnRequest = {
  protocol: typeof RUNTIME_PROTOCOL;
  version: typeof RUNTIME_PROTOCOL_VERSION;
  type: 'turn.request';
  request_id: string;
  conversation_id: string;
  input: string;
};

export type RuntimeTurnCancel = {
  protocol: typeof RUNTIME_PROTOCOL;
  version: typeof RUNTIME_PROTOCOL_VERSION;
  type: 'turn.cancel';
  request_id: string;
  conversation_id: string;
};

export type RuntimeRequest = RuntimeTurnRequest | RuntimeTurnCancel;

export type RuntimeEventType =
  | 'session.updated'
  | 'turn.started'
  | 'context.updated'
  | 'message.delta'
  | 'message.completed'
  | 'reasoning-start'
  | 'reasoning-delta'
  | 'reasoning-end'
  | 'tool.started'
  | 'tool.completed'
  | 'execution.started'
  | 'execution.output.delta'
  | 'execution.completed'
  | 'turn.completed'
  | 'turn.cancelled'
  | 'error';

export type RuntimeContextUsage = {
  used_tokens: number;
  context_window: number;
};

export type RuntimeSessionUpdatedData = {
  model: string;
  provider: string;
};

export type RuntimeToolStartedData = {
  tool_id: string;
  name: string;
  target?: string;
};

export type RuntimeReasoningStartedData = {
  reasoning_id: string;
};

export type RuntimeReasoningDeltaData = {
  reasoning_id: string;
  delta: string;
};

export type RuntimeReasoningEndedData = {
  reasoning_id: string;
};

export type RuntimeTurnCompletedData = {
  content: string;
  message_id?: string;
  context?: RuntimeContextUsage;
};

export type RuntimeTurnCancelledData = {
  content: string;
  message_id?: string;
};

export type RuntimeExecutionStartedData = {
  execution_id: string;
  capability: 'shell.exec';
  program: string;
  args: string[];
  cwd?: string;
  target?: string;
};

export type RuntimeExecutionCompletedData = {
  execution_id: string;
  capability: 'shell.exec';
  stdout: string;
  stderr: string;
  exit_code: number;
  duration_ms: number;
  status: string;
};

export type RuntimeEvent = {
  protocol: typeof RUNTIME_PROTOCOL;
  version: typeof RUNTIME_PROTOCOL_VERSION;
  type: RuntimeEventType;
  request_id?: string;
  conversation_id?: string;
  data: Record<string, unknown>;
};

export class RuntimeProtocolError extends Error {
  public constructor(message: string) {
    super(message);
    this.name = 'RuntimeProtocolError';
  }
}

function objectValue(value: unknown, context: string): Record<string, unknown> {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new RuntimeProtocolError(`${context} must be an object.`);
  }

  return value as Record<string, unknown>;
}

function requiredString(value: unknown, field: string): string {
  if (typeof value !== 'string' || !value.trim()) {
    throw new RuntimeProtocolError(`Runtime request field "${field}" must be a non-empty string.`);
  }

  return value;
}

export function parseRuntimeMessage(payload: string): RuntimeRequest {
  let value: unknown;
  try {
    value = JSON.parse(payload) as unknown;
  } catch (error) {
    throw new RuntimeProtocolError(
      `Runtime request is not valid JSON: ${error instanceof Error ? error.message : String(error)}`,
    );
  }

  const request = objectValue(value, 'Runtime request');
  if (request.protocol !== RUNTIME_PROTOCOL) {
    throw new RuntimeProtocolError(`Unsupported runtime protocol "${String(request.protocol)}".`);
  }
  if (request.version !== RUNTIME_PROTOCOL_VERSION) {
    throw new RuntimeProtocolError(
      `Unsupported runtime protocol version "${String(request.version)}".`,
    );
  }

  const request_id = requiredString(request.request_id, 'request_id');
  const conversation_id = requiredString(request.conversation_id, 'conversation_id');
  if (request.type === 'turn.request') {
    return {
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'turn.request',
      request_id,
      conversation_id,
      input: requiredString(request.input, 'input'),
    };
  }
  if (request.type === 'turn.cancel') {
    return {
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'turn.cancel',
      request_id,
      conversation_id,
    };
  }
  throw new RuntimeProtocolError(`Unsupported runtime message type "${String(request.type)}".`);
}

export function parseRuntimeTurnRequest(payload: string): RuntimeTurnRequest {
  const request = parseRuntimeMessage(payload);
  if (request.type !== 'turn.request') {
    throw new RuntimeProtocolError(`Expected turn.request, received "${request.type}".`);
  }
  return request;
}

export function runtimeEvent(
  request: RuntimeTurnRequest,
  type: RuntimeEventType,
  data: Record<string, unknown> = {},
): RuntimeEvent {
  return {
    protocol: RUNTIME_PROTOCOL,
    version: RUNTIME_PROTOCOL_VERSION,
    type,
    request_id: request.request_id,
    conversation_id: request.conversation_id,
    data,
  };
}

export function runtimeErrorEvent(
  message: string,
  request?: Pick<RuntimeTurnRequest, 'request_id' | 'conversation_id'>,
): RuntimeEvent {
  return {
    protocol: RUNTIME_PROTOCOL,
    version: RUNTIME_PROTOCOL_VERSION,
    type: 'error',
    ...(request === undefined
      ? {}
      : {
          request_id: request.request_id,
          conversation_id: request.conversation_id,
        }),
    data: {
      code: 'runtime_error',
      message,
    },
  };
}

export function serializeRuntimeMessage(message: RuntimeEvent): string {
  return `${JSON.stringify(message)}\n`;
}
