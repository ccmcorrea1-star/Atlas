import assert from 'node:assert/strict';
import { createServer, type Server } from 'node:http';
import { test } from 'node:test';

import {
  parseRequest,
  runSearch,
  sanitizeResults,
  type SearchProviderFactory,
} from '../../src/capabilities/tools/web/search/search.js';

async function listen(server: Server): Promise<{ endpoint: string; close: () => Promise<void> }> {
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  if (address === null || typeof address === 'string') {
    throw new Error('test server did not expose an address');
  }
  return {
    endpoint: `http://127.0.0.1:${address.port}`,
    close: () =>
      new Promise<void>((resolve, reject) => {
        server.close((error) => (error ? reject(error) : resolve()));
      }),
  };
}

test('parseRequest extrai query e aplica limite padrão', () => {
  assert.deepEqual(parseRequest('{"target":"local","query":"atlas"}'), {
    target: 'local',
    query: 'atlas',
    limit: 5,
  });
});

test('parseRequest limita ao máximo e rejeita entradas inválidas', () => {
  assert.equal(parseRequest('{"target":"local","query":"a","limit":100}').limit, 20);
  assert.throws(() => parseRequest('{"target":"local","query":"  "}'), /non-empty/);
  assert.throws(() => parseRequest('{"target":"local"}'), /query/);
  assert.throws(() => parseRequest('{"target":"local","query":"a","limit":0}'), /limit/);
  assert.throws(() => parseRequest('{invalido}'), /invalid JSON/);
});

test('sanitizeResults mantém só entradas válidas', () => {
  assert.deepEqual(
    sanitizeResults([
      { title: 'T', url: 'https://x.test', snippet: 'S', source: 'w' },
      { title: '', url: 'https://x.test' },
      { title: 'Sem url' },
      'texto',
      42,
    ]),
    [{ title: 'T', url: 'https://x.test', snippet: 'S', source: 'w' }],
  );
  assert.throws(() => sanitizeResults({}), /array/);
});

test('SearXNG é o backend local default e normaliza o envelope', async () => {
  const server = createServer((_request, response) => {
    response.setHeader('content-type', 'application/json');
    response.end(
      JSON.stringify({
        results: [{ title: 'T', url: 'https://x.test', content: 'S', engine: 'local' }],
      }),
    );
  });
  const testServer = await listen(server);
  try {
    const outcome = await runSearch('atlas', 5, { endpoint: testServer.endpoint });
    assert.deepEqual(outcome, {
      status: 'success',
      error: '',
      results: [{ title: 'T', url: 'https://x.test', snippet: 'S', source: 'local' }],
    });
  } finally {
    await testServer.close();
  }
});

test('fallback troca de adapter quando o provider primário falha', async () => {
  const providers = new Map<string, SearchProviderFactory>([
    [
      'local',
      () => ({
        search: async () => {
          throw new Error('local unavailable');
        },
      }),
    ],
    [
      'hosted-test',
      () => ({
        search: async () => [{ title: 'Fallback', url: 'https://fallback.test' }],
      }),
    ],
  ]);

  const outcome = await runSearch(
    'atlas',
    5,
    { provider: 'local', fallbackProviders: ['hosted-test'] },
    providers,
  );
  assert.equal(outcome.status, 'success');
  assert.deepEqual(outcome.results, [{ title: 'Fallback', url: 'https://fallback.test' }]);
});

test('provider desconhecido não é executado nem inventa resultados', async () => {
  const outcome = await runSearch('atlas', 5, { provider: 'missing' }, new Map());
  assert.equal(outcome.status, 'failed');
  assert.match(outcome.error, /not installed/);
  assert.deepEqual(outcome.results, []);
});
