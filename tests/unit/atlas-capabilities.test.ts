import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { test } from 'node:test';

import type { CapabilityDefinition, CapabilityRuntime } from '../../src/capability-runtime.js';
import { runAtlas } from '../../src/index.js';

type RequestBody = Record<string, unknown>;

function responseEnvelope(
  requestNumber: number,
  input: RequestBody,
  output: RequestBody[],
): RequestBody {
  return {
    id: `capability-response-${requestNumber}`,
    object: 'response',
    created_at: 1,
    status: 'completed',
    model: 'gpt-5.6-luna',
    output,
    usage: {
      input_tokens: 1,
      output_tokens: 1,
      total_tokens: 2,
    },
    metadata: {
      received_input: JSON.stringify(input),
    },
  };
}

function responseBody(requestNumber: number, input: RequestBody): RequestBody {
  const output =
    requestNumber === 1
      ? [
          {
            id: 'function-call-discover',
            type: 'function_call',
            status: 'completed',
            call_id: 'discover-call',
            name: 'discover',
            arguments: JSON.stringify({ path: 'process' }),
          },
        ]
      : requestNumber === 2
        ? [
            {
              id: 'function-call-process-exec',
              type: 'function_call',
              status: 'completed',
              call_id: 'process-exec-call',
              name: materializedToolName(input),
              arguments: JSON.stringify({ program: 'node', args: ['--version'] }),
            },
          ]
        : [
            {
              id: 'final-message',
              type: 'message',
              status: 'completed',
              role: 'assistant',
              content: [
                {
                  type: 'output_text',
                  text: `Executed node --version: ${process.version}`,
                  annotations: [],
                },
              ],
            },
          ];

  return responseEnvelope(requestNumber, input, output);
}

