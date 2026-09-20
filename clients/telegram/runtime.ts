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
    );
  }

  private exchange(
    payload: Record<string, unknown>,
    isTerminal: (event: RuntimeEvent) => boolean,
    onEvent: (event: RuntimeEvent) => void,
  ): Promise<RuntimeEvent> {
    return this.exchangeWithRetry(payload, isTerminal, onEvent);
  }

  private async exchangeWithRetry(
    payload: Record<string, unknown>,
    isTerminal: (event: RuntimeEvent) => boolean,
    onEvent: (event: RuntimeEvent) => void,
  ): Promise<RuntimeEvent> {
    let lastError: unknown;
    for (let attempt = 0; attempt < 8; attempt += 1) {
      try {
        return await this.exchangeOnce(payload, isTerminal, onEvent);
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
  ): Promise<RuntimeEvent> {
    return new Promise((resolve, reject) => {
      const socket = createConnection(this.socketPath);
      let buffer = '';
      let settled = false;

      const finish = (callback: () => void) => {
        if (settled) {
          return;
        }
        settled = true;
        socket.destroy();
        callback();
      };

      const fail = (error: Error) => finish(() => reject(error));
      socket.once('error', fail);
      socket.once('close', () => {
        if (!settled) {
          finish(() => reject(new Error('Atlas Runtime socket closed before a terminal event.')));
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
        socket.write(`${JSON.stringify(payload)}\n`);
      });
    });
  }
}

function isTransientRuntimeError(error: unknown): boolean {
  if (!(error instanceof Error)) {
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
