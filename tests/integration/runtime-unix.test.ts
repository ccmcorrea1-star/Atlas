import assert from 'node:assert/strict';
import { createConnection } from 'node:net';
import { createServer } from 'node:http';
import { readFile, rm, writeFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { test } from 'node:test';

import type { CapabilityRuntime } from '../../src/capabilities/runtime-client.js';
import { atlasRuntimeSessionData } from '../../src/config/index.js';
import { runAtlas } from '../../src/index.js';
import { AtlasRuntimeServer } from '../../src/runtime/server.js';
import { RUNTIME_PROTOCOL, RUNTIME_PROTOCOL_VERSION } from '../../src/runtime/protocol.js';

type WireMessage = Record<string, unknown>;

const capabilityRuntime: CapabilityRuntime = {
  discover: async () => [],
  listTools: async () => [],
  getDefinition: async () => undefined,
  getSkill: async () => undefined,
  execute: async () => ({ target: 'local', status: 'ok', error: '' }),
};

const shellCapabilityRuntime: CapabilityRuntime = {
  discover: async () => [{ id: 'shell.exec', type: 'tool', summary: 'execute a command' }],
  listTools: async () => [
    { id: 'shell.exec', type: 'tool', summary: 'execute a command', group: 'shell' },
  ],
  getDefinition: async (id) =>
    id === 'shell.exec'
      ? {
          id,
          type: 'tool',
          summary: 'execute a command',
          description: 'execute a command with shell semantics',
          schema: {
            type: 'object',
            properties: {
              command: { type: 'string' },
              cwd: { type: 'string' },
            },
            required: ['command'],
            additionalProperties: false,
          },
        }
      : undefined,
  getSkill: async () => undefined,
  execute: async (_id, target, arguments_) => {
    const failed = arguments_.command === 'false';
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

function responseBody(
  status: string,
  output: WireMessage[] = [],
  usage: WireMessage = {
    input_tokens: 1,
    output_tokens: 2,
    total_tokens: 3,
  },
): WireMessage {
  return {
    id: 'runtime-integration-response',
    object: 'response',
    created_at: 1,
    status,
    model: 'gpt-5.6-luna',
    output,
    usage,
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

function functionCallStream(
  item: WireMessage,
  usage: WireMessage = {
    input_tokens: 1,
    output_tokens: 2,
    total_tokens: 3,
  },
): WireMessage[] {
  return [
    {
      type: 'response.created',
      sequence_number: 1,
      response: responseBody('in_progress', [], usage),
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
      response: responseBody('completed', [item], usage),
    },
  ];
}

function messageStream(
  text: string,
  usage: WireMessage = {
    input_tokens: 1,
    output_tokens: 2,
    total_tokens: 3,
  },
): WireMessage[] {
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
      response: responseBody('in_progress', [], usage),
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
      response: responseBody('completed', [message], usage),
    },
  ];
}

async function startShellStreamingModelServer(): Promise<{
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
            arguments: JSON.stringify({ query: 'executar comando' }),
          }
        : requests.length === 2
          ? {
              id: 'describe-call',
              type: 'function_call',
              status: 'completed',
              call_id: 'describe-call',
              name: 'describe',
              arguments: JSON.stringify({ id: 'shell.exec' }),
            }
          : requests.length === 3
            ? {
                id: 'execution-call',
                type: 'function_call',
                status: 'completed',
                call_id: 'execution-call',
                name: 'execute',
                arguments: JSON.stringify({
                  id: 'shell.exec',
                  arguments: {
                    command: failed ? 'false' : 'node --version',
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
            name: 'shell_exec',
            arguments: JSON.stringify({ command: 'node --version', cwd: '/tmp' }),
          }
        : undefined;
    const usage =
      requests.length === 1
        ? { input_tokens: 6_800, output_tokens: 420, total_tokens: 7_220 }
        : { input_tokens: 10_400, output_tokens: 1_200, total_tokens: 11_600 };
    const events = output
      ? functionCallStream(output, usage)
      : messageStream('Executed node --version: v22.x.x', usage);

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

async function startInputStreamingModelServer(): Promise<{
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
    const output =
      requests.length === 1
        ? {
            id: 'input-call',
            type: 'function_call',
            status: 'completed',
            call_id: 'input-call',
            name: 'request_input',
            arguments: JSON.stringify({
              prompt: 'Qual ambiente?',
              choices: ['local', 'remote'],
            }),
          }
        : undefined;
    const events = output ? functionCallStream(output) : messageStream('Ambiente: remote');

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
        reject(new Error('Input model server did not receive a TCP address.'));
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

async function startCrashBoundaryModelServer(): Promise<{
  baseURL: string;
  close: () => Promise<void>;
}> {
  let requestCount = 0;
  const server = createServer(async (request, response) => {
    const chunks: Buffer[] = [];
    for await (const chunk of request) {
      chunks.push(Buffer.from(chunk));
    }
    const _body = JSON.parse(Buffer.concat(chunks).toString('utf8')) as WireMessage;
    requestCount += 1;
    if (requestCount === 1) {
      const output = {
        id: 'side-effect-call',
        type: 'function_call',
        status: 'completed',
        call_id: 'side-effect-call',
        name: 'shell_exec',
        arguments: JSON.stringify({ command: 'touch crash-boundary', cwd: '/tmp' }),
      };
      const events = functionCallStream(output);
      response.writeHead(200, { 'content-type': 'text/event-stream' });
      for (const event of events) {
        response.write(`data: ${JSON.stringify(event)}\n\n`);
      }
      response.end();
      return;
    }
    response.writeHead(200, { 'content-type': 'text/event-stream' });
    request.on('aborted', () => response.destroy());
  });

  const port = await new Promise<number>((resolvePort, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      if (!address || typeof address === 'string') {
        reject(new Error('Crash boundary model server did not receive a TCP address.'));
        return;
      }
      resolvePort(address.port);
    });
  });

  return {
    baseURL: `http://127.0.0.1:${port}/zen/go/v1`,
    close: () => {
      server.closeAllConnections();
      return new Promise<void>((resolveClose, reject) => {
        server.close((error) => (error ? reject(error) : resolveClose()));
      });
    },
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

async function startPartialHangingModelServer(): Promise<{
  baseURL: string;
  close: () => Promise<void>;
}> {
  const server = createServer(async (request, response) => {
    for await (const _chunk of request) {
      // Consome o request antes de deixar o stream aberto.
    }

    response.writeHead(200, { 'content-type': 'text/event-stream' });
    for (const event of messageStream('partial')) {
      response.write(`data: ${JSON.stringify(event)}\n\n`);
      if (event.type === 'response.output_text.delta') {
        break;
      }
    }
    request.on('aborted', () => response.destroy());
  });

  const port = await new Promise<number>((resolvePort, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      if (!address || typeof address === 'string') {
        reject(new Error('Partial hanging model server did not receive a TCP address.'));
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

function sendNotificationRoundTrip(socketPath: string): Promise<WireMessage> {
  return new Promise((resolve, reject) => {
    const subscriber = createConnection(socketPath);
    const publisher = createConnection(socketPath);
    let subscriberBuffer = '';
    let publisherStarted = false;
    const fail = (error: Error) => {
      subscriber.destroy();
      publisher.destroy();
      reject(error);
    };
    subscriber.once('error', fail);
    publisher.once('error', fail);
    subscriber.setEncoding('utf8');
    publisher.setEncoding('utf8');
    subscriber.on('data', (chunk: string) => {
      subscriberBuffer += chunk;
      let newlineIndex = subscriberBuffer.indexOf('\n');
      while (newlineIndex !== -1) {
        const line = subscriberBuffer.slice(0, newlineIndex).trim();
        subscriberBuffer = subscriberBuffer.slice(newlineIndex + 1);
        newlineIndex = subscriberBuffer.indexOf('\n');
        if (!line) {
          continue;
        }
        const event = JSON.parse(line) as WireMessage;
        if (event.type === 'notification.subscribed' && !publisherStarted) {
          publisherStarted = true;
          publisher.write(
            `${JSON.stringify({
              protocol: RUNTIME_PROTOCOL,
              version: RUNTIME_PROTOCOL_VERSION,
              type: 'notification.publish',
              request_id: 'notification-integration-publish',
              notification_id: 'notification-integration-1',
              conversation_id: 'telegram:123:thread:root',
              content: 'Download concluído.',
              source: 'torrent',
              title: 'Atlas',
              level: 'success',
            })}\n`,
          );
        }
        if (event.type === 'notification.created') {
          subscriber.destroy();
          publisher.destroy();
          resolve(event);
          return;
        }
      }
    });
    subscriber.once('connect', () => {
      subscriber.write(
        `${JSON.stringify({
          protocol: RUNTIME_PROTOCOL,
          version: RUNTIME_PROTOCOL_VERSION,
          type: 'notification.subscribe',
          request_id: 'notification-integration-subscribe',
          conversation_id: '*',
        })}\n`,
      );
    });
  });
}

function sendInterruptedSessionNotification(socketPath: string): Promise<WireMessage> {
  return new Promise((resolve, reject) => {
    const socket = createConnection(socketPath);
    let buffer = '';
    socket.setEncoding('utf8');
    socket.once('error', reject);
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
        const event = JSON.parse(line) as WireMessage;
        if (event.type === 'session.interrupted') {
          socket.destroy();
          resolve(event);
          return;
        }
      }
    });
    socket.once('connect', () => {
      socket.write(
        `${JSON.stringify({
          protocol: RUNTIME_PROTOCOL,
          version: RUNTIME_PROTOCOL_VERSION,
          type: 'notification.subscribe',
          request_id: 'interrupted-session-subscribe',
          conversation_id: '*',
        })}\n`,
      );
    });
  });
}

function sendTopic(socketPath: string): Promise<WireMessage> {
  return new Promise((resolve, reject) => {
    const socket = createConnection(socketPath, () => {
      socket.end(
        `${JSON.stringify({
          protocol: RUNTIME_PROTOCOL,
          version: RUNTIME_PROTOCOL_VERSION,
          type: 'topic.request',
          request_id: 'topic-integration-request',
          conversation_id: 'telegram:123:thread:88',
          topic_id: '88',
          message_id: '60',
          action: 'closed',
          source: 'telegram',
        })}\n`,
      );
    });
    socket.setEncoding('utf8');
    socket.once('error', reject);
    socket.once('data', (chunk: string) => {
      socket.destroy();
      resolve(JSON.parse(chunk.trim()) as WireMessage);
    });
  });
}

function sendInline(socketPath: string): Promise<WireMessage> {
  return new Promise((resolve, reject) => {
    const socket = createConnection(socketPath, () => {
      socket.end(
        `${JSON.stringify({
          protocol: RUNTIME_PROTOCOL,
          version: RUNTIME_PROTOCOL_VERSION,
          type: 'inline.request',
          request_id: 'inline-integration-request',
          conversation_id: 'telegram:inline:123',
          query_id: 'inline-query-123',
          user_id: '123',
          query: '',
          offset: '',
          chat_type: 'sender',
        })}\n`,
      );
    });
    socket.setEncoding('utf8');
    socket.once('error', reject);
    socket.once('data', (chunk: string) => {
      socket.destroy();
      resolve(JSON.parse(chunk.trim()) as WireMessage);
    });
  });
}

function sendReaction(socketPath: string): Promise<WireMessage> {
  return new Promise((resolve, reject) => {
    const socket = createConnection(socketPath, () => {
      socket.end(
        `${JSON.stringify({
          protocol: RUNTIME_PROTOCOL,
          version: RUNTIME_PROTOCOL_VERSION,
          type: 'reaction.request',
          request_id: 'reaction-integration-request',
          conversation_id: 'telegram:123:thread:root',
          message_id: '51',
          action: 'added',
          reactions: ['👍'],
          source: 'telegram',
          actor_id: '7',
        })}
`,
      );
    });
    socket.setEncoding('utf8');
    socket.once('error', reject);
    let buffer = '';
    socket.on('data', (chunk: string) => {
      buffer += chunk;
      const newlineIndex = buffer.indexOf('\n');
      if (newlineIndex === -1) {
        return;
      }
      const event = JSON.parse(buffer.slice(0, newlineIndex)) as WireMessage;
      socket.destroy();
      resolve(event);
    });
  });
}

function sendTurnAndCancel(
  socketPath: string,
  conversationId: string,
  cancelConversationId = conversationId,
  cancelAfterEvent = 'turn.started',
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
          if (event.type === cancelAfterEvent && !cancelSent) {
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
          if (event.type === 'turn.cancelled' || event.type === 'error') {
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

test('cancels an active Unix runtime turn as a normal terminal event', async () => {
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
      ['session.updated', 'turn.started', 'turn.cancelled'],
    );
    assert.equal(events.at(-1)?.request_id, 'tui-cancel-request');
    assert.deepEqual(events.at(-1)?.data, { content: '' });
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
      ['session.updated', 'turn.started', 'turn.cancelled'],
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

test('preserves streamed response content when cancelling a turn', async () => {
  const model = await startPartialHangingModelServer();
  const socketPath = `/tmp/atlas-runtime-cancel-partial-${randomUUID()}.sock`;
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
    const { events } = await sendTurnAndCancel(
      socketPath,
      'cancel-partial-conversation',
      'cancel-partial-conversation',
      'message.delta',
    );

    assert.deepEqual(
      events.map((event) => event.type),
      ['session.updated', 'turn.started', 'message.delta', 'turn.cancelled'],
    );
    assert.deepEqual(events.at(-1)?.data, {
      content: 'partial',
      message_id: 'runtime-execution-message',
    });
  } finally {
    await runtime.close();
    await model.close();
  }
});

function sendTurn(
  socketPath: string,
  conversationId: string,
  input = 'execute node --version',
  requestId = 'tui-integration-request',
): Promise<WireMessage[]> {
  return new Promise((resolveTurn, rejectTurn) => {
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
          if (
            event.type === 'turn.completed' ||
            event.type === 'turn.cancelled' ||
            event.type === 'error'
          ) {
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

function sendCommand(
  socketPath: string,
  conversationId: string,
  command: 'status' | 'new' | 'stop',
  requestId: string,
): Promise<WireMessage> {
  return new Promise((resolveCommand, rejectCommand) => {
    const socket = createConnection(socketPath, () => {
      socket.write(
        `${JSON.stringify({
          protocol: RUNTIME_PROTOCOL,
          version: RUNTIME_PROTOCOL_VERSION,
          type: 'command.request',
          request_id: requestId,
          conversation_id: conversationId,
          command,
        })}\n`,
      );
    });
    let buffer = '';
    socket.once('error', rejectCommand);
    socket.setEncoding('utf8');
    socket.on('data', (chunk: string) => {
      buffer += chunk;
      const newlineIndex = buffer.indexOf('\n');
      if (newlineIndex === -1) {
        return;
      }
      const event = JSON.parse(buffer.slice(0, newlineIndex)) as WireMessage;
      socket.destroy();
      resolveCommand(event);
    });
  });
}

function sendRecovery(
  socketPath: string,
  type: 'turn.recover' | 'turn.discard',
  conversationId: string,
  requestId: string,
): Promise<WireMessage[]> {
  return new Promise((resolveTurn, rejectTurn) => {
    const socket = createConnection(socketPath, () => {
      socket.write(
        `${JSON.stringify({
          protocol: RUNTIME_PROTOCOL,
          version: RUNTIME_PROTOCOL_VERSION,
          type,
          request_id: requestId,
          conversation_id: conversationId,
          ...(type === 'turn.recover' ? { confirm: true } : {}),
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
          if (
            event.type === 'turn.completed' ||
            event.type === 'turn.cancelled' ||
            event.type === 'error'
          ) {
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

test('publishes a typed reaction completion over the public Unix protocol', async () => {
  const socketPath = `/tmp/atlas-runtime-reaction-${randomUUID()}.sock`;
  const runtime = new AtlasRuntimeServer({
    socketPath,
    runOptions: {
      apiKey: 'atlas...ey',
      capabilityRuntime,
    },
  });

  try {
    await runtime.listen();
    const event = await sendReaction(socketPath);
    assert.equal(event.type, 'reaction.completed');
    assert.equal(event.request_id, 'reaction-integration-request');
    assert.equal(event.conversation_id, 'telegram:123:thread:root');
    assert.deepEqual(event.data, {
      message_id: '51',
      action: 'added',
      reactions: ['👍'],
      source: 'telegram',
      actor_id: '7',
    });
  } finally {
    await runtime.close();
  }
});

test('publishes a typed topic update over the public Unix protocol', async () => {
  const socketPath = `/tmp/atlas-runtime-topic-${randomUUID()}.sock`;
  const runtime = new AtlasRuntimeServer({
    socketPath,
    runOptions: {
      apiKey: 'atlas...ey',
      capabilityRuntime,
    },
  });

  try {
    await runtime.listen();
    const event = await sendTopic(socketPath);
    assert.equal(event.type, 'topic.updated');
    assert.equal(event.request_id, 'topic-integration-request');
    assert.equal(event.conversation_id, 'telegram:123:thread:88');
    assert.deepEqual(event.data, {
      topic_id: '88',
      message_id: '60',
      action: 'closed',
      source: 'telegram',
    });
  } finally {
    await runtime.close();
  }
});

test('publishes inline results over the public Unix protocol', async () => {
  const socketPath = `/tmp/atlas-runtime-inline-${randomUUID()}.sock`;
  const runtime = new AtlasRuntimeServer({
    socketPath,
    runOptions: {
      apiKey: 'atlas-inline-test-key',
    },
  });

  try {
    await runtime.listen();
    const event = await sendInline(socketPath);
    assert.equal(event.type, 'inline.completed');
    assert.equal(event.request_id, 'inline-integration-request');
    assert.equal(event.conversation_id, 'telegram:inline:123');
    const data = event.data as { query_id: string; results: Array<{ message_text: string }> };
    assert.equal(data.query_id, 'inline-query-123');
    assert.deepEqual(data.results, []);
  } finally {
    await runtime.close();
  }
});

test('replays a persisted restart interruption to notification subscribers', async () => {
  const socketPath = `/tmp/atlas-runtime-interrupted-${randomUUID()}.sock`;
  const ledgerPath = `/tmp/atlas-runtime-interrupted-${randomUUID()}.json`;
  const conversationId = 'telegram:123:thread:root';
  await writeFile(
    ledgerPath,
    JSON.stringify({
      version: 2,
      requests: [
        {
          request: {
            request_id: 'interrupted-request',
            conversation_id: conversationId,
            input: 'operação em andamento',
          },
          fingerprint: 'test-fingerprint',
          state: 'executing',
          events: [],
        },
      ],
    }),
  );
  const runtime = new AtlasRuntimeServer({
    socketPath,
    requestLedgerPath: ledgerPath,
    runOptions: {
      apiKey: 'atlas...ey',
      capabilityRuntime,
    },
  });

  try {
    await runtime.listen();
    const event = await sendInterruptedSessionNotification(socketPath);
    assert.equal(event.type, 'session.interrupted');
    assert.equal(event.request_id, 'interrupted-request');
    assert.equal(event.conversation_id, conversationId);
    assert.deepEqual(event.data, {
      request_id: 'interrupted-request',
      detected_at: (event.data as { detected_at: string }).detected_at,
      reason: 'restart',
    });
  } finally {
    await runtime.close();
    await rm(ledgerPath, { force: true });
  }
});

test('exposes restart_interrupted and associates the next input without replaying the old request', async () => {
  const model = await startStreamingModelServer();
  const socketPath = `/tmp/atlas-runtime-session-recovery-${randomUUID()}.sock`;
  const ledgerPath = `/tmp/atlas-runtime-session-recovery-${randomUUID()}.json`;
  const conversationId = 'telegram:123:thread:root';
  await writeFile(
    ledgerPath,
    JSON.stringify({
      version: 2,
      requests: [
        {
          request: {
            request_id: 'interrupted-request',
            conversation_id: conversationId,
            input: 'operação em andamento',
          },
          fingerprint: 'test-fingerprint',
          state: 'executing',
          events: [],
        },
      ],
    }),
  );
  const runtime = new AtlasRuntimeServer({
    socketPath,
    requestLedgerPath: ledgerPath,
    runOptions: {
      apiKey: 'atlas...ey',
      baseURL: model.baseURL,
      capabilityRuntime,
    },
  });

  try {
    await runtime.listen();
    const statusEvent = await sendCommand(
      socketPath,
      conversationId,
      'status',
      'session-status-request',
    );
    const statusSession = (
      statusEvent.data as { session: { status: string; interrupted_request_id: string } }
    ).session;
    assert.equal(statusSession.status, 'restart_interrupted');
    assert.equal(statusSession.interrupted_request_id, 'interrupted-request');

    const events = await sendTurn(
      socketPath,
      conversationId,
      'continue safely',
      'next-input-request',
    );
    assert.equal(events.at(-1)?.type, 'turn.completed');
    assert.equal(events.at(-1)?.request_id, 'next-input-request');

    const persisted = JSON.parse(await readFile(ledgerPath, 'utf8')) as {
      requests: Array<{
        request: { request_id: string };
        state: string;
        interruption?: { resumed_by_request_id?: string; resumed_at?: string };
      }>;
    };
    const interrupted = persisted.requests.find(
      (record) => record.request.request_id === 'interrupted-request',
    );
    assert.equal(interrupted?.state, 'ambiguous');
    assert.equal(interrupted?.interruption?.resumed_by_request_id, 'next-input-request');
    assert.equal(typeof interrupted?.interruption?.resumed_at, 'string');
  } finally {
    await runtime.close();
    await model.close();
    await rm(ledgerPath, { force: true });
  }
});

test('delivers published notifications to persistent subscribers', async () => {
  const socketPath = `/tmp/atlas-runtime-notification-${randomUUID()}.sock`;
  const runtime = new AtlasRuntimeServer({
    socketPath,
    runOptions: {
      apiKey: 'atlas...ey',
      capabilityRuntime,
    },
  });

  try {
    await runtime.listen();
    const event = await sendNotificationRoundTrip(socketPath);
    assert.equal(event.type, 'notification.created');
    assert.equal(event.request_id, 'notification-integration-publish');
    assert.deepEqual(event.data, {
      notification_id: 'notification-integration-1',
      conversation_id: 'telegram:123:thread:root',
      content: 'Download concluído.',
      source: 'torrent',
      title: 'Atlas',
      level: 'success',
    });
  } finally {
    await runtime.close();
  }
});

test('suspends and resumes a real Runtime turn through input.respond', async () => {
  const model = await startInputStreamingModelServer();
  const socketPath = `/tmp/atlas-runtime-input-${randomUUID()}.sock`;
  const runtime = new AtlasRuntimeServer({
    socketPath,
    runOptions: {
      apiKey: 'atlas-runtime-input-key',
      baseURL: model.baseURL,
      capabilityRuntime,
    },
  });

  try {
    await runtime.listen();
    const events = await new Promise<WireMessage[]>((resolveTurn, rejectTurn) => {
      const socket = createConnection(socketPath, () => {
        socket.write(
          `${JSON.stringify({
            protocol: RUNTIME_PROTOCOL,
            version: RUNTIME_PROTOCOL_VERSION,
            type: 'turn.request',
            request_id: 'input-integration-request',
            conversation_id: 'input-integration-conversation',
            input: 'pergunte o ambiente',
          })}\n`,
        );
      });
      const events: WireMessage[] = [];
      let buffer = '';
      let inputSent = false;
      socket.once('error', rejectTurn);
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
          const event = JSON.parse(line) as WireMessage;
          events.push(event);
          if (event.type === 'input.requested' && !inputSent) {
            inputSent = true;
            const responseSocket = createConnection(socketPath, () => {
              responseSocket.end(
                `${JSON.stringify({
                  protocol: RUNTIME_PROTOCOL,
                  version: RUNTIME_PROTOCOL_VERSION,
                  type: 'input.respond',
                  request_id: 'input-response-request',
                  conversation_id: 'input-integration-conversation',
                  input_id: 'input-call',
                  value: 'remote',
                })}\n`,
              );
            });
            responseSocket.once('error', rejectTurn);
          }
          if (event.type === 'turn.completed' || event.type === 'error') {
            socket.destroy();
            resolveTurn(events);
            return;
          }
        }
      });
    });

    assert.deepEqual(
      events.map((event) => event.type),
      [
        'session.updated',
        'turn.started',
        'input.requested',
        'input.resolved',
        'message.delta',
        'message.completed',
        'context.updated',
        'turn.completed',
      ],
    );
    assert.equal(model.requests.length, 2);
    assert.match(JSON.stringify(model.requests[1]?.input), /remote/);
  } finally {
    await runtime.close();
    await model.close();
  }
});

test('reconciles a repeated turn.request without executing the model twice', async () => {
  const model = await startStreamingModelServer();
  const socketPath = `/tmp/atlas-runtime-idempotent-${randomUUID()}.sock`;
  const runtime = new AtlasRuntimeServer({
    socketPath,
    runOptions: {
      apiKey: 'atlas-runtime-idempotent-key',
      baseURL: model.baseURL,
      capabilityRuntime,
    },
  });

  try {
    await runtime.listen();
    const first = await sendTurn(socketPath, 'runtime-idempotent-conversation');
    const second = await sendTurn(socketPath, 'runtime-idempotent-conversation');
    assert.deepEqual(second, first);
    assert.equal(model.requests.length, 1);
  } finally {
    await runtime.close();
    await model.close();
  }
});

test('does not replay an ambiguous request after a crash boundary with a side effect', async () => {
  const model = await startCrashBoundaryModelServer();
  const directory = `/tmp/atlas-runtime-crash-${randomUUID()}`;
  const socketPath = `${directory}.sock`;
  const ledgerPath = `${directory}.requests.json`;
  const request = {
    protocol: RUNTIME_PROTOCOL,
    version: RUNTIME_PROTOCOL_VERSION,
    type: 'turn.request' as const,
    request_id: 'crash-boundary-request',
    conversation_id: 'crash-boundary-conversation',
    input: 'execute side effect',
  };
  let sideEffects = 0;
  const effectRuntime: CapabilityRuntime = {
    ...shellCapabilityRuntime,
    execute: async (id, target, arguments_) => {
      sideEffects += 1;
      return shellCapabilityRuntime.execute(id, target, arguments_);
    },
  };

  const firstRuntime = new AtlasRuntimeServer({
    socketPath,
    requestLedgerPath: ledgerPath,
    runOptions: {
      apiKey: 'atlas-crash-boundary-key',
      baseURL: model.baseURL,
      capabilityRuntime: effectRuntime,
    },
  });
  let turnSocket: ReturnType<typeof createConnection> | undefined;
  let secondRuntime: AtlasRuntimeServer | undefined;
  try {
    await firstRuntime.listen();
    await new Promise<void>((resolve, reject) => {
      turnSocket = createConnection(socketPath, () => {
        turnSocket?.write(`${JSON.stringify(request)}\n`);
      });
      turnSocket.once('error', reject);
      turnSocket.setEncoding('utf8');
      turnSocket.on('data', (chunk: string) => {
        if (chunk.includes('execution.completed')) {
          resolve();
        }
      });
    });
    await firstRuntime.close();
    assert.equal(sideEffects, 1);
    secondRuntime = new AtlasRuntimeServer({
      socketPath,
      requestLedgerPath: ledgerPath,
      runOptions: {
        apiKey: 'atlas-crash-boundary-key',
        baseURL: model.baseURL,
        capabilityRuntime: effectRuntime,
      },
    });
    await secondRuntime.listen();
    const events = await sendTurn(
      socketPath,
      request.conversation_id,
      request.input,
      request.request_id,
    );
    assert.equal(events.at(-1)?.type, 'error');
    assert.match(String((events.at(-1)?.data as WireMessage).message), /ambiguous/);
    assert.equal(sideEffects, 1);
    const discarded = await sendRecovery(
      socketPath,
      'turn.discard',
      request.conversation_id,
      request.request_id,
    );
    assert.equal(discarded.at(-1)?.type, 'error');
    assert.equal((discarded.at(-1)?.data as WireMessage).code, 'recovery_discarded');
    const afterDiscard = await sendTurn(
      socketPath,
      request.conversation_id,
      request.input,
      request.request_id,
    );
    assert.equal(afterDiscard.at(-1)?.type, 'error');
    assert.equal((afterDiscard.at(-1)?.data as WireMessage).code, 'recovery_discarded');
    assert.equal(sideEffects, 1);
  } finally {
    turnSocket?.destroy();
    await firstRuntime.close();
    await secondRuntime?.close();
    await rm(directory, { recursive: true, force: true });
    await rm(ledgerPath, { force: true });
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

test('publishes shell execution lifecycle events over the public Unix protocol', async () => {
  const model = await startShellStreamingModelServer();
  const socketPath = `/tmp/atlas-runtime-shell-execution-${randomUUID()}.sock`;
  const runtime = new AtlasRuntimeServer({
    socketPath,
    runOptions: {
      apiKey: 'atlas-runtime-shell-execution-key',
      baseURL: model.baseURL,
      capabilityRuntime: shellCapabilityRuntime,
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
      capability: 'shell.exec',
      program: 'sh',
      args: ['-c', 'node --version'],
      cwd: '/tmp',
      target: 'local',
    });
    assert.deepEqual(completed, {
      execution_id: 'execution-call',
      capability: 'shell.exec',
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
      capabilityRuntime: shellCapabilityRuntime,
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
      capability: 'shell.exec',
      program: 'sh',
      args: ['-c', 'node --version'],
      cwd: '/tmp',
      target: 'local',
    });
    assert.equal((events.at(-1)?.data as WireMessage).content, 'Executed node --version: v22.x.x');
    assert.deepEqual(events.find((event) => event.type === 'context.updated')?.data, {
      used_tokens: 10_400,
      context_window: 256_000,
    });

    // Um round-trip para a tool e outro para a resposta: sem list_tools/describe.
    assert.equal(model.requests.length, 2);
    const toolNames = ((model.requests[0]?.tools as WireMessage[] | undefined) ?? []).map(
      (tool) => tool.name,
    );
    assert.equal(toolNames[0], 'shell_exec');
    assert.ok(toolNames.includes('list_tools'));
    assert.doesNotMatch(JSON.stringify(model.requests[0]?.input), /"name":"(discover|describe)"/);
  } finally {
    await runtime.close();
    await model.close();
  }
});

test('publishes failed shell execution status and output over the public protocol', async () => {
  const model = await startShellStreamingModelServer();
  const socketPath = `/tmp/atlas-runtime-shell-execution-failure-${randomUUID()}.sock`;
  const runtime = new AtlasRuntimeServer({
    socketPath,
    runOptions: {
      apiKey: 'atlas-runtime-shell-execution-failure-key',
      baseURL: model.baseURL,
      capabilityRuntime: shellCapabilityRuntime,
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
      capability: 'shell.exec',
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

test('normalizes generic tool output before publishing tool.completed', async () => {
  const requests: WireMessage[] = [];
  const server = createServer(async (request, response) => {
    const chunks: Buffer[] = [];
    for await (const chunk of request) {
      chunks.push(Buffer.from(chunk));
    }
    requests.push(JSON.parse(Buffer.concat(chunks).toString('utf8')) as WireMessage);

    const output =
      requests.length === 1
        ? {
            id: 'read-call',
            type: 'function_call',
            status: 'completed',
            call_id: 'read-call',
            name: 'filesystem_read',
            arguments: JSON.stringify({ path: 'README.md' }),
          }
        : undefined;
    const events = output ? functionCallStream(output) : messageStream('Arquivo lido.');

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
        reject(new Error('Tool output model server did not receive a TCP address.'));
        return;
      }
      resolvePort(address.port);
    });
  });

  const definition = {
    id: 'filesystem.read',
    type: 'tool' as const,
    summary: 'ler o conteudo de um arquivo',
    description: 'ler o arquivo completo',
    schema: {
      type: 'object',
      properties: { path: { type: 'string' } },
      required: ['path'],
      additionalProperties: false,
    },
  };
  const toolOutputRuntime: CapabilityRuntime = {
    discover: async () => [],
    listTools: async () => [],
    getDefinition: async (id) => (id === definition.id ? definition : undefined),
    getSkill: async () => undefined,
    execute: async (_id, target, arguments_) => ({
      target,
      status: 'success',
      error: '',
      output: { path: arguments_.path, content: 'conteudo real' },
    }),
  };

  const events: Array<{ type: string; output?: string; target?: string }> = [];
  try {
    await runAtlas('Leia o README.md.', {
      apiKey: 'atlas...ey',
      baseURL: `http://127.0.0.1:${port}/zen/go/v1`,
      conversationId: 'normalized-tool-output-conversation',
      capabilityRuntime: toolOutputRuntime,
      onEvent: (event) => {
        if (event.type === 'tool.completed') {
          events.push({ type: event.type, output: event.output });
        }
        if (event.type === 'tool.started') {
          events.push({ type: event.type, target: event.target });
        }
      },
    });
  } finally {
    await new Promise<void>((resolveClose, reject) => {
      server.close((error) => (error ? reject(error) : resolveClose()));
    });
  }

  assert.equal(events.length, 2);
  assert.equal(events[0]?.target, 'README.md');
  const output = events[1]?.output;
  assert.ok(output);
  // O output publico e o resultado decodificado, sem o wrapper aninhado do SDK.
  assert.deepEqual(JSON.parse(output), {
    target: 'local',
    status: 'success',
    error: '',
    output: { path: 'README.md', content: 'conteudo real' },
  });
  assert.doesNotMatch(output, /\\"target\\"/);
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
