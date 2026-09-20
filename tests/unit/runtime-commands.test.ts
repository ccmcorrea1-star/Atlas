import assert from 'node:assert/strict';
import { createConnection } from 'node:net';
import { randomUUID } from 'node:crypto';
import { test } from 'node:test';

import { AtlasRuntimeServer } from '../../src/runtime/server.js';
import { RUNTIME_PROTOCOL, RUNTIME_PROTOCOL_VERSION } from '../../src/runtime/protocol.js';

async function sendCommand(
  socketPath: string,
  conversationId: string,
  command: 'new' | 'status' | 'stop',
) {
  return new Promise<Record<string, unknown>>((resolve, reject) => {
    const socket = createConnection(socketPath, () => {
      socket.end(
        `${JSON.stringify({
          protocol: RUNTIME_PROTOCOL,
          version: RUNTIME_PROTOCOL_VERSION,
          type: 'command.request',
          request_id: randomUUID(),
          conversation_id: conversationId,
          command,
        })}\n`,
      );
    });
    let buffer = '';
    socket.once('error', reject);
    socket.setEncoding('utf8');
    socket.on('data', (chunk: string) => {
      buffer += chunk;
      const newline = buffer.indexOf('\n');
      if (newline === -1) {
        return;
      }
      socket.destroy();
      resolve(JSON.parse(buffer.slice(0, newline)) as Record<string, unknown>);
    });
  });
}

test('handles Runtime commands independently from Telegram', async () => {
  const runtime = new AtlasRuntimeServer({
    socketPath: `/tmp/atlas-runtime-command-${randomUUID()}.sock`,
  });
  const conversationId = 'telegram:123:thread:9';
  try {
    await runtime.listen();
    const status = await sendCommand(runtime.socketPath, conversationId, 'status');
    assert.equal(status.type, 'command.completed');
    assert.deepEqual(status.data && (status.data as Record<string, unknown>).session, {
      id: conversationId,
      model: 'gpt-5.6-luna',
      provider: 'opencode-go',
      status: 'idle',
    });

    const fresh = await sendCommand(runtime.socketPath, conversationId, 'new');
    assert.equal(fresh.type, 'command.completed');
    assert.equal((fresh.data as Record<string, unknown>).message, 'Nova sessão iniciada.');

    const stop = await sendCommand(runtime.socketPath, conversationId, 'stop');
    assert.equal(stop.type, 'command.completed');
    assert.equal((stop.data as Record<string, unknown>).message, 'Nenhum turno ativo.');
  } finally {
    await runtime.close();
  }
});
