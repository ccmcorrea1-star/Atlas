import assert from 'node:assert/strict';
import { createServer } from 'node:http';

import { Atlas, createAtlasRunner } from './index.js';

type CapturedRequest = {
  body: Record<string, unknown>;
  headers: Record<string, string | string[] | undefined>;
  url: string | undefined;
};

const capturedRequests: CapturedRequest[] = [];
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
  response.end(
    JSON.stringify({
      id: 'resp_atlas_smoke',
      object: 'response',
      created_at: 1,
      status: 'completed',
      model: 'gpt-5.6-luna',
      output: [
        {
          id: 'msg_atlas_smoke',
          type: 'message',
          status: 'completed',
          role: 'assistant',
          content: [{ type: 'output_text', text: 'Atlas smoke OK', annotations: [] }],
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
  const runner = createAtlasRunner({
    apiKey: 'atlas-smoke-key',
    baseURL: `http://127.0.0.1:${port}/zen/go/v1`,
    sessionId: 'atlas-smoke-session',
  });
  const result = await runner.run(Atlas, 'Reply with exactly: Atlas smoke OK.');
  const secondResult = await runner.run(Atlas, 'Reply with exactly: Atlas smoke OK.');

  assert.equal(result.finalOutput, 'Atlas smoke OK');
  assert.equal(secondResult.finalOutput, 'Atlas smoke OK');
  assert.equal(capturedRequests.length, 2);
  assert.equal(capturedRequests[0]?.url, '/zen/go/v1/responses');
  assert.equal(capturedRequests[0]?.headers['user-agent'], 'Atlas/1.0.0');
  assert.equal(capturedRequests[0]?.headers['x-opencode-session'], 'atlas-smoke-session');
  assert.equal(capturedRequests[0]?.headers.authorization, 'Bearer atlas-smoke-key');
  assert.equal(capturedRequests[0]?.body.model, 'gpt-5.6-luna');
  assert.equal(capturedRequests[1]?.headers['x-opencode-session'], 'atlas-smoke-session');

  console.log('Atlas smoke passed: Responses endpoint and session headers are configured.');
} finally {
  await close();
}
