import assert from 'node:assert/strict';
import { createConnection } from 'node:net';
import { createServer } from 'node:http';
import { randomUUID } from 'node:crypto';
import { test } from 'node:test';

import type { CapabilityRuntime } from '../../src/capability-runtime.js';
import { AtlasRuntimeServer } from '../../src/runtime/server.js';
import { RUNTIME_PROTOCOL, RUNTIME_PROTOCOL_VERSION } from '../../src/runtime/protocol.js';

type WireMessage = Record<string, unknown>;

const capabilityRuntime: CapabilityRuntime = {
  discover: async () => [],
  getDefinition: async () => undefined,
  execute: async () => ({ target: 'local', status: 'ok', error: '' }),
};

function responseBody(status: string, output: WireMessage[] = []): WireMessage {
  return {
    id: 'runtime-integration-response',
    object: 'response',
    created_at: 1,
    status,
    model: 'gpt-5.6-luna',
    output,
    usage: {
      input_tokens: 1,
      output_tokens: 2,
      total_tokens: 3,
    },
  };
}

async function startStreamingModelServer(): Promise<{
  baseURL: string;
  requests: WireMessage[];
  close: () => Promise<void>;
}> {
  const requests: WireMessage[] = [];
  const server = createServer(async (request, response) => {
    const chunks: Buffer[] = [];
    for await (const chunk of request) {
      chunks.push(Buffer.from(chunk));
    }
    requests.push(JSON.parse(Buffer.concat(chunks).toString('utf8')) as WireMessage);

    const message = {
      id: 'runtime-integration-message',
      type: 'message',
      status: 'completed',
      role: 'assistant',
      content: [{ type: 'output_text', text: 'v22.x.x', annotations: [] }],
    };
    const events = [
      {
        type: 'response.created',
        sequence_number: 1,
        response: responseBody('in_progress'),
      },
      {
        type: 'response.output_item.added',
        sequence_number: 2,
        output_index: 0,
        item: {
          id: message.id,
          type: 'message',
          status: 'in_progress',
          role: 'assistant',
          content: [],
        },
      },
      {
        type: 'response.content_part.added',
        sequence_number: 3,
        output_index: 0,
        content_index: 0,
        item_id: message.id,
        part: { type: 'output_text', text: '', annotations: [] },
      },
      {
        type: 'response.output_text.delta',
        sequence_number: 4,
        output_index: 0,
        content_index: 0,
        item_id: message.id,
        delta: 'v22.x.x',
      },
      {
        type: 'response.output_text.done',
        sequence_number: 5,
        output_index: 0,
        content_index: 0,
        item_id: message.id,
        text: 'v22.x.x',
      },
      {
        type: 'response.content_part.done',
        sequence_number: 6,
        output_index: 0,
        content_index: 0,
        item_id: message.id,
        part: message.content[0],
      },
      {
        type: 'response.output_item.done',
        sequence_number: 7,
        output_index: 0,
        item: message,
      },
      {
        type: 'response.completed',
        sequence_number: 8,
        response: responseBody('completed', [message]),
      },
    ];

    response.writeHead(200, { 'content-type': 'text/event-stream' });
    for (const event of events) {
      response.write(`data: ${JSON.stringify(event)}\n\n`);
    }
    response.end();
  });

  const port = await new Promise<number>((resolvePort, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      if (!address || typeof address === 'string') {
        reject(new Error('Runtime integration model server did not receive a TCP address.'));
        return;
      }
      resolvePort(address.port);
    });
  });

  return {
    baseURL: `http://127.0.0.1:${port}/zen/go/v1`,
    requests,
    close: () =>
      new Promise<void>((resolveClose, reject) => {
        server.close((error) => (error ? reject(error) : resolveClose()));
      }),
  };
}

function sendTurn(socketPath: string, conversationId: string): Promise<WireMessage[]> {
  return new Promise((resolveTurn, rejectTurn) => {
    const requestId = 'tui-integration-request';
    const socket = createConnection(socketPath, () => {
      socket.write(
        `${JSON.stringify({
          protocol: RUNTIME_PROTOCOL,
          version: RUNTIME_PROTOCOL_VERSION,
          type: 'turn.request',
          request_id: requestId,
          conversation_id: conversationId,
          input: 'execute node --version',
        })}\n`,
      );
    });
    const events: WireMessage[] = [];
    let buffer = '';

    socket.once('error', rejectTurn);
    socket.setEncoding('utf8');
    socket.on('data', (chunk: string) => {
      buffer += chunk;
      let newlineIndex = buffer.indexOf('\n');
      while (newlineIndex !== -1) {
        const line = buffer.slice(0, newlineIndex);
        buffer = buffer.slice(newlineIndex + 1);
        if (line) {
          const event = JSON.parse(line) as WireMessage;
          events.push(event);
          if (event.type === 'turn.completed') {
            socket.destroy();
            resolveTurn(events);
            return;
          }
        }
        newlineIndex = buffer.indexOf('\n');
      }
    });
  });
}

test('connects the public Unix protocol to runAtlas and returns the real response', async () => {
  const model = await startStreamingModelServer();
  const socketPath = `/tmp/atlas-runtime-integration-${randomUUID()}.sock`;
  const runtime = new AtlasRuntimeServer({
    socketPath,
    runOptions: {
      apiKey: 'atlas-runtime-integration-key',
      baseURL: model.baseURL,
      capabilityRuntime,
    },
  });

  try {
    await runtime.listen();
    const events = await sendTurn(socketPath, 'runtime-integration-conversation');

    assert.deepEqual(
      events.map((event) => event.type),
      ['turn.started', 'message.delta', 'message.completed', 'turn.completed'],
    );
    assert.equal(events[0]?.request_id, 'tui-integration-request');
    assert.equal(events.at(-1)?.conversation_id, 'runtime-integration-conversation');
    assert.equal((events.at(-1)?.data as WireMessage).content, 'v22.x.x');
    assert.equal(model.requests.length, 1);
    assert.equal(model.requests[0]?.stream, true);
    assert.equal(model.requests[0]?.model, 'gpt-5.6-luna');
  } finally {
    await runtime.close();
    await model.close();
  }
});
