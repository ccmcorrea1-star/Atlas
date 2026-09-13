import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { test } from 'node:test';

import { runAtlas } from '../../src/index.js';

type RequestBody = Record<string, unknown>;

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
              name: 'process.exec',
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

async function startCapabilityAgentServer(): Promise<{
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
    response.end(JSON.stringify(responseBody(requests.length, body)));
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
    const materializedTool = tools(secondRequest).find((tool) => tool.name === 'process.exec');
    assert.ok(materializedTool);
    assert.equal(materializedTool.description, 'executar um programa local diretamente, sem shell');
    const schema = materializedTool.parameters as RequestBody;
    const properties = schema.properties as RequestBody;
    assert.ok(properties.program);
    assert.ok(properties.args);
    assert.deepEqual(schema.required, ['program']);
    assert.match(JSON.stringify(secondRequest.input), /\\"path\\":\\"process\\"/);
    assert.doesNotMatch(JSON.stringify(firstTools), /get_definition|call_tool/);

    const thirdRequest = server.requests[2] as RequestBody;
    assert.match(JSON.stringify(thirdRequest.input), /process-exec-call/);
    assert.match(
      JSON.stringify(thirdRequest.input),
      new RegExp(process.version.replaceAll('.', '\\.')),
    );
  } finally {
    await server.close();
  }
});
