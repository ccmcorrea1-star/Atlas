import assert from 'node:assert/strict';
import { createConnection } from 'node:net';
import { createServer } from 'node:http';
import { randomUUID } from 'node:crypto';
import { test } from 'node:test';

import type { CapabilityRuntime } from '../../src/capability-runtime.js';
import { atlasRuntimeSessionData } from '../../src/config/index.js';
import { runAtlas } from '../../src/index.js';
import { AtlasRuntimeServer } from '../../src/runtime/server.js';
import { RUNTIME_PROTOCOL, RUNTIME_PROTOCOL_VERSION } from '../../src/runtime/protocol.js';

type WireMessage = Record<string, unknown>;

const capabilityRuntime: CapabilityRuntime = {
  discover: async () => [],
  listTools: async () => [],
  getDefinition: async () => undefined,
  execute: async () => ({ target: 'local', status: 'ok', error: '' }),
};

const processCapabilityRuntime: CapabilityRuntime = {
  discover: async () => [{ id: 'process.exec', type: 'tool', summary: 'execute a process' }],
  listTools: async () => [
    { id: 'process', type: 'group', summary: 'process tools' },
    { id: 'process.exec', type: 'tool', summary: 'execute a process', group: 'process' },
  ],
  getDefinition: async (id) =>
    id === 'process.exec'
      ? {
          id,
          type: 'tool',
          summary: 'execute a process',
          description: 'execute a process directly without a shell',
          schema: {
            type: 'object',
            properties: {
              program: { type: 'string' },
              args: { type: 'array', items: { type: 'string' } },
              cwd: { type: 'string' },
            },
            required: ['program'],
            additionalProperties: false,
          },
        }
      : undefined,
  execute: async (_id, target, arguments_) => {
    const failed = arguments_.program === 'false';
    return {
      target,
      status: failed ? 'failed' : 'success',
      error: failed ? 'permission denied' : '',
      output: {
        stdout: failed ? '' : 'v22.x.x\n',
        stderr: failed ? 'permission denied' : '',
        exit_code: failed ? 1 : 0,
        duration_ms: failed ? 7 : 120,
      },
    };
  },
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

function functionCallStream(item: WireMessage): WireMessage[] {
  return [
    {
      type: 'response.created',
      sequence_number: 1,
      response: responseBody('in_progress'),
    },
    {
      type: 'response.output_item.added',
      sequence_number: 2,
      output_index: 0,
      item: { ...item, status: 'in_progress', arguments: '' },
    },
    {
      type: 'response.function_call_arguments.delta',
      sequence_number: 3,
      output_index: 0,
      item_id: item.id,
      delta: item.arguments,
    },
    {
      type: 'response.function_call_arguments.done',
      sequence_number: 4,
      output_index: 0,
      item_id: item.id,
      arguments: item.arguments,
    },
    {
      type: 'response.output_item.done',
      sequence_number: 5,
      output_index: 0,
      item,
    },
    {
      type: 'response.completed',
      sequence_number: 6,
      response: responseBody('completed', [item]),
    },
  ];
}

function messageStream(text: string): WireMessage[] {
  const message = {
    id: 'runtime-execution-message',
    type: 'message',
    status: 'completed',
    role: 'assistant',
    content: [{ type: 'output_text', text, annotations: [] }],
  };
  return [
    {
      type: 'response.created',
      sequence_number: 1,
      response: responseBody('in_progress'),
    },
    {
      type: 'response.output_item.added',
      sequence_number: 2,
      output_index: 0,
      item: { ...message, status: 'in_progress', content: [] },
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
      delta: text,
    },
    {
      type: 'response.output_text.done',
      sequence_number: 5,
      output_index: 0,
      content_index: 0,
      item_id: message.id,
      text,
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
}

async function startProcessStreamingModelServer(): Promise<{
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
    const body = JSON.parse(Buffer.concat(chunks).toString('utf8')) as WireMessage;
    requests.push(body);

    const failed = JSON.stringify(requests[0]?.input).includes('fail');
    const output =
      requests.length === 1
        ? {
            id: 'discover-call',
            type: 'function_call',
            status: 'completed',
            call_id: 'discover-call',
            name: 'discover',
            arguments: JSON.stringify({ query: 'executar programa' }),
          }
        : requests.length === 2
          ? {
              id: 'describe-call',
              type: 'function_call',
              status: 'completed',
              call_id: 'describe-call',
              name: 'describe',
              arguments: JSON.stringify({ id: 'process.exec' }),
            }
          : requests.length === 3
            ? {
                id: 'execution-call',
                type: 'function_call',
                status: 'completed',
                call_id: 'execution-call',
                name: 'execute',
                arguments: JSON.stringify({
                  id: 'process.exec',
                  arguments: {
                    program: failed ? 'false' : 'node',
                    args: failed ? [] : ['--version'],
                    cwd: '/tmp',
                  },
                }),
              }
            : undefined;
    const events = output
      ? functionCallStream(output)
      : messageStream(failed ? 'process failed' : 'Executed node --version: v22.x.x');

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
        reject(new Error('Process execution model server did not receive a TCP address.'));
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

async function startDirectToolStreamingModelServer(): Promise<{
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

    // Uma unica chamada de tool direta e a resposta final: sem discover/describe.
    const output =
      requests.length === 1
        ? {
            id: 'direct-execution-call',
            type: 'function_call',
            status: 'completed',
            call_id: 'direct-execution-call',
            name: 'process_exec',
            arguments: JSON.stringify({ program: 'node', args: ['--version'], cwd: '/tmp' }),
          }
        : undefined;
    const events = output
      ? functionCallStream(output)
      : messageStream('Executed node --version: v22.x.x');

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
        reject(new Error('Direct tool model server did not receive a TCP address.'));
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

async function startFailingStreamingModelServer(): Promise<{
  baseURL: string;
  close: () => Promise<void>;
}> {
  const server = createServer(async (request, response) => {
    for await (const _chunk of request) {
      // Consume the request before simulating a provider stream failure.
    }

    response.writeHead(200, { 'content-type': 'text/event-stream' });
    response.end(
      `data: ${JSON.stringify({
        type: 'error',
        sequence_number: 1,
        code: 'test_provider_error',
        message: 'simulated provider failure',
      })}\n\n`,
    );
  });

  const port = await new Promise<number>((resolvePort, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      if (!address || typeof address === 'string') {
        reject(new Error('Failing streaming model server did not receive a TCP address.'));
        return;
      }
      resolvePort(address.port);
    });
  });

  return {
    baseURL: `http://127.0.0.1:${port}/zen/go/v1`,
    close: () =>
      new Promise<void>((resolveClose, rejectClose) => {
        server.close((error) => (error ? rejectClose(error) : resolveClose()));
      }),
  };
}

async function startHangingModelServer(): Promise<{
  baseURL: string;
  close: () => Promise<void>;
}> {
  const server = createServer((request, response) => {
    request.on('aborted', () => response.destroy());
  });

  const port = await new Promise<number>((resolvePort, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      if (!address || typeof address === 'string') {
        reject(new Error('Hanging model server did not receive a TCP address.'));
        return;
      }
      resolvePort(address.port);
    });
  });

  return {
    baseURL: `http://127.0.0.1:${port}/zen/go/v1`,
    close: () =>
      new Promise<void>((resolveClose, rejectClose) => {
        server.close((error) => (error ? rejectClose(error) : resolveClose()));
      }),
  };
}

function sendTurnAndCancel(
  socketPath: string,
  conversationId: string,
  cancelConversationId = conversationId,
): Promise<{ events: WireMessage[]; cancelEvents: WireMessage[] }> {
  return new Promise((resolveTurn, rejectTurn) => {
    const requestId = 'tui-cancel-request';
    const socket = createConnection(socketPath, () => {
      socket.write(
        `${JSON.stringify({
          protocol: RUNTIME_PROTOCOL,
          version: RUNTIME_PROTOCOL_VERSION,
          type: 'turn.request',
          request_id: requestId,
          conversation_id: conversationId,
          input: 'wait forever',
        })}\n`,
      );
    });
    const events: WireMessage[] = [];
    const cancelEvents: WireMessage[] = [];
    let buffer = '';
    let cancelSent = false;

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
          if (event.type === 'turn.started' && !cancelSent) {
            cancelSent = true;
            const cancelSocket = createConnection(socketPath, () => {
              cancelSocket.end(
                `${JSON.stringify({
                  protocol: RUNTIME_PROTOCOL,
                  version: RUNTIME_PROTOCOL_VERSION,
                  type: 'turn.cancel',
                  request_id: requestId,
                  conversation_id: cancelConversationId,
                })}\n`,
              );
            });
            cancelSocket.once('error', rejectTurn);
            if (cancelConversationId !== conversationId) {
              cancelSocket.setEncoding('utf8');
              cancelSocket.once('data', (cancelChunk: string) => {
                cancelEvents.push(JSON.parse(cancelChunk.trim()) as WireMessage);
                const correctCancelSocket = createConnection(socketPath, () => {
                  correctCancelSocket.end(
                    `${JSON.stringify({
                      protocol: RUNTIME_PROTOCOL,
                      version: RUNTIME_PROTOCOL_VERSION,
                      type: 'turn.cancel',
                      request_id: requestId,
                      conversation_id: conversationId,
                    })}\n`,
                  );
                });
                correctCancelSocket.once('error', rejectTurn);
              });
            }
          }
          if (event.type === 'error') {
            socket.destroy();
            resolveTurn({ events, cancelEvents });
            return;
          }
        }
        newlineIndex = buffer.indexOf('\n');
      }
    });
  });
}

