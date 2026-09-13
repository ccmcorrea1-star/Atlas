import assert from 'node:assert/strict';
import { test } from 'node:test';

import { Atlas, createAtlasRunner, OpenCodeGoSession } from '../../src/index.js';
import { startOpenCodeGoTestServer } from '../support/open-code-go-test-server.js';

test('uses the configured session for direct Runner requests', async () => {
  const server = await startOpenCodeGoTestServer(['Direct runner response.']);

  try {
    const runner = createAtlasRunner({
      apiKey: 'atlas-provider-test-key',
      baseURL: server.baseURL,
      sessionId: 'direct-runner-session',
    });

    const result = await runner.run(Atlas, 'Reply with the direct response.');

    assert.equal(result.finalOutput, 'Direct runner response.');
    assert.equal(server.requests[0]?.headers['x-opencode-session'], 'direct-runner-session');
    assert.equal(server.requests[0]?.headers['user-agent'], 'Atlas/1.0.0');
  } finally {
    await server.close();
  }
});

test('rejects empty OpenCode Go session IDs', () => {
  assert.throws(() => new OpenCodeGoSession(' '), /OpenCode Go session ID cannot be empty/);
});
