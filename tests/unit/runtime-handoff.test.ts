import assert from 'node:assert/strict';
import { createConnection, createServer } from 'node:net';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { randomUUID } from 'node:crypto';
import { test } from 'node:test';

import { UnixTelegramRuntime } from '../../clients/telegram/runtime.js';
import { startStreamingTestServer } from '../support/open-code-go-streaming-server.js';
import { OperationStore } from '../../src/host/index.js';
import { RUNTIME_PROTOCOL, RUNTIME_PROTOCOL_VERSION } from '../../src/runtime/protocol.js';
import { AtlasRuntimeServer } from '../../src/runtime/server.js';

type WireMessage = Record<string, unknown>;

async function sendTurn(
  socketPath: string,
  requestId: string,
  conversationId: string,
): Promise<WireMessage[]> {
  return new Promise((resolve, reject) => {
    const socket = createConnection(socketPath);
    const events: WireMessage[] = [];
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
    socket.once('error', (error) => finish(() => reject(error)));
    socket.setEncoding('utf8');
    socket.on('data', (chunk: string) => {
      buffer += chunk;
      let newline = buffer.indexOf('\n');
      while (newline !== -1) {
        const line = buffer.slice(0, newline).trim();
        buffer = buffer.slice(newline + 1);
        newline = buffer.indexOf('\n');
        if (!line) {
          continue;
        }
        const event = JSON.parse(line) as WireMessage;
        events.push(event);
        if (event.type === 'turn.completed' || event.type === 'error') {
          finish(() => resolve(events));
          return;
        }
      }
    });
    socket.once('connect', () => {
      socket.write(
        `${JSON.stringify({
          protocol: RUNTIME_PROTOCOL,
          version: RUNTIME_PROTOCOL_VERSION,
          type: 'turn.request',
          request_id: requestId,
          conversation_id: conversationId,
          input: 'continue',
        })}\n`,
      );
    });
  });
}

test('recupera operação pendente e mantém a mesma conversation_id', async () => {
  const model = await startStreamingTestServer();
  const directory = await mkdtemp(join(tmpdir(), 'atlas-handoff-runtime-'));
  const store = new OperationStore(join(directory, 'operations.json'));
  const operation = await store.create({
    request_id: 'handoff-request',
    conversation_id: 'handoff-conversation',
    objective: 'Verificar a alteração no Runtime.',
    state: 'checkpointed',
    base_commit: 'base-commit',
    candidate_build: 'candidate:handoff',
    next_step: 'Concluir a tarefa original.',
  });
  const runtime = new AtlasRuntimeServer({
    socketPath: join(directory, 'runtime.sock'),
    operationStore: store,
    runOptions: {
      apiKey: 'atlas...ey',
      baseURL: model.baseURL,
      coreTools: false,
    },
  });

  try {
    await runtime.listen();
    const events = await sendTurn(
      runtime.socketPath,
      operation.request_id,
      operation.conversation_id,
    );
    assert.deepEqual(
      events.map((event) => event.type),
      [
        'operation.resuming',
        'session.updated',
        'turn.started',
        'operation.resumed',
        'turn.completed',
      ],
    );
    assert.ok(events.every((event) => event.conversation_id === operation.conversation_id));
    assert.match(JSON.stringify(model.requests[0]), /Verificar a alteração no Runtime/);
    assert.equal((await store.get(operation.operation_id))?.state, 'resumed');
  } finally {
    await runtime.close();
    await model.close();
    await rm(directory, { recursive: true, force: true });
  }
});

test('cliente Telegram reconecta após a troca do Runtime e reutiliza a requisição', async () => {
  const socketPath = join(tmpdir(), `atlas-client-reconnect-${randomUUID()}.sock`);
  const requests: WireMessage[] = [];
  let connections = 0;
  const server = createServer((socket) => {
    connections += 1;
    if (connections === 1) {
      socket.destroy();
      return;
    }
    let buffer = '';
    socket.setEncoding('utf8');
    socket.on('data', (chunk: string) => {
      buffer += chunk;
      const newline = buffer.indexOf('\n');
      if (newline === -1) {
        return;
      }
      requests.push(JSON.parse(buffer.slice(0, newline)) as WireMessage);
      socket.end(
        `${JSON.stringify({
          protocol: RUNTIME_PROTOCOL,
          version: RUNTIME_PROTOCOL_VERSION,
          type: 'turn.completed',
          request_id: 'reconnect-request',
          conversation_id: 'reconnect-conversation',
          data: { content: 'retomado' },
        })}\n`,
      );
    });
  });

  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(socketPath, resolve);
  });
  try {
    const runtime = new UnixTelegramRuntime(socketPath);
    const terminal = await runtime.runTurn(
      {
        request_id: 'reconnect-request',
        conversation_id: 'reconnect-conversation',
        input: 'continue',
      },
      () => undefined,
    );
    assert.equal(terminal.type, 'turn.completed');
    assert.ok(connections >= 2);
    assert.equal(requests.length, 1);
    assert.equal(requests[0]?.request_id, 'reconnect-request');
    assert.equal(requests[0]?.conversation_id, 'reconnect-conversation');
  } finally {
    await new Promise<void>((resolveClose, rejectClose) => {
      server.close((error) => (error ? rejectClose(error) : resolveClose()));
    });
    await rm(socketPath, { force: true });
  }
});