test('cancels an active Unix runtime turn and emits a terminal error', async () => {
  const model = await startHangingModelServer();
  const socketPath = `/tmp/atlas-runtime-cancel-${randomUUID()}.sock`;
  const runtime = new AtlasRuntimeServer({
    socketPath,
    runOptions: {
      apiKey: '[REDACTED]',
      baseURL: model.baseURL,
      capabilityRuntime,
    },
  });

  try {
    await runtime.listen();
    const { events } = await sendTurnAndCancel(socketPath, 'cancel-integration-conversation');

    assert.deepEqual(
      events.map((event) => event.type),
      ['session.updated', 'turn.started', 'error'],
    );
    assert.equal(events.at(-1)?.request_id, 'tui-cancel-request');
    assert.equal((events.at(-1)?.data as WireMessage).message, 'turn cancelled by client');
  } finally {
    await runtime.close();
    await model.close();
  }
});

test('does not cancel a turn when conversation identity does not match', async () => {
  const model = await startHangingModelServer();
  const socketPath = `/tmp/atlas-runtime-cancel-identity-${randomUUID()}.sock`;
  const runtime = new AtlasRuntimeServer({
    socketPath,
    runOptions: {
      apiKey: '[REDACTED]',
      baseURL: model.baseURL,
      capabilityRuntime,
    },
  });

  try {
    await runtime.listen();
    const result = await sendTurnAndCancel(
      socketPath,
      'cancel-identity-conversation',
      'other-conversation',
    );

    assert.deepEqual(
      result.events.map((event) => event.type),
      ['session.updated', 'turn.started', 'error'],
    );
    assert.deepEqual(
      result.cancelEvents.map((event) => event.type),
      ['error'],
    );
    assert.match(
      String((result.cancelEvents[0]?.data as WireMessage).message),
      /No active turn exists/,
    );
  } finally {
    await runtime.close();
    await model.close();
  }
});

