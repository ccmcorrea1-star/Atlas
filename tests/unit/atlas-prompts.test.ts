import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { test } from 'node:test';

import { loadInstructions, runAtlas } from '../../src/atlas.js';

type RequestBody = Record<string, unknown>;

// O provedor envia as instrucoes no input como mensagem de developer ou system.
function agentInstructions(body: RequestBody): string | undefined {
  if (!Array.isArray(body.input)) {
    return undefined;
  }

  const message = body.input
    .filter((item): item is RequestBody => item !== null && typeof item === 'object')
    .find((item) => item.role === 'developer' || item.role === 'system');
  return typeof message?.content === 'string' ? message.content : undefined;
}

function responseBody(): RequestBody {
  return {
    id: 'prompt-response',
    object: 'response',
    created_at: 1,
    status: 'completed',
    model: 'gpt-5.6-luna',
    output: [
      {
        id: 'prompt-message',
        type: 'message',
        status: 'completed',
        role: 'assistant',
        content: [{ type: 'output_text', text: 'ok', annotations: [] }],
      },
    ],
    usage: { input_tokens: 1, output_tokens: 1, total_tokens: 2 },
  };
}

async function startServer(): Promise<{
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
    requests.push(JSON.parse(Buffer.concat(chunks).toString('utf8')) as RequestBody);

    response.writeHead(200, { 'content-type': 'application/json' });
    response.end(JSON.stringify(responseBody()));
  });

  const port = await new Promise<number>((resolvePort, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      if (!address || typeof address === 'string') {
        reject(new Error('Prompt test server did not receive a TCP address.'));
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

test('loads the default prompt and caches it', () => {
  const instructions = loadInstructions();

  assert.ok(instructions.length > 0);
  assert.equal(loadInstructions(), instructions);
});

test('sends the default prompt as the agent instructions', async () => {
  const server = await startServer();

  try {
    const baseOptions = {
      apiKey: 'atlas-prompts-test-key',
      baseURL: server.baseURL,
      coreTools: false,
    };

    await runAtlas('Oi.', {
      ...baseOptions,
      model: 'gpt-5.6-luna',
      conversationId: 'prompt-gpt-conversation',
    });
    assert.equal(agentInstructions(server.requests[0] ?? {}), loadInstructions());

    // Qualquer modelo usa o mesmo prompt padrao.
    await runAtlas('Oi.', {
      ...baseOptions,
      model: 'claude-3-5-sonnet',
      models: { 'claude-3-5-sonnet': { endpoint: 'responses' } },
      conversationId: 'prompt-default-conversation',
    });
    assert.equal(agentInstructions(server.requests[1] ?? {}), loadInstructions());
  } finally {
    await server.close();
  }
});
