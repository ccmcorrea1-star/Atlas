import { createConnection, type Socket } from 'node:net';
import { randomUUID } from 'node:crypto';

import {
  RUNTIME_PROTOCOL,
  RUNTIME_PROTOCOL_VERSION,
  type RuntimeAttachment,
  type RuntimeCommandName,
  type RuntimeEvent,
} from '../../src/runtime/protocol.js';
import type { TelegramRuntime } from './types.js';

type RuntimeTurnPayload = {
  request_id: string;
  conversation_id: string;
  input: string;
  attachments?: RuntimeAttachment[];
};

function isTerminalTurn(event: RuntimeEvent): boolean {
  return (
    event.type === 'turn.completed' || event.type === 'turn.cancelled' || event.type === 'error'
  );
}

function isTerminalCommand(event: RuntimeEvent): boolean {
  return event.type === 'command.completed' || event.type === 'error';
}

function isTerminalApproval(event: RuntimeEvent): boolean {
  return event.type === 'approval.resolved' || event.type === 'error';
}

function isTerminalInput(event: RuntimeEvent): boolean {
  return (
    event.type === 'input.resolved' || event.type === 'turn.completed' || event.type === 'error'
  );
}

export class UnixTelegramRuntime implements TelegramRuntime {
  public constructor(private readonly socketPath: string) {}

  public runTurn(
    request: RuntimeTurnPayload,
    onEvent: (event: RuntimeEvent) => void,
  ): Promise<RuntimeEvent> {
    return this.exchange(
      {
        protocol: RUNTIME_PROTOCOL,
        version: RUNTIME_PROTOCOL_VERSION,
        type: 'turn.request',
        ...request,
      },
      isTerminalTurn,
      onEvent,
      false,
    );
  }

  public command(conversationId: string, command: RuntimeCommandName): Promise<RuntimeEvent> {
    return this.exchange(
      {
        protocol: RUNTIME_PROTOCOL,
        version: RUNTIME_PROTOCOL_VERSION,
        type: 'command.request',
        request_id: randomUUID(),
        conversation_id: conversationId,
        command,
      },
      isTerminalCommand,
      () => undefined,
      true,
    );
  }

  public respondApproval(
    conversationId: string,
    approvalId: string,
    approved: boolean,
    comment?: string,
  ): Promise<RuntimeEvent> {
    return this.exchange(
      {
        protocol: RUNTIME_PROTOCOL,
        version: RUNTIME_PROTOCOL_VERSION,
        type: 'approval.respond',
        request_id: randomUUID(),
        conversation_id: conversationId,
        approval_id: approvalId,
        approved,
        ...(comment === undefined ? {} : { comment }),
      },
      isTerminalApproval,
      () => undefined,
      true,
    );
  }

  public respondInput(
    conversationId: string,
    inputId: string,
    value: string,
  ): Promise<RuntimeEvent> {
    return this.exchange(
      {
        protocol: RUNTIME_PROTOCOL,
        version: RUNTIME_PROTOCOL_VERSION,
        type: 'input.respond',
        request_id: randomUUID(),
        conversation_id: conversationId,
        input_id: inputId,
        value,
      },
      isTerminalInput,
      () => undefined,
      true,
    );
  }

  private exchange(
    payload: Record<string, unknown>,
    isTerminal: (event: RuntimeEvent) => boolean,
    onEvent: (event: RuntimeEvent) => void,
    retryAfterSend: boolean,
  ): Promise<RuntimeEvent> {
    return this.exchangeWithRetry(payload, isTerminal, onEvent, retryAfterSend);
  }

  private async exchangeWithRetry(
    payload: Record<string, unknown>,
    isTerminal: (event: RuntimeEvent) => boolean,
    onEvent: (event: RuntimeEvent) => void,
    retryAfterSend: boolean,
  ): Promise<RuntimeEvent> {
    let lastError: unknown;
    for (let attempt = 0; attempt < 8; attempt += 1) {
      try {
        return await this.exchangeOnce(payload, isTerminal, onEvent, retryAfterSend);
      } catch (error) {
        lastError = error;
        if (!isTransientRuntimeError(error) || attempt === 7) {
          throw error;
        }
        await new Promise((resolve) => setTimeout(resolve, 50 * (attempt + 1)));
      }
    }
    throw lastError instanceof Error ? lastError : new Error(String(lastError));
  }

  private exchangeOnce(
    payload: Record<string, unknown>,
    isTerminal: (event: RuntimeEvent) => boolean,
    onEvent: (event: RuntimeEvent) => void,
    retryAfterSend: boolean,
  ): Promise<RuntimeEvent> {
    return new Promise((resolve, reject) => {
      const socket = createConnection(this.socketPath);
      let buffer = '';
      let settled = false;
      let sent = false;

      const finish = (callback: () => void) => {
        if (settled) {
          return;
        }
        settled = true;
        socket.destroy();
        callback();
      };

      const fail = (error: Error) =>
        finish(() =>
          reject(sent && !retryAfterSend ? new RuntimeRequestSentError(error.message) : error),
        );
      socket.once('error', fail);
      socket.once('close', () => {
        if (!settled) {
          const error = new Error('Atlas Runtime socket closed before a terminal event.');
          finish(() =>
            reject(sent && !retryAfterSend ? new RuntimeRequestSentError(error.message) : error),
          );
        }
      });
      socket.setEncoding('utf8');
      socket.on('data', (chunk: string) => {
        buffer += chunk;
        let newlineIndex = buffer.indexOf('\n');
        while (newlineIndex !== -1) {
          const line = buffer.slice(0, newlineIndex).trim();
          buffer = buffer.slice(newlineIndex + 1);
          newlineIndex = buffer.indexOf('\n');
          if (!line) {
            continue;
          }
          let parsed: unknown;
          try {
            parsed = JSON.parse(line) as unknown;
          } catch {
            fail(new Error('Atlas Runtime returned invalid JSON.'));
            return;
          }
          if (!isRuntimeEvent(parsed)) {
            fail(new Error('Atlas Runtime returned an invalid event.'));
            return;
          }
          onEvent(parsed);
          if (isTerminal(parsed)) {
            finish(() => resolve(parsed));
            return;
          }
        }
      });
      socket.once('connect', () => {
        try {
          socket.write(`${JSON.stringify(payload)}\n`, (error?: Error | null) => {
            if (error != null) {
              fail(error);
              return;
            }
            // Retry permanece seguro antes da confirmação da escrita.
            sent = true;
          });
        } catch (error) {
          fail(error instanceof Error ? error : new Error(String(error)));
        }
      });
    });
  }
}

class RuntimeRequestSentError extends Error {
  public constructor(message: string) {
    super(message);
    this.name = 'RuntimeRequestSentError';
  }
}

function isTransientRuntimeError(error: unknown): boolean {
  if (!(error instanceof Error)) {
    return false;
  }
  if (error instanceof RuntimeRequestSentError) {
    return false;
  }
  return (
    error.message.includes('socket closed before a terminal event') ||
    error.message.includes('ENOENT') ||
    error.message.includes('ECONNREFUSED') ||
    error.message.includes('ECONNRESET') ||
    error.message.includes('EPIPE')
  );
}

function isRuntimeEvent(value: unknown): value is RuntimeEvent {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    return false;
  }
  const event = value as Record<string, unknown>;
  return (
    event.protocol === RUNTIME_PROTOCOL &&
    event.version === RUNTIME_PROTOCOL_VERSION &&
    typeof event.type === 'string' &&
    event.data !== null &&
    typeof event.data === 'object' &&
    !Array.isArray(event.data)
  );
}

export type { Socket };