function sendTurn(
  socketPath: string,
  conversationId: string,
  input = 'execute node --version',
): Promise<WireMessage[]> {
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
          input,
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
          if (event.type === 'turn.completed' || event.type === 'error') {
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
      [
        'session.updated',
        'turn.started',
        'message.delta',
        'message.completed',
        'context.updated',
        'turn.completed',
      ],
    );
    assert.equal(events[0]?.request_id, 'tui-integration-request');
    assert.deepEqual(events[0]?.data, {
      model: 'gpt-5.6-luna',
      provider: 'opencode-go',
    });
    assert.equal(events.at(-1)?.conversation_id, 'runtime-integration-conversation');
    assert.equal((events.at(-1)?.data as WireMessage).content, 'v22.x.x');
    assert.deepEqual((events.at(-1)?.data as WireMessage).context, {
      used_tokens: 1,
      context_window: 256000,
    });
    assert.equal(model.requests.length, 1);
    assert.equal(model.requests[0]?.stream, true);
    assert.equal(model.requests[0]?.model, 'gpt-5.6-luna');
  } finally {
    await runtime.close();
    await model.close();
  }
});

test('waits for a real streamed run before returning its final output', async () => {
  const model = await startStreamingModelServer();

  try {
    const events: string[] = [];
    const result = await runAtlas('stream this response', {
      apiKey: 'atlas-streaming-integration-key',
      baseURL: model.baseURL,
      conversationId: 'streaming-integration-conversation',
      capabilityRuntime,
      onEvent: (event) => {
        events.push(event.type);
      },
    });

    assert.equal(result.finalOutput, 'v22.x.x');
    assert.deepEqual(events, ['message.delta', 'message.completed']);
    assert.equal(model.requests[0]?.stream, true);
  } finally {
    await model.close();
  }
});

