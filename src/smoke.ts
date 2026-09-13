import assert from 'node:assert/strict';
import { createServer } from 'node:http';

import { getAtlasRunner, runAtlas } from './index.js';

type CapturedRequest = {
  body: Record<string, unknown>;
  headers: Record<string, string | string[] | undefined>;
  url: string | undefined;
};

const capturedRequests: CapturedRequest[] = [];
// O servidor local valida o contrato sem depender da API do OpenCode Go.
const server = createServer(async (request, response) => {
  const chunks: Buffer[] = [];
  for await (const chunk of request) {
    chunks.push(Buffer.from(chunk));
  }

  capturedRequests.push({
    body: JSON.parse(Buffer.concat(chunks).toString('utf8')) as Record<string, unknown>,
    headers: request.headers,
    url: request.url,
  });

  response.writeHead(200, { 'content-type': 'application/json' });
  const requestNumber = capturedRequests.length;
  const outputText =
    requestNumber === 1
      ? 'First turn stored.'
      : requestNumber === 2
        ? 'Second turn saw the context.'
        : 'Independent conversation.';

  response.end(
    JSON.stringify({
      id: `resp_atlas_smoke_${requestNumber}`,
      object: 'response',
      created_at: 1,
      status: 'completed',
      model: 'gpt-5.6-luna',
      output: [
        {
          id: `msg_atlas_smoke_${requestNumber}`,
          type: 'message',
          status: 'completed',
          role: 'assistant',
          content: [{ type: 'output_text', text: outputText, annotations: [] }],
        },
      ],
      usage: {
        input_tokens: 1,
        output_tokens: 3,
        total_tokens: 4,
      },
    }),
  );
});

function listen(): Promise<number> {
  return new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      if (!address || typeof address === 'string') {
        reject(new Error('Smoke server did not receive a TCP address.'));
        return;
      }

      resolve(address.port);
    });
  });
}

function close(): Promise<void> {
  return new Promise((resolve, reject) => {
    server.close((error) => (error ? reject(error) : resolve()));
  });
}

const port = await listen();

try {
  // Duas conversas comprovam continuidade e isolamento do histórico.
  const providerOptions = {
    apiKey: 'atlas-smoke-key',
    baseURL: `http://127.0.0.1:${port}/zen/go/v1`,
  };
  const runner = getAtlasRunner(providerOptions);
  const firstResult = await runAtlas('Remember this context: Atlas smoke first turn.', {
    ...providerOptions,
    conversationId: 'atlas-smoke-conversation-a',
  });
  const secondResult = await runAtlas('Use the context from my previous turn.', {
    ...providerOptions,
    conversationId: 'atlas-smoke-conversation-a',
  });
  const independentResult = await runAtlas('This is a separate conversation.', {
    ...providerOptions,
    conversationId: 'atlas-smoke-conversation-b',
  });

  assert.equal(firstResult.finalOutput, 'First turn stored.');
  assert.equal(secondResult.finalOutput, 'Second turn saw the context.');
  assert.equal(independentResult.finalOutput, 'Independent conversation.');
  assert.equal(capturedRequests.length, 3);
  assert.equal(capturedRequests[0]?.url, '/zen/go/v1/responses');
  assert.equal(capturedRequests[0]?.headers['user-agent'], 'Atlas/1.0.0');
  assert.equal(capturedRequests[0]?.headers['x-opencode-session'], 'atlas-smoke-conversation-a');
  assert.equal(capturedRequests[0]?.headers.authorization, 'Bearer atlas-smoke-key');
  assert.equal(capturedRequests[0]?.body.model, 'gpt-5.6-luna');
  assert.equal(capturedRequests[1]?.headers['x-opencode-session'], 'atlas-smoke-conversation-a');
  assert.notEqual(
    capturedRequests[2]?.headers['x-opencode-session'],
    capturedRequests[0]?.headers['x-opencode-session'],
  );
  assert.match(JSON.stringify(capturedRequests[1]?.body.input), /Atlas smoke first turn/);
  assert.doesNotMatch(JSON.stringify(capturedRequests[2]?.body.input), /Atlas smoke first turn/);
  assert.equal(getAtlasRunner(providerOptions), runner);
  assert.equal(getAtlasRunner(providerOptions).config.modelProvider, runner.config.modelProvider);

  console.log('Atlas smoke passed: conversation sessions preserve context and reuse the Runner.');
} finally {
  await close();
}