async function startCapabilityAgentServer(
  responseFactory: (requestNumber: number, input: RequestBody) => RequestBody = responseBody,
): Promise<{
  baseURL: string;
  requests: RequestBody[];
  close: () => Promise<void>;
}> {
  const requests: RequestBody[] = [];
  const server = createServer(async (request, response) => {
    const chunks: Buffer[] = [];
    for await (const chunk of request) {
      chunks.push(Buffer.from(chunk));
    }
    const body = JSON.parse(Buffer.concat(chunks).toString('utf8')) as RequestBody;
    requests.push(body);

    response.writeHead(200, { 'content-type': 'application/json' });
    response.end(JSON.stringify(responseFactory(requests.length, body)));
  });

  const port = await new Promise<number>((resolvePort, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      if (!address || typeof address === 'string') {
        reject(new Error('Capability Agent test server did not receive a TCP address.'));
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

function tools(request: RequestBody): RequestBody[] {
  return (request.tools as RequestBody[] | undefined) ?? [];
}

function materializedToolName(request: RequestBody): string {
  const tool = tools(request).find((candidate) => candidate.name !== 'discover');
  if (!tool || typeof tool.name !== 'string') {
    throw new Error('Capability test server did not receive a materialized tool.');
  }
  return tool.name;
}

function materializedToolNames(request: RequestBody): string[] {
  return tools(request).flatMap((tool) =>
    typeof tool.name === 'string' && tool.name !== 'discover' ? [tool.name] : [],
  );
}

function materializedToolNameAt(request: RequestBody, index: number): string {
  const name = materializedToolNames(request)[index];
  if (name === undefined) {
    throw new Error(`Capability test server did not receive materialized tool ${index}.`);
  }
  return name;
}

test('discovers and executes process.exec as a directly materialized Agent tool', async () => {
  const server = await startCapabilityAgentServer();

  try {
    const result = await runAtlas('Execute node --version.', {
      apiKey: 'atlas-capabilities-test-key',
      baseURL: server.baseURL,
      conversationId: 'capability-conversation',
    });

    assert.equal(result.finalOutput, `Executed node --version: ${process.version}`);
    assert.equal(server.requests.length, 3);

    const firstRequest = server.requests[0] as RequestBody;
    const firstTools = tools(firstRequest);
    assert.deepEqual(
      firstTools.map((tool) => tool.name),
      ['discover'],
    );
    assert.match(JSON.stringify(firstRequest), /process - executar e gerenciar processos/);

    const secondRequest = server.requests[1] as RequestBody;
    const materializedTool = tools(secondRequest).find((tool) => tool.name !== 'discover');
    assert.ok(materializedTool);
    assert.match(materializedTool.name as string, /^process_exec_[a-f0-9]{16}$/);
    assert.equal(materializedTool.description, 'executar um programa local diretamente, sem shell');
    const schema = materializedTool.parameters as RequestBody;
    const properties = schema.properties as RequestBody;
    assert.ok(properties.program);
    assert.ok(properties.args);
    assert.deepEqual(schema.required, ['program']);
    assert.match(JSON.stringify(secondRequest.input), /\\"path\\":\\"process\\"/);
    assert.match(JSON.stringify(secondRequest.input), /process\.exec/);
    assert.doesNotMatch(JSON.stringify(firstTools), /get_definition|call_tool/);

    const thirdRequest = server.requests[2] as RequestBody;
    assert.match(JSON.stringify(thirdRequest.input), /process-exec-call/);
    assert.match(JSON.stringify(thirdRequest.input), new RegExp(materializedTool.name as string));
    assert.match(
      JSON.stringify(thirdRequest.input),
      new RegExp(process.version.replaceAll('.', '\\.')),
    );
  } finally {
    await server.close();
  }
});

test('keeps provider names safe and dispatches colliding IDs to their capabilities', async () => {
  const definitions: CapabilityDefinition[] = [
    {
      id: 'foo.bar',
      type: 'tool',
      summary: 'primeira capability de teste',
      description: 'executa a primeira capability de teste',
      schema: {
        type: 'object',
        properties: { value: { type: 'string' } },
        required: ['value'],
        additionalProperties: false,
      },
    },
    {
      id: 'foo_bar',
      type: 'tool',
      summary: 'segunda capability de teste',
      description: 'executa a segunda capability de teste',
      schema: {
        type: 'object',
        properties: { value: { type: 'string' } },
        required: ['value'],
        additionalProperties: false,
      },
    },
    {
      id: 'already_valid',
      type: 'tool',
      summary: 'capability com ID provider-safe',
      description: 'executa uma capability com ID provider-safe',
      schema: {
        type: 'object',
        properties: { value: { type: 'string' } },
        required: ['value'],
        additionalProperties: false,
      },
    },
  ];
  const definitionsById = new Map(definitions.map((definition) => [definition.id, definition]));
  const executed: Array<{ id: string; arguments_: Record<string, unknown> }> = [];
  const capabilityRuntime: CapabilityRuntime = {
    discover: async (request = {}) =>
      request.path === undefined
        ? [{ id: 'demo', type: 'group', summary: 'capabilities de teste' }]
        : definitions.map(({ id, type, summary }) => ({ id, type, summary })),
    getDefinition: async (id) => definitionsById.get(id),
    execute: async (id, target, arguments_) => {
      executed.push({ id, arguments_ });
      return { target, status: 'ok', error: '', capability: id };
    },
  };
  const server = await startCapabilityAgentServer((requestNumber, input) => {
    const output =
      requestNumber === 1
        ? [
            {
              id: 'function-call-discover',
              type: 'function_call',
              status: 'completed',
              call_id: 'discover-call',
              name: 'discover',
              arguments: JSON.stringify({ path: 'demo' }),
            },
          ]
        : requestNumber <= 3
          ? [
              {
                id: `function-call-${requestNumber}`,
                type: 'function_call',
                status: 'completed',
                call_id: `capability-call-${requestNumber}`,
                name: materializedToolNameAt(input, requestNumber - 2),
                arguments: JSON.stringify({ value: `value-${requestNumber}` }),
              },
            ]
          : [
              {
                id: 'final-message',
                type: 'message',
                status: 'completed',
                role: 'assistant',
                content: [
                  { type: 'output_text', text: 'Capabilities executadas.', annotations: [] },
                ],
              },
            ];

    return responseEnvelope(requestNumber, input, output);
  });

  try {
    const result = await runAtlas('Execute as capabilities de teste.', {
      apiKey: 'atlas-capabilities-collision-test-key',
      baseURL: server.baseURL,
      conversationId: 'capability-collision-conversation',
      capabilityRuntime,
    });

    assert.equal(result.finalOutput, 'Capabilities executadas.');
    const materializedTools = tools(server.requests[1] as RequestBody).filter(
      (tool) => tool.name !== 'discover',
    );
    const names = materializedTools.map((tool) => tool.name as string);
    assert.equal(names.length, 3);
    assert.ok(names.every((name) => /^[a-zA-Z0-9_-]+$/.test(name)));
    assert.match(names[0] as string, /^foo_bar_[a-f0-9]{16}$/);
    assert.match(names[1] as string, /^foo_bar_[a-f0-9]{16}$/);
    assert.notEqual(names[0], names[1]);
    assert.match(names[2] as string, /^already_valid_[a-f0-9]{16}$/);
    assert.deepEqual(
      executed.map(({ id }) => id),
      ['foo.bar', 'foo_bar'],
    );
    assert.deepEqual(
      executed.map(({ arguments_ }) => arguments_),
      [{ value: 'value-2' }, { value: 'value-3' }],
    );
    assert.match(JSON.stringify(server.requests[1]?.input), /foo\.bar/);
  } finally {
    await server.close();
  }
});
