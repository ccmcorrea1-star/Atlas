import assert from 'node:assert/strict';
import test from 'node:test';

import { deduplicateOpenCodeRequestBody } from '../../src/providers/opencode-go.js';

test('removes duplicate input item IDs before sending to OpenCode Go', () => {
  const body = JSON.stringify({
    model: 'gpt-5.6-luna',
    input: [
      { type: 'reasoning', id: 'rs_duplicate', summary: [] },
      { type: 'function_call', id: 'fc_unique', name: 'shell.exec' },
      { type: 'reasoning', id: 'rs_duplicate', summary: [] },
      { type: 'function_call_output', call_id: 'fc_unique', output: 'ok' },
    ],
  });

  const result = JSON.parse(String(deduplicateOpenCodeRequestBody(body))) as {
    input: Array<{ id?: string; call_id?: string }>;
  };

  assert.deepEqual(result.input, [
    { type: 'reasoning', id: 'rs_duplicate', summary: [] },
    { type: 'function_call', id: 'fc_unique', name: 'shell.exec' },
    { type: 'function_call_output', call_id: 'fc_unique', output: 'ok' },
  ]);
});

test('does not rewrite requests without duplicate IDs', () => {
  const body = JSON.stringify({ input: [{ type: 'message', id: 'msg_unique' }] });
  assert.equal(deduplicateOpenCodeRequestBody(body), body);
});

test('leaves non-JSON and non-string bodies untouched', () => {
  const invalid = '{not-json';
  assert.equal(deduplicateOpenCodeRequestBody(invalid), invalid);
  assert.equal(deduplicateOpenCodeRequestBody(null), null);
});
