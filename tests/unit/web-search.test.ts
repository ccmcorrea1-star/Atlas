import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer, type Server } from 'node:http';
import { test } from 'node:test';

import {
  parseRequest,
  runSearch,
  sanitizeResults,
  type SearchProviderFactory,
} from '../../src/capabilities/tools/web/search/search.js';

const capability = JSON.parse(
  readFileSync(
    new URL('../../src/capabilities/tools/web/search/capability.json', import.meta.url),
    'utf8',
  ),
) as {
  description: string;
  schema: { properties: Record<string, { description?: string }> };
};

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
  assert.throws(() => parseRequest('{"target":"local","query":"a","category":"blog"}'), /category/);
  assert.throws(
    () => parseRequest('{"target":"local","query":"a","timeRange":"year"}'),
    /timeRange/,
  );
  assert.throws(() => parseRequest('{"target":"local","query":"a","domains":[""]}'), /domains/);
  assert.throws(() => parseRequest('{invalido}'), /invalid JSON/);
});

test('parseRequest mantém compatibilidade e aceita filtros opcionais', () => {
  assert.deepEqual(
    parseRequest(
      JSON.stringify({
        target: 'local',
        query: ' noticias tec ',
        limit: 3,
        category: 'news',
        timeRange: 'week',
        domains: ['example.com', ' example.org '],
      }),
    ),
    {
      target: 'local',
      query: 'noticias tec',
      limit: 3,
      category: 'news',
      timeRange: 'week',
      domains: ['example.com', 'example.org'],
    },
  );
});

test('contrato orienta buscas atuais e validação seletiva de fontes', () => {
  assert.match(capability.description, /category=news/);
  assert.match(capability.description, /timeRange=day/);
  assert.match(capability.description, /limit=8/);
  assert.match(capability.description, /2 ou 3 URLs/);
  assert.match(capability.description, /web\.fetch/);
  assert.match(capability.schema.properties.category?.description ?? '', /news/);
  assert.match(capability.schema.properties.timeRange?.description ?? '', /day/);
});

test('sanitizeResults mantém só entradas válidas', () => {
  assert.deepEqual(
    sanitizeResults([
      { title: 'T', url: 'https://x.test', snippet: 'S', source: 'w' },
      { title: 'Duplicado', url: 'https://x.test' },
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
  let requestedUrl: URL | undefined;
  const server = createServer((request, response) => {
    requestedUrl = new URL(request.url ?? '/', 'http://test.local');
    response.setHeader('content-type', 'application/json');
    response.end(
      JSON.stringify({
        results: [
          {
            title: 'T',
            url: 'https://x.test',
            content: 'S',
            engine: 'local',
            publishedDate: '2026-09-20T12:00:00Z',
            score: 0.75,
          },
        ],
      }),
    );
  });
  const testServer = await listen(server);
  try {
    const outcome = await runSearch('atlas', 5, { endpoint: testServer.endpoint });
    assert.deepEqual(outcome, {
      status: 'success',
      error: '',
      results: [
        {
          title: 'T',
          url: 'https://x.test',
          snippet: 'S',
          source: 'local',
          publishedAt: '2026-09-20T12:00:00Z',
          score: 0.75,
        },
      ],
    });
    assert.equal(requestedUrl?.searchParams.get('q'), 'atlas');
    assert.equal(requestedUrl?.searchParams.get('categories'), 'general');
    assert.equal(requestedUrl?.searchParams.get('format'), 'json');
    assert.equal(requestedUrl?.searchParams.has('time_range'), false);
  } finally {
    await testServer.close();
  }
});

test('SearXNG expõe engines indisponíveis em resultados parciais', async () => {
  const server = createServer((_request, response) => {
    response.setHeader('content-type', 'application/json');
    response.end(
      JSON.stringify({
        results: [{ title: 'T', url: 'https://x.test' }],
        unresponsive_engines: [
          ['brave', 'Suspended: too many requests'],
          ['duckduckgo', 'CAPTCHA'],
        ],
      }),
    );
  });
  const testServer = await listen(server);
  try {
    const outcome = await runSearch('atlas', 5, { endpoint: testServer.endpoint });
    assert.deepEqual(outcome, {
      status: 'success',
      error: '',
      results: [{ title: 'T', url: 'https://x.test' }],
      warnings: ['brave: Suspended: too many requests', 'duckduckgo: CAPTCHA'],
    });
  } finally {
    await testServer.close();
  }
});

test('SearXNG falha explicitamente quando engines indisponíveis deixam a busca vazia', async () => {
  const server = createServer((_request, response) => {
    response.setHeader('content-type', 'application/json');
    response.end(
      JSON.stringify({
        results: [],
        unresponsive_engines: [['brave', 'Suspended: too many requests']],
      }),
    );
  });
  const testServer = await listen(server);
  try {
    const outcome = await runSearch('atlas', 5, { endpoint: testServer.endpoint });
    assert.deepEqual(outcome, {
      status: 'failed',
      error:
        'SearXNG returned no results; unavailable engines: brave: Suspended: too many requests',
      results: [],
      warnings: ['brave: Suspended: too many requests'],
    });
  } finally {
    await testServer.close();
  }
});

test('SearXNG mapeia noticias, freshness e dominios', async () => {
  let requestedUrl: URL | undefined;
  const server = createServer((request, response) => {
    requestedUrl = new URL(request.url ?? '/', 'http://test.local');
    response.setHeader('content-type', 'application/json');
    response.end(JSON.stringify({ results: [] }));
  });
  const testServer = await listen(server);
  try {
    const outcome = await runSearch(
      'noticias tec',
      5,
      { endpoint: testServer.endpoint },
      undefined,
      { category: 'news', timeRange: 'day', domains: ['example.com', 'example.org'] },
    );

    assert.deepEqual(outcome.results, []);
    assert.equal(requestedUrl?.searchParams.get('categories'), 'news');
    assert.equal(requestedUrl?.searchParams.get('time_range'), 'day');
    assert.equal(
      requestedUrl?.searchParams.get('q'),
      'noticias tec (site:example.com OR site:example.org)',
    );
  } finally {
    await testServer.close();
  }
});

test('SearXNG omite metadados opcionais ausentes ou inválidos', async () => {
  const server = createServer((_request, response) => {
    response.setHeader('content-type', 'application/json');
    response.end(
      JSON.stringify({
        results: [
          { title: 'Sem metadados', url: 'https://missing.test' },
          {
            title: 'Metadados inválidos',
            url: 'https://invalid.test',
            publishedDate: null,
            score: 'not-a-number',
          },
        ],
      }),
    );
  });
  const testServer = await listen(server);
  try {
    const outcome = await runSearch('atlas', 5, { endpoint: testServer.endpoint });
    assert.deepEqual(outcome.results, [
      { title: 'Sem metadados', url: 'https://missing.test' },
      { title: 'Metadados inválidos', url: 'https://invalid.test' },
    ]);
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