test('publishes process execution lifecycle events over the public Unix protocol', async () => {
  const model = await startProcessStreamingModelServer();
  const socketPath = `/tmp/atlas-runtime-process-execution-${randomUUID()}.sock`;
  const runtime = new AtlasRuntimeServer({
    socketPath,
    runOptions: {
      apiKey: 'atlas-runtime-process-execution-key',
      baseURL: model.baseURL,
      capabilityRuntime: processCapabilityRuntime,
    },
  });

  try {
    await runtime.listen();
    const events = await sendTurn(socketPath, 'process-execution-conversation');

    assert.deepEqual(
      events.map((event) => event.type),
      [
        'session.updated',
        'turn.started',
        'execution.started',
        'execution.completed',
        'message.delta',
        'message.completed',
        'context.updated',
        'turn.completed',
      ],
    );
    const started = events[2]?.data as WireMessage;
    const completed = events[3]?.data as WireMessage;
    assert.deepEqual(started, {
      execution_id: 'execution-call',
      capability: 'process.exec',
      program: 'node',
      args: ['--version'],
      cwd: '/tmp',
      target: 'local',
    });
    assert.deepEqual(completed, {
      execution_id: 'execution-call',
      capability: 'process.exec',
      stdout: 'v22.x.x\n',
      stderr: '',
      exit_code: 0,
      duration_ms: 120,
      status: 'success',
    });
  } finally {
    await runtime.close();
    await model.close();
  }
});

test('executes a materialized core tool directly and publishes the capability lifecycle', async () => {
  const model = await startDirectToolStreamingModelServer();
  const socketPath = `/tmp/atlas-runtime-direct-tool-${randomUUID()}.sock`;
  const runtime = new AtlasRuntimeServer({
    socketPath,
    runOptions: {
      apiKey: 'atlas...ey',
      baseURL: model.baseURL,
      capabilityRuntime: processCapabilityRuntime,
    },
  });

  try {
    await runtime.listen();
    const events = await sendTurn(socketPath, 'materialized-tool-conversation');

    // A tool direta cobre o mesmo lifecycle publico do caminho generico.
    assert.deepEqual(
      events.map((event) => event.type),
      [
        'session.updated',
        'turn.started',
        'execution.started',
        'execution.completed',
        'message.delta',
        'message.completed',
        'context.updated',
        'turn.completed',
      ],
    );
    assert.deepEqual(events[2]?.data, {
      execution_id: 'direct-execution-call',
      capability: 'process.exec',
      program: 'node',
      args: ['--version'],
      cwd: '/tmp',
      target: 'local',
    });
    assert.equal((events.at(-1)?.data as WireMessage).content, 'Executed node --version: v22.x.x');

    // Um round-trip para a tool e outro para a resposta: sem list_tools/describe.
    assert.equal(model.requests.length, 2);
    const toolNames = ((model.requests[0]?.tools as WireMessage[] | undefined) ?? []).map(
      (tool) => tool.name,
    );
    assert.equal(toolNames[0], 'process_exec');
    assert.ok(toolNames.includes('list_tools'));
    assert.doesNotMatch(JSON.stringify(model.requests[0]?.input), /"name":"(discover|describe)"/);
  } finally {
    await runtime.close();
    await model.close();
  }
});

