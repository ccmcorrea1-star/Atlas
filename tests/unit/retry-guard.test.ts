import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { test } from 'node:test';

import {
  HookableCapabilityRuntime,
  RetryGuard,
  type CapabilityExecutionHookContext,
} from '../../src/capability-hooks.js';
import {
  NativeCapabilityRuntime,
  type CapabilityExecutionResult,
  type CapabilityRuntime,
} from '../../src/capability-runtime.js';
import { runAtlas } from '../../src/index.js';

type RequestBody = Record<string, unknown>;

function context(
  overrides: Partial<CapabilityExecutionHookContext> = {},
): CapabilityExecutionHookContext {
  return {
    id: 'web.search',
    target: 'local',
    arguments: { query: 'noticias de hoje' },
    ...overrides,
  };
}

function failing(status = 'failed'): CapabilityExecutionResult {
  return { target: 'local', status, error: 'previous failure' };
}

test('fingerprint is stable regardless of JSON key order', () => {
  const guard = new RetryGuard();
  assert.equal(
    guard.fingerprint({ id: 'x', target: 'local', arguments: { a: 1, b: { c: 2, d: 3 } } }),
    guard.fingerprint({ id: 'x', target: 'local', arguments: { b: { d: 3, c: 2 }, a: 1 } }),
  );
  assert.notEqual(
    guard.fingerprint(context()),
    guard.fingerprint(context({ arguments: { query: 'outra noticia' } })),
  );
});

test('blocks an identical call after a failure', () => {
  const guard = new RetryGuard();
  guard.after_execute(context(), { target: 'local', status: 'failed', error: 'boom' });

  const blocked = guard.before_execute(context());
  assert.ok(blocked);
  assert.equal(blocked.status, 'failed');
  assert.equal(blocked.blocked, true);
  assert.match(blocked.error, /already failed in this turn/);
});

test('allows the same capability with different arguments or target', () => {
  const guard = new RetryGuard();
  guard.after_execute(context(), failing());

  assert.equal(guard.before_execute(context({ arguments: { query: 'outra noticia' } })), undefined);
  assert.equal(guard.before_execute(context({ target: 'remote' })), undefined);
});

test('does not block after a successful call', () => {
  const guard = new RetryGuard();
  guard.after_execute(context(), { target: 'local', status: 'success', error: '' });

  assert.equal(guard.before_execute(context()), undefined);
});

test('blocks every retry-blocking status including unavailable', () => {
  for (const status of ['failed', 'unavailable', 'invalid_arguments', 'permission_denied']) {
    const guard = new RetryGuard();
    const scoped = context({ arguments: { query: status } });
    guard.after_execute(scoped, { target: 'local', status, error: 'x' });

    const blocked = guard.before_execute(scoped);
    assert.ok(blocked, `expected status ${status} to block`);
    assert.equal(blocked.status, status);
  }
});

test('allows a retry after explicit invalidation', () => {
  const guard = new RetryGuard();
  guard.after_execute(context(), failing());
  assert.ok(guard.before_execute(context()));

  guard.invalidate();
  assert.equal(guard.before_execute(context()), undefined);
});

test('HookableCapabilityRuntime short-circuits a blocked call without executing it', async () => {
  let executions = 0;
  const runtime: CapabilityRuntime = {
    discover: async () => [],
    listTools: async () => [],
    getDefinition: async () => undefined,
    execute: async (id, target) => {
      executions += 1;
      return { target, status: 'unavailable', error: `no provider for ${id}` };
    },
  };
  const hooked = new HookableCapabilityRuntime(runtime, new RetryGuard());

  const first = await hooked.execute('web.search', 'local', { query: 'atlas' });
  const second = await hooked.execute('web.search', 'local', { query: 'atlas' });

  assert.equal(executions, 1);
  assert.equal(first.status, 'unavailable');
  assert.equal(second.status, 'unavailable');
  assert.equal(second.blocked, true);
  assert.match(second.error, /already failed in this turn/);
});

test('HookableCapabilityRuntime reports thrown executions to after_execute', async () => {
  const observed: CapabilityExecutionResult[] = [];
  const runtime: CapabilityRuntime = {
    discover: async () => [],
    listTools: async () => [],
    getDefinition: async () => undefined,
    execute: async () => {
      throw new Error('runtime exploded');
    },
  };
  const hooked = new HookableCapabilityRuntime(runtime, {
    after_execute: (_context, result) => {
      observed.push(result);
    },
  });

  await assert.rejects(hooked.execute('web.search', 'local', { query: 'atlas' }), /exploded/);
  assert.deepEqual(observed, [{ target: 'local', status: 'failed', error: 'runtime exploded' }]);
});

function executeCall(
  requestNumber: number,
  id: string,
  arguments_: RequestBody,
  callId: string,
): RequestBody[] {
  return [
    {
      id: `function-call-${requestNumber}`,
      type: 'function_call',
      status: 'completed',
      call_id: callId,
      name: 'execute',
      arguments: JSON.stringify({ id, arguments: arguments_ }),
    },
  ];
}

function finalMessage(requestNumber: number, text: string): RequestBody[] {
  return [
    {
      id: `final-message-${requestNumber}`,
      type: 'message',
      status: 'completed',
      role: 'assistant',
      content: [{ type: 'output_text', text, annotations: [] }],
    },
  ];
}

