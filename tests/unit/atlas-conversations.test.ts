import assert from 'node:assert/strict';
import { test } from 'node:test';

import { getAtlasRunner, runAtlas } from '../../src/index.js';
import { startOpenCodeGoTestServer } from '../support/open-code-go-test-server.js';

test('keeps conversation identity and history while reusing the Runner', async () => {
  const server = await startOpenCodeGoTestServer([
    'First turn stored.',
    'Second turn saw the context.',
    'Independent conversation.',
  ]);

  try {
    const providerOptions = {
      apiKey: 'atlas-test-key',
      baseURL: server.baseURL,
    };
    const runner = getAtlasRunner(providerOptions);

    const firstResult = await runAtlas('Remember this context: the first Atlas turn.', {
      ...providerOptions,
      conversationId: 'conversation-a',
    });
    const secondResult = await runAtlas('Use the context from my previous turn.', {
      ...providerOptions,
      conversationId: 'conversation-a',
    });
    const independentResult = await runAtlas('This is a separate conversation.', {
      ...providerOptions,
      conversationId: 'conversation-b',
    });

    assert.equal(firstResult.finalOutput, 'First turn stored.');
    assert.equal(secondResult.finalOutput, 'Second turn saw the context.');
    assert.equal(independentResult.finalOutput, 'Independent conversation.');
    assert.equal(server.requests.length, 3);
    assert.equal(server.requests[0]?.headers['x-opencode-session'], 'conversation-a');
    assert.equal(server.requests[1]?.headers['x-opencode-session'], 'conversation-a');
    assert.notEqual(
      server.requests[2]?.headers['x-opencode-session'],
      server.requests[0]?.headers['x-opencode-session'],
    );
    assert.match(JSON.stringify(server.requests[1]?.body.input), /the first Atlas turn/);
    assert.doesNotMatch(JSON.stringify(server.requests[2]?.body.input), /the first Atlas turn/);
    assert.strictEqual(getAtlasRunner(providerOptions), runner);
    assert.strictEqual(
      getAtlasRunner(providerOptions).config.modelProvider,
      runner.config.modelProvider,
    );
  } finally {
    await server.close();
  }
});