test('publishes failed process execution status and output over the public protocol', async () => {
  const model = await startProcessStreamingModelServer();
  const socketPath = `/tmp/atlas-runtime-process-execution-failure-${randomUUID()}.sock`;
  const runtime = new AtlasRuntimeServer({
    socketPath,
    runOptions: {
      apiKey: 'atlas-runtime-process-execution-failure-key',
      baseURL: model.baseURL,
      capabilityRuntime: processCapabilityRuntime,
    },
  });

  try {
    await runtime.listen();
    const events = await sendTurn(
      socketPath,
      'process-execution-failure-conversation',
      'please fail this process',
    );

    assert.equal(events[2]?.type, 'execution.started');
    assert.equal(events[3]?.type, 'execution.completed');
    assert.deepEqual(events[3]?.data, {
      execution_id: 'execution-call',
      capability: 'process.exec',
      stdout: '',
      stderr: 'permission denied',
      exit_code: 1,
      duration_ms: 7,
      status: 'failed',
    });
    assert.equal(
      events.some((event) => event.type === 'tool.started' || event.type === 'tool.completed'),
      false,
    );
  } finally {
    await runtime.close();
    await model.close();
  }
});

test('publishes provider stream failures as a terminal runtime error', async () => {
  const model = await startFailingStreamingModelServer();
  const socketPath = `/tmp/atlas-runtime-stream-error-${randomUUID()}.sock`;
  const runtime = new AtlasRuntimeServer({
    socketPath,
    runOptions: {
      apiKey: 'atlas-stream-error-key',
      baseURL: model.baseURL,
      capabilityRuntime,
    },
  });

  try {
    await runtime.listen();
    const events = await sendTurn(socketPath, 'stream-error-conversation');

    assert.deepEqual(
      events.map((event) => event.type),
      ['session.updated', 'turn.started', 'error'],
    );
    assert.equal((events.at(-1)?.data as WireMessage).message, 'simulated provider failure');
  } finally {
    await runtime.close();
    await model.close();
  }
});

test('session metadata follows the global Atlas config', async () => {
  const model = await startStreamingModelServer();
  const socketPath = `/tmp/atlas-runtime-config-${randomUUID()}.sock`;
  const runtime = new AtlasRuntimeServer({
    socketPath,
    atlasConfig: { version: 1, provider: 'opencode-go', model: 'custom-model' },
    runOptions: {
      apiKey: 'atlas-config-integration-key',
      baseURL: model.baseURL,
      capabilityRuntime,
    },
  });

  try {
    await runtime.listen();
    const events = await sendTurn(socketPath, 'config-integration-conversation');

    assert.deepEqual(
      events.map((event) => event.type),
      // Modelos fora do registro não têm janela de contexto declarada.
      ['session.updated', 'turn.started', 'message.delta', 'message.completed', 'turn.completed'],
    );
    assert.deepEqual(events[0]?.data, {
      model: 'custom-model',
      provider: 'opencode-go',
    });
    assert.equal(model.requests[0]?.model, 'custom-model');
  } finally {
    await runtime.close();
    await model.close();
  }
});

test('rejects providers that the Runtime does not support yet', () => {
  assert.throws(
    () => atlasRuntimeSessionData({ version: 1, provider: 'anthropic', model: 'claude-x' }),
    /is not supported yet/,
  );
});