async function startAgentServer(
  outputFor: (requestNumber: number) => RequestBody[],
): Promise<{ baseURL: string; requests: RequestBody[]; close: () => Promise<void> }> {
  const requests: RequestBody[] = [];
  const server = createServer(async (request, response) => {
    const chunks: Buffer[] = [];
    for await (const chunk of request) {
      chunks.push(Buffer.from(chunk));
    }
    const body = JSON.parse(Buffer.concat(chunks).toString('utf8')) as RequestBody;
    requests.push(body);

    response.writeHead(200, { 'content-type': 'application/json' });
    response.end(
      JSON.stringify({
        id: `retry-response-${requests.length}`,
        object: 'response',
        created_at: 1,
        status: 'completed',
        model: 'gpt-5.6-luna',
        output: outputFor(requests.length),
        usage: { input_tokens: 1, output_tokens: 1, total_tokens: 2 },
      }),
    );
  });

  const port = await new Promise<number>((resolvePort, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      if (!address || typeof address === 'string') {
        reject(new Error('Retry guard test server did not receive a TCP address.'));
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

test('blocks an identical failing capability call within the same turn', async () => {
  const executed: string[] = [];
  const capabilityRuntime: CapabilityRuntime = {
    discover: async () => [],
    listTools: async () => [],
    getDefinition: async () => undefined,
    execute: async (id, target) => {
      executed.push(id);
      return { target, status: 'unavailable', error: 'no search provider configured' };
    },
  };
  const server = await startAgentServer((requestNumber) =>
    requestNumber === 1
      ? executeCall(1, 'web.search', { query: 'noticias de hoje' }, 'search-call-1')
      : requestNumber === 2
        ? executeCall(2, 'web.search', { query: 'noticias de hoje' }, 'search-call-2')
        : finalMessage(requestNumber, 'done'),
  );

  try {
    const result = await runAtlas('busque noticias de hoje', {
      apiKey: 'retry-guard-same-turn-key',
      baseURL: server.baseURL,
      conversationId: 'retry-guard-same-turn',
      capabilityRuntime,
    });

    assert.equal(result.finalOutput, 'done');
    assert.equal(executed.length, 1);
    assert.match(JSON.stringify(server.requests[2]?.input), /already failed in this turn/);
  } finally {
    await server.close();
  }
});

test('allows the same failing call again in a new turn', async () => {
  const executed: string[] = [];
  const capabilityRuntime: CapabilityRuntime = {
    discover: async () => [],
    listTools: async () => [],
    getDefinition: async () => undefined,
    execute: async (id, target) => {
      executed.push(id);
      return { target, status: 'unavailable', error: 'no search provider configured' };
    },
  };
  const server = await startAgentServer((requestNumber) =>
    requestNumber === 1 || requestNumber === 3
      ? executeCall(
          requestNumber,
          'web.search',
          { query: 'noticias de hoje' },
          `search-call-${requestNumber}`,
        )
      : finalMessage(requestNumber, `turn-${requestNumber} done`),
  );

  try {
    const options = {
      apiKey: 'retry-guard-new-turn-key',
      baseURL: server.baseURL,
      conversationId: 'retry-guard-new-turn',
      capabilityRuntime,
    };
    const first = await runAtlas('busque noticias de hoje', options);
    const second = await runAtlas('busque as mesmas noticias de novo', options);

    assert.equal(first.finalOutput, 'turn-2 done');
    assert.equal(second.finalOutput, 'turn-4 done');
    assert.equal(executed.length, 2);
    assert.doesNotMatch(JSON.stringify(server.requests[3]?.input), /already failed in this turn/);
  } finally {
    await server.close();
  }
});

test('executes web.search without provider once and blocks the identical retry', async () => {
  const savedProvider = process.env.ATLAS_WEB_SEARCH_COMMAND;
  delete process.env.ATLAS_WEB_SEARCH_COMMAND;

  const native = new NativeCapabilityRuntime();
  let executions = 0;
  const capabilityRuntime: CapabilityRuntime = {
    discover: (request) => native.discover(request),
    listTools: (request) => native.listTools(request),
    getDefinition: (id) => native.getDefinition(id),
    execute: async (id, target, arguments_, options) => {
      executions += 1;
      return native.execute(id, target, arguments_, options);
    },
  };
  const server = await startAgentServer((requestNumber) =>
    requestNumber === 1
      ? executeCall(1, 'web.search', { query: 'noticias de hoje' }, 'search-call-1')
      : requestNumber === 2
        ? executeCall(2, 'web.search', { query: 'noticias de hoje' }, 'search-call-2')
        : finalMessage(requestNumber, 'done'),
  );

  try {
    const result = await runAtlas('busque noticias de hoje', {
      apiKey: 'retry-guard-web-search-key',
      baseURL: server.baseURL,
      conversationId: 'retry-guard-web-search',
      capabilityRuntime,
    });

    assert.equal(result.finalOutput, 'done');
    assert.equal(executions, 1);
    const retryOutput = JSON.stringify(server.requests[2]?.input);
    assert.match(retryOutput, /already failed in this turn/);
    assert.match(retryOutput, /no search provider configured/);
  } finally {
    await server.close();
    if (savedProvider !== undefined) {
      process.env.ATLAS_WEB_SEARCH_COMMAND = savedProvider;
    }
  }
});
