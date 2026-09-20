export const RUNTIME_PROTOCOL = 'atlas-runtime';
export const RUNTIME_PROTOCOL_VERSION = 1;

export type RuntimeAttachmentType = 'voice' | 'audio' | 'image' | 'document';

export type RuntimeAttachment = {
  type: RuntimeAttachmentType;
  uri: string;
  media_type: string;
  file_name?: string;
  size_bytes?: number;
  source?: Record<string, string>;
};

export type RuntimeCommandName = 'new' | 'status' | 'stop';

export type RuntimeCommandDefinition = {
  name: RuntimeCommandName;
  description: string;
  available_during_turn: boolean;
};

export const RUNTIME_COMMANDS: readonly RuntimeCommandDefinition[] = [
  { name: 'new', description: 'inicia uma nova sessão', available_during_turn: true },
  { name: 'status', description: 'consulta o estado da sessão', available_during_turn: true },
  { name: 'stop', description: 'cancela o turno ativo', available_during_turn: true },
];

export type RuntimeSessionStatus = 'idle' | 'running' | 'cancelled';

export type RuntimeSession = {
  id: string;
  model: string;
  provider: string;
  status: RuntimeSessionStatus;
  active_request_id?: string;
};

export type RuntimeTurnRequest = {
  protocol: typeof RUNTIME_PROTOCOL;
  version: typeof RUNTIME_PROTOCOL_VERSION;
  type: 'turn.request';
  request_id: string;
  conversation_id: string;
  input: string;
  attachments?: RuntimeAttachment[];
};

export type RuntimeTurnCancel = {
  protocol: typeof RUNTIME_PROTOCOL;
  version: typeof RUNTIME_PROTOCOL_VERSION;
  type: 'turn.cancel';
  request_id: string;
  conversation_id: string;
};

export type RuntimeCommandRequest = {
  protocol: typeof RUNTIME_PROTOCOL;
  version: typeof RUNTIME_PROTOCOL_VERSION;
  type: 'command.request';
  request_id: string;
  conversation_id: string;
  command: RuntimeCommandName;
};

export type RuntimeApprovalResponse = {
  protocol: typeof RUNTIME_PROTOCOL;
  version: typeof RUNTIME_PROTOCOL_VERSION;
  type: 'approval.respond';
  request_id: string;
  conversation_id: string;
  approval_id: string;
  approved: boolean;
  comment?: string;
};

export type RuntimeRequest =
  RuntimeTurnRequest | RuntimeTurnCancel | RuntimeCommandRequest | RuntimeApprovalResponse;

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
  | 'command.completed'
  | 'approval.requested'
  | 'approval.resolved'
  | 'runtime.restarting'
  | 'runtime.ready'
  | 'operation.resuming'
  | 'operation.resumed'
  | 'error';

export type RuntimeContextUsage = {
  used_tokens: number;
  context_window: number;
};

export type RuntimeSessionUpdatedData = {
  model: string;
  provider: string;
};

export type RuntimeCommandCompletedData = {
  command: RuntimeCommandName;
  message: string;
  session: RuntimeSession;
};

export type RuntimeApprovalRequestedData = {
  approval_id: string;
  tool_name: string;
  reason: string;
  expires_at?: string;
};

export type RuntimeApprovalResolvedData = {
  approval_id: string;
  approved: boolean;
  comment?: string;
};

export type RuntimeRestartingData = {
  reason: 'handoff' | 'rollback' | 'crash';
};

export type RuntimeOperationData = {
  operation_id: string;
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
  attachments?: RuntimeAttachment[];
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

function optionalString(value: unknown, field: string): string | undefined {
  if (value === undefined) {
    return undefined;
  }
  return requiredString(value, field);
}

function parseAttachments(value: unknown): RuntimeAttachment[] | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (!Array.isArray(value)) {
    throw new RuntimeProtocolError('Runtime request field "attachments" must be an array.');
  }

  return value.map((item, index) => {
    const attachment = objectValue(item, `Runtime attachment ${index}`);
    const type = requiredString(attachment.type, `attachments[${index}].type`);
    if (!['voice', 'audio', 'image', 'document'].includes(type)) {
      throw new RuntimeProtocolError(`Unsupported attachment type "${type}".`);
    }

    const size = attachment.size_bytes;
    if (
      size !== undefined &&
      (typeof size !== 'number' || !Number.isSafeInteger(size) || size < 0)
    ) {
      throw new RuntimeProtocolError(
        `attachments[${index}].size_bytes must be a non-negative integer.`,
      );
    }

    const source = attachment.source;
    if (source !== undefined) {
      const sourceObject = objectValue(source, `attachments[${index}].source`);
      for (const [key, sourceValue] of Object.entries(sourceObject)) {
        if (typeof sourceValue !== 'string') {
          throw new RuntimeProtocolError(`attachments[${index}].source.${key} must be a string.`);
        }
      }
    }

    const fileName =
      attachment.file_name === undefined
        ? undefined
        : requiredString(attachment.file_name, `attachments[${index}].file_name`);
    return {
      type: type as RuntimeAttachmentType,
      uri: requiredString(attachment.uri, `attachments[${index}].uri`),
      media_type: requiredString(attachment.media_type, `attachments[${index}].media_type`),
      ...(fileName === undefined ? {} : { file_name: fileName }),
      ...(size === undefined ? {} : { size_bytes: size as number }),
      ...(source === undefined ? {} : { source: source as Record<string, string> }),
    };
  });
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
    const attachments = parseAttachments(request.attachments);
    return {
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'turn.request',
      request_id,
      conversation_id,
      input: requiredString(request.input, 'input'),
      ...(attachments === undefined ? {} : { attachments }),
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
  if (request.type === 'command.request') {
    const command = requiredString(request.command, 'command');
    if (!RUNTIME_COMMANDS.some((definition) => definition.name === command)) {
      throw new RuntimeProtocolError(`Unsupported runtime command "${command}".`);
    }
    return {
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'command.request',
      request_id,
      conversation_id,
      command: command as RuntimeCommandName,
    };
  }
  if (request.type === 'approval.respond') {
    if (typeof request.approved !== 'boolean') {
      throw new RuntimeProtocolError('Runtime request field "approved" must be a boolean.');
    }
    const comment = optionalString(request.comment, 'comment');
    return {
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'approval.respond',
      request_id,
      conversation_id,
      approval_id: requiredString(request.approval_id, 'approval_id'),
      approved: request.approved === true,
      ...(comment === undefined ? {} : { comment }),
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
  request: Pick<RuntimeTurnRequest, 'request_id' | 'conversation_id'>,
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

export function runtimeLifecycleEvent(
  type: Extract<
    RuntimeEventType,
    'runtime.restarting' | 'runtime.ready' | 'operation.resuming' | 'operation.resumed'
  >,
  conversationId?: string,
  data: Record<string, unknown> = {},
  requestId?: string,
): RuntimeEvent {
  return {
    protocol: RUNTIME_PROTOCOL,
    version: RUNTIME_PROTOCOL_VERSION,
    type,
    ...(requestId === undefined ? {} : { request_id: requestId }),
    ...(conversationId === undefined ? {} : { conversation_id: conversationId }),
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
