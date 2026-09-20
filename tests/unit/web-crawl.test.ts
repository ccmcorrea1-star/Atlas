import assert from 'node:assert/strict';
import { createServer, type Server } from 'node:http';
import { test } from 'node:test';

import { parseRequest, runCrawl } from '../../src/capabilities/tools/web/crawl.js';

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

test('web.crawl valida a entrada de crawling', () => {
  assert.deepEqual(parseRequest('{"target":"local","url":"https://example.test","maxPages":3}'), {
    target: 'local',
    request: { url: 'https://example.test', maxPages: 3 },
  });
  assert.throws(() => parseRequest('{"target":"local","url":"file:///tmp/x"}'), /HTTP/);
  assert.throws(
    () => parseRequest('{"target":"local","url":"https://x","maxDepth":0}'),
    /positive/,
  );
});

test('web.crawl usa somente o adapter local configurado', async () => {
  const server = createServer(async (request, response) => {
    assert.equal(request.method, 'POST');
    response.setHeader('content-type', 'application/json');
    response.end(JSON.stringify([{ url: 'https://example.test' }]));
  });
  const testServer = await listen(server);
  try {
    const result = await runCrawl(
      { url: 'https://example.test', maxPages: 2 },
      { provider: 'crawl4ai', endpoint: testServer.endpoint },
    );
    assert.deepEqual(result, { status: 'success', pages: [{ url: 'https://example.test' }] });
  } finally {
    await testServer.close();
  }
});

test('web.crawl fica unavailable sem endpoint', async () => {
  assert.deepEqual(await runCrawl({ url: 'https://example.test' }, { provider: 'crawl4ai' }), {
    status: 'unavailable',
    error: 'Crawl4AI endpoint is not configured',
  });
});
