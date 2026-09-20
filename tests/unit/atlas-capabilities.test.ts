import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { test } from 'node:test';

import type { CapabilityRuntime, ToolDefinition } from '../../src/capability-runtime.js';
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
            arguments: JSON.stringify({ query: 'executar comando' }),
          },
        ]
      : requestNumber === 2
        ? [
            {
              id: 'function-call-describe',
              type: 'function_call',
              status: 'completed',
              call_id: 'describe-call',
              name: 'describe',
              arguments: JSON.stringify({ id: 'shell.exec' }),
            },
          ]
        : requestNumber === 3
          ? [
              {
                id: 'function-call-shell-exec',
                type: 'function_call',
                status: 'completed',
                call_id: 'shell-exec-call',
                name: 'execute',
                arguments: JSON.stringify({
                  id: 'shell.exec',
                  arguments: { command: 'node --version' },
                }),
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

// As core tools vem do Registry, na ordem declarada em atlas.ts.
const materializedToolNames = [
  'filesystem_read',
  'filesystem_list',
  'filesystem_search',
  'filesystem_glob',
  'filesystem_patch',
  'shell_exec',
  'system_info',
  'lsp_diagnostics',
  'git_status',
  'git_diff',
  'web_search',
  'web_fetch',
];
const baseToolNames = ['list_tools', 'discover', 'skill', 'describe', 'execute'];

test('materializes core capabilities as direct tools from the registry', async () => {
  const definition: ToolDefinition = {
    id: 'filesystem.read',
    type: 'tool',
    summary: 'ler o conteudo de um arquivo',
    description: 'ler o arquivo completo ou por intervalo de linhas',
    schema: {
      type: 'object',
      properties: {
        path: { type: 'string' },
        offset: { type: 'integer' },
        limit: { type: 'integer' },
      },
      required: ['path'],
      additionalProperties: false,
    },
  };
  const executed: Array<{ id: string; arguments_: Record<string, unknown> }> = [];
  const capabilityRuntime: CapabilityRuntime = {
    discover: async () => [],
    listTools: async () => [
      {
        id: definition.id,
        type: definition.type,
        summary: definition.summary,
        group: 'filesystem',
      },
    ],
    getDefinition: async (id) => (id === definition.id ? definition : undefined),
    getSkill: async () => undefined,
    execute: async (id, target, arguments_) => {
      executed.push({ id, arguments_ });
      return { target, status: 'success', error: '', output: { content: 'conteudo real' } };
    },
  };
  // O modelo chama a tool direta e responde: dois round-trips, sem discover.
  const server = await startCapabilityAgentServer((requestNumber, input) => {
    const output =
      requestNumber === 1
        ? [
            {
              id: 'function-call-read',
              type: 'function_call',
              status: 'completed',
              call_id: 'read-call',
              name: 'filesystem_read',
              arguments: JSON.stringify({ path: 'README.md' }),
            },
          ]
        : [
            {
              id: 'final-read-message',
              type: 'message',
              status: 'completed',
              role: 'assistant',
              content: [{ type: 'output_text', text: 'Arquivo lido.', annotations: [] }],
            },
          ];
    return responseEnvelope(requestNumber, input, output);
  });

  try {
    const result = await runAtlas('Leia o README.md.', {
      apiKey: 'atlas...ey',
      baseURL: server.baseURL,
      conversationId: 'materialized-tools-conversation',
      capabilityRuntime,
    });

    assert.equal(result.finalOutput, 'Arquivo lido.');
    // Chamada direta: nenhum list_tools/describe antes de executar.
    assert.equal(server.requests.length, 2);
    assert.deepEqual(executed, [{ id: 'filesystem.read', arguments_: { path: 'README.md' } }]);

    const firstRequest = server.requests[0] as RequestBody;
    const readTool = tools(firstRequest).find((tool) => tool.name === 'filesystem_read');
    assert.ok(readTool);
    const readParameters = readTool.parameters as RequestBody;
    assert.deepEqual(Object.keys(readParameters.properties as RequestBody).sort(), [
      'limit',
      'offset',
      'path',
    ]);
    assert.deepEqual(readParameters.required, ['path']);
    assert.equal(readParameters.additionalProperties, false);
    assert.equal(
      readTool.description,
      'ler o conteudo de um arquivo — ler o arquivo completo ou por intervalo de linhas',
    );
    // A capability materializada nao aparece no payload como schema do describe.
    assert.doesNotMatch(JSON.stringify(firstRequest.input), /function-call-describe/);
  } finally {
    await server.close();
  }
});

test('discovers Skills without instructions and materializes them on demand', async () => {
  const skill = {
    id: 'release.procedure',
    type: 'skill' as const,
    summary: 'publicar uma versão com validações',
    instructions: '1. Use git.status.\n2. Execute the release checks.\n',
  };
  const capabilityRuntime: CapabilityRuntime = {
    discover: async () => [
      { id: 'shell.exec', type: 'tool', summary: 'executar comandos' },
      { id: skill.id, type: skill.type, summary: skill.summary },
    ],
    listTools: async () => [],
    getDefinition: async () => undefined,
    getSkill: async (id) => (id === skill.id ? skill : undefined),
    execute: async () => ({ target: 'local', status: 'success', error: '' }),
  };
  const server = await startCapabilityAgentServer((requestNumber, input) => {
    const output =
      requestNumber === 1
        ? [
            {
              id: 'function-call-discover-skill',
              type: 'function_call',
              status: 'completed',
              call_id: 'discover-skill-call',
              name: 'discover',
              arguments: JSON.stringify({ query: 'publicar versão' }),
            },
          ]
        : requestNumber === 2
          ? [
              {
                id: 'function-call-skill',
                type: 'function_call',
                status: 'completed',
                call_id: 'skill-call',
                name: 'skill',
                arguments: JSON.stringify({ id: skill.id }),
              },
            ]
          : [
              {
                id: 'final-skill-message',
                type: 'message',
                status: 'completed',
                role: 'assistant',
                content: [{ type: 'output_text', text: 'Instruções carregadas.', annotations: [] }],
              },
            ];
    return responseEnvelope(requestNumber, input, output);
  });

  try {
    const result = await runAtlas('Como publico uma versão?', {
      apiKey: 'atlas-skills-test-key',
      baseURL: server.baseURL,
      conversationId: 'skills-progressive-disclosure-conversation',
      capabilityRuntime,
    });

    assert.equal(result.finalOutput, 'Instruções carregadas.');
    assert.equal(server.requests.length, 3);
    assert.deepEqual(
      tools(server.requests[0] as RequestBody).map((tool) => tool.name),
      baseToolNames,
    );
    assert.doesNotMatch(JSON.stringify(server.requests[1]?.input), /Use git\.status/);
    assert.match(JSON.stringify(server.requests[2]?.input), /Use git\.status/);
    assert.match(JSON.stringify(server.requests[2]?.input), /release\.procedure/);
  } finally {
    await server.close();
  }
});

test('materializes web.search and web.fetch as direct tools', async () => {
  const definitions: ToolDefinition[] = [
    {
      id: 'web.search',
      type: 'tool',
      summary: 'pesquisar na web',
      description: 'consultar resultados atualizados',
      schema: {
        type: 'object',
        properties: { query: { type: 'string' } },
        required: ['query'],
        additionalProperties: false,
      },
    },
    {
      id: 'web.fetch',
      type: 'tool',
      summary: 'ler uma página',
      description: 'obter conteúdo HTTP',
      schema: {
        type: 'object',
        properties: { url: { type: 'string' } },
        required: ['url'],
        additionalProperties: false,
      },
    },
  ];
  const definitionsById = new Map(definitions.map((definition) => [definition.id, definition]));
  const executed: string[] = [];
  const capabilityRuntime: CapabilityRuntime = {
    discover: async () => [],
    listTools: async () => [],
    getDefinition: async (id) => definitionsById.get(id),
    getSkill: async () => undefined,
    execute: async (id, target, arguments_) => {
      executed.push(`${id}:${JSON.stringify(arguments_)}`);
      return { target, status: 'success', error: '', output: { results: [] } };
    },
  };
  const server = await startCapabilityAgentServer((requestNumber, input) => {
    const output =
      requestNumber === 1
        ? [
            {
              id: 'function-call-web-search',
              type: 'function_call',
              status: 'completed',
              call_id: 'web-search-call',
              name: 'web_search',
              arguments: JSON.stringify({ query: 'Atlas' }),
            },
          ]
        : [
            {
              id: 'final-web-search-message',
              type: 'message',
              status: 'completed',
              role: 'assistant',
              content: [{ type: 'output_text', text: 'Busca concluída.', annotations: [] }],
            },
          ];
    return responseEnvelope(requestNumber, input, output);
  });

  try {
    const result = await runAtlas('Pesquise Atlas na web.', {
      apiKey: 'atlas-web-tools-test-key',
      baseURL: server.baseURL,
      conversationId: 'web-tools-conversation',
      capabilityRuntime,
    });

    assert.equal(result.finalOutput, 'Busca concluída.');
    assert.deepEqual(
      tools(server.requests[0] as RequestBody).map((tool) => tool.name),
      ['web_search', 'web_fetch', ...baseToolNames],
    );
    assert.deepEqual(executed, ['web.search:{"query":"Atlas"}']);
    assert.doesNotMatch(JSON.stringify(server.requests[0]?.input), /function-call-discover/);
  } finally {
    await server.close();
  }
});

test('exposes every core capability and keeps the base tools in the same payload', async () => {
  const { NativeCapabilityRuntime } = await import('../../src/capability-runtime.js');
  const capabilityRuntime = new NativeCapabilityRuntime();
  const server = await startCapabilityAgentServer();

  try {
    await runAtlas('Execute node --version.', {
      apiKey: 'atlas...ey',
      baseURL: server.baseURL,
      conversationId: 'materialized-catalog-conversation',
      capabilityRuntime,
    });

    const firstRequest = server.requests[0] as RequestBody;
    assert.deepEqual(
      tools(firstRequest).map((tool) => tool.name),
      [...materializedToolNames, ...baseToolNames],
    );

    // Schemas reais do Registry, sem copia manual em src/atlas.ts.
    const readTool = tools(firstRequest).find((tool) => tool.name === 'filesystem_read');
    assert.ok(readTool);
    const readParameters = readTool.parameters as RequestBody;
    assert.deepEqual(Object.keys(readParameters.properties as RequestBody).sort(), [
      'limit',
      'offset',
      'path',
    ]);
    assert.deepEqual(readParameters.required, ['path']);
    const statusTool = tools(firstRequest).find((tool) => tool.name === 'git_status');
    assert.ok(statusTool);
    assert.deepEqual(
      Object.keys((statusTool.parameters as RequestBody).properties as RequestBody),
      ['path'],
    );
  } finally {
    await capabilityRuntime.close();
    await server.close();
  }
});

test('discovers, describes, and executes shell.exec through base tools', async () => {
  const server = await startCapabilityAgentServer();
  const { NativeCapabilityRuntime } = await import('../../src/capability-runtime.js');
  const capabilityRuntime = new NativeCapabilityRuntime();

  try {
    const result = await runAtlas('Execute node --version.', {
      apiKey: 'atlas-capabilities-test-key',
      baseURL: server.baseURL,
      conversationId: 'capability-conversation',
      capabilityRuntime,
    });

    assert.equal(result.finalOutput, `Executed node --version: ${process.version}`);
    assert.equal(server.requests.length, 4);

    const firstRequest = server.requests[0] as RequestBody;
    assert.deepEqual(
      tools(firstRequest).map((tool) => tool.name),
      [...materializedToolNames, ...baseToolNames],
    );
    const discoverTool = tools(firstRequest).find((tool) => tool.name === 'discover');
    assert.ok(discoverTool);
    const discoverParameters = discoverTool.parameters as RequestBody;
    const listTools = tools(firstRequest).find((tool) => tool.name === 'list_tools');
    assert.ok(listTools);
    const listToolsParameters = listTools.parameters as RequestBody;
    assert.deepEqual(Object.keys(listToolsParameters.properties as RequestBody), ['group']);
    assert.deepEqual(Object.keys(discoverParameters.properties as RequestBody).sort(), [
      'limit',
      'query',
    ]);
    assert.deepEqual(discoverParameters.required, ['query']);
    assert.doesNotMatch(JSON.stringify(discoverTool), /path|group/);

    const secondRequest = server.requests[1] as RequestBody;
    assert.deepEqual(
      tools(secondRequest).map((tool) => tool.name),
      [...materializedToolNames, ...baseToolNames],
    );
    assert.match(JSON.stringify(secondRequest.input), /\\"query\\":\\"executar comando\\"/);
    assert.match(JSON.stringify(secondRequest.input), /shell\.exec/);
    assert.doesNotMatch(JSON.stringify(secondRequest.input), /process - executar/);
    assert.doesNotMatch(JSON.stringify(secondRequest.input), /description/);
    assert.doesNotMatch(JSON.stringify(secondRequest.input), /schema/);
    assert.doesNotMatch(JSON.stringify(tools(firstRequest)), /get_definition|call_tool/);

    const thirdRequest = server.requests[2] as RequestBody;
    assert.deepEqual(
      tools(thirdRequest).map((tool) => tool.name),
      [...materializedToolNames, ...baseToolNames],
    );
    const executeTool = tools(thirdRequest).find((tool) => tool.name === 'execute');
    assert.ok(executeTool);
    const schema = executeTool.parameters as RequestBody;
    const properties = schema.properties as RequestBody;
    assert.ok(properties.id);
    assert.ok(properties.arguments);
    assert.deepEqual(schema.required, ['id', 'arguments']);
    assert.match(JSON.stringify(thirdRequest.input), /\\"id\\":\\"shell\.exec\\"/);
    assert.match(JSON.stringify(thirdRequest.input), /description/);
    assert.match(JSON.stringify(thirdRequest.input), /schema/);

    const fourthRequest = server.requests[3] as RequestBody;
    assert.match(JSON.stringify(fourthRequest.input), /shell-exec-call/);
    assert.match(JSON.stringify(fourthRequest.input), /execute/);
    assert.match(
      JSON.stringify(fourthRequest.input),
      new RegExp(process.version.replaceAll('.', '\\.')),
    );
    assert.deepEqual(
      tools(fourthRequest).map((tool) => tool.name),
      [...materializedToolNames, ...baseToolNames],
    );
  } finally {
    await capabilityRuntime.close();
    await server.close();
  }
});

test('lists the complete tool catalog and filters tools by group', async () => {
  const calls: Array<{ group?: string }> = [];
  const capabilityRuntime: CapabilityRuntime = {
    discover: async () => [],
    listTools: async (request = {}) => {
      calls.push(request);
      return request.group === undefined
        ? [{ id: 'shell.exec', type: 'tool', summary: 'execute a command', group: 'shell' }]
        : [{ id: 'shell.exec', type: 'tool', summary: 'execute a command', group: 'shell' }];
    },
    getDefinition: async () => undefined,
    getSkill: async () => undefined,
    execute: async () => ({ target: 'local', status: 'ok', error: '' }),
  };
  const server = await startCapabilityAgentServer((requestNumber, input) => {
    const output =
      requestNumber === 1 || requestNumber === 2
        ? [
            {
              id: `function-call-list-tools-${requestNumber}`,
              type: 'function_call',
              status: 'completed',
              call_id: `list-tools-call-${requestNumber}`,
              name: 'list_tools',
              arguments: JSON.stringify(requestNumber === 1 ? {} : { group: 'shell' }),
            },
          ]
        : [
            {
              id: 'final-list-tools-message',
              type: 'message',
              status: 'completed',
              role: 'assistant',
              content: [{ type: 'output_text', text: 'Catalog listed.', annotations: [] }],
            },
          ];
    return responseEnvelope(requestNumber, input, output);
  });

  try {
    const result = await runAtlas('Quais tools tem?', {
      apiKey: 'atlas...ey',
      baseURL: server.baseURL,
      conversationId: 'list-tools-conversation',
      capabilityRuntime,
    });

    assert.equal(result.finalOutput, 'Catalog listed.');
    assert.deepEqual(calls, [{}, { group: 'shell' }]);
    assert.match(JSON.stringify(server.requests[1]?.input), /shell\.exec/);
    assert.match(JSON.stringify(server.requests[2]?.input), /shell\.exec/);
    assert.deepEqual(
      tools(server.requests[0] as RequestBody).map((tool) => tool.name),
      baseToolNames,
    );
  } finally {
    await server.close();
  }
});

test('forwards streamed output from the native shell capability', async () => {
  const { NativeCapabilityRuntime } = await import('../../src/capability-runtime.js');
  const runtime = new NativeCapabilityRuntime();
  try {
    const events: Array<{ channel: string; delta: string }> = [];
    const result = await runtime.execute(
      'shell.exec',
      'local',
      {
        command: 'printf out-one; sleep 0.04; printf err-one >&2; sleep 0.04; printf out-two',
      },
      {
        onOutput: (channel, delta) => {
          events.push({ channel, delta });
        },
      },
    );
    assert.deepEqual(
      events.map((event) => event.channel),
      ['stdout', 'stderr', 'stdout'],
    );
    assert.deepEqual(
      events.map((event) => event.delta),
      ['out-one', 'err-one', 'out-two'],
    );
    assert.equal(result.status, 'success');
    assert.equal(result.stdout, 'out-oneout-two');
    assert.equal(result.stderr, 'err-one');
  } finally {
    await runtime.close();
  }
});

test('keeps non-materialized capability schemas out of tools across conversation turns', async () => {
  const definition: ToolDefinition = {
    id: 'sandbox.run',
    type: 'tool',
    summary: 'executa um programa em sandbox',
    description: 'executa um programa local em ambiente isolado',
    schema: {
      type: 'object',
      properties: {
        program: { type: 'string' },
        args: { type: 'array', items: { type: 'string' } },
      },
      required: ['program'],
      additionalProperties: false,
    },
  };
  const executed: Array<{ id: string; arguments_: Record<string, unknown> }> = [];
  const capabilityRuntime: CapabilityRuntime = {
    discover: async () => [
      { id: definition.id, type: definition.type, summary: definition.summary },
    ],
    listTools: async () => [
      { id: definition.id, type: definition.type, summary: definition.summary, group: 'process' },
    ],
    getDefinition: async (id) => (id === definition.id ? definition : undefined),
    getSkill: async () => undefined,
    execute: async (id, target, arguments_) => {
      executed.push({ id, arguments_ });
      return {
        target,
        status: 'success',
        error: '',
        output: { stdout: 'ok', stderr: '', exit_code: 0, duration_ms: 1 },
      };
    },
  };
  const server = await startCapabilityAgentServer((requestNumber, input) => {
    const output =
      requestNumber === 1
        ? [
            {
              id: 'function-call-describe',
              type: 'function_call',
              status: 'completed',
              call_id: 'describe-sandbox-call',
              name: 'describe',
              arguments: JSON.stringify({ id: definition.id }),
            },
          ]
        : requestNumber === 3
          ? [
              {
                id: 'function-call-execute',
                type: 'function_call',
                status: 'completed',
                call_id: 'execute-sandbox-call',
                name: 'execute',
                arguments: JSON.stringify({
                  id: definition.id,
                  arguments: { program: 'python', args: ['--version'] },
                }),
              },
            ]
          : [
              {
                id: `final-message-${requestNumber}`,
                type: 'message',
                status: 'completed',
                role: 'assistant',
                content: [
                  {
                    type: 'output_text',
                    text: requestNumber === 2 ? 'Capability described.' : 'Python tested.',
                    annotations: [],
                  },
                ],
              },
            ];

    return responseEnvelope(requestNumber, input, output);
  });

  try {
    const options = {
      apiKey: 'atlas-capabilities-continuation-test-key',
      baseURL: server.baseURL,
      conversationId: 'capability-continuation-conversation',
      capabilityRuntime,
    };
    const firstResult = await runAtlas('O que sandbox.run faz?', options);
    const secondResult = await runAtlas('Agora testa com python.', options);

    assert.equal(firstResult.finalOutput, 'Capability described.');
    assert.equal(secondResult.finalOutput, 'Python tested.');
    assert.equal(server.requests.length, 4);
    assert.deepEqual(
      tools(server.requests[0] as RequestBody).map((tool) => tool.name),
      baseToolNames,
    );
    assert.deepEqual(
      tools(server.requests[2] as RequestBody).map((tool) => tool.name),
      baseToolNames,
    );
    assert.doesNotMatch(JSON.stringify(tools(server.requests[2] as RequestBody)), /sandbox\.run/);
    assert.deepEqual(executed, [
      { id: 'sandbox.run', arguments_: { program: 'python', args: ['--version'] } },
    ]);
  } finally {
    await server.close();
  }
});

test('dispatches different capability IDs through the generic execute tool', async () => {
  const definitions: ToolDefinition[] = [
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
    discover: async () => definitions.map(({ id, type, summary }) => ({ id, type, summary })),
    listTools: async () =>
      definitions.map(({ id, type, summary }) => ({ id, type, summary, group: 'foo' })),
    getDefinition: async (id) => definitionsById.get(id),
    getSkill: async () => undefined,
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
              arguments: JSON.stringify({ query: 'capabilities de teste' }),
            },
          ]
        : requestNumber === 2 || requestNumber === 4
          ? [
              {
                id: `function-call-describe-${requestNumber}`,
                type: 'function_call',
                status: 'completed',
                call_id: `describe-call-${requestNumber}`,
                name: 'describe',
                arguments: JSON.stringify({ id: definitions[requestNumber === 2 ? 0 : 1]?.id }),
              },
            ]
          : requestNumber === 3 || requestNumber === 5
            ? [
                {
                  id: `function-call-${requestNumber}`,
                  type: 'function_call',
                  status: 'completed',
                  call_id: `capability-call-${requestNumber}`,
                  name: 'execute',
                  arguments: JSON.stringify({
                    id: definitions[requestNumber === 3 ? 0 : 1]?.id,
                    arguments: { value: `value-${requestNumber}` },
                  }),
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
    assert.deepEqual(
      tools(server.requests[0] as RequestBody).map((tool) => tool.name),
      baseToolNames,
    );
    assert.deepEqual(
      tools(server.requests[2] as RequestBody).map((tool) => tool.name),
      baseToolNames,
    );
    assert.deepEqual(
      tools(server.requests[4] as RequestBody).map((tool) => tool.name),
      baseToolNames,
    );
    assert.match(JSON.stringify(server.requests[2]?.input), /description/);
    assert.deepEqual(
      executed.map(({ id }) => id),
      ['foo.bar', 'foo_bar'],
    );
    assert.deepEqual(
      executed.map(({ arguments_ }) => arguments_),
      [{ value: 'value-3' }, { value: 'value-5' }],
    );
    assert.match(JSON.stringify(server.requests[1]?.input), /foo\.bar/);
    assert.doesNotMatch(JSON.stringify(server.requests[2]?.tools), /foo\.bar|foo_bar/);
  } finally {
    await server.close();
  }
});
