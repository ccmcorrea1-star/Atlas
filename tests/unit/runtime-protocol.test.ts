import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  RUNTIME_PROTOCOL,
  RUNTIME_PROTOCOL_VERSION,
  runtimeEvent,
  serializeRuntimeMessage,
  type RuntimeTurnRequest,
} from '../../src/runtime/protocol.js';

const request: RuntimeTurnRequest = {
  protocol: RUNTIME_PROTOCOL,
  version: RUNTIME_PROTOCOL_VERSION,
  type: 'turn.request',
  request_id: 'request-1',
  conversation_id: 'conversation-1',
  input: 'execute node --version',
};

test('serializes the process execution lifecycle without provider tool names', () => {
  const started = runtimeEvent(request, 'execution.started', {
    execution_id: 'call-1',
    capability: 'process.exec',
    program: 'node',
    args: ['--version'],
    target: 'local',
  });
  const completed = runtimeEvent(request, 'execution.completed', {
    execution_id: 'call-1',
    capability: 'process.exec',
    stdout: 'v22.x.x\n',
    stderr: '',
    exit_code: 0,
    duration_ms: 120,
    status: 'success',
  });

  assert.equal(started.type, 'execution.started');
  assert.equal(completed.type, 'execution.completed');
  assert.equal('name' in started.data, false);
  assert.equal('name' in completed.data, false);
  assert.deepEqual(JSON.parse(serializeRuntimeMessage(started)), started);
  assert.deepEqual(JSON.parse(serializeRuntimeMessage(completed)), completed);
});

test('carries Runtime-provided context usage on the completed turn', () => {
  const completed = runtimeEvent(request, 'turn.completed', {
    content: 'done',
    context: {
      used_tokens: 6600,
      context_window: 256000,
    },
  });

  assert.deepEqual(completed.data.context, {
    used_tokens: 6600,
    context_window: 256000,
  });
});
