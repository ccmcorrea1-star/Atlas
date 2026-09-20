import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  RUNTIME_PROTOCOL,
  RUNTIME_PROTOCOL_VERSION,
  parseRuntimeMessage,
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

test('parses a turn cancellation with the original request identity', () => {
  const cancel = parseRuntimeMessage(
    JSON.stringify({
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'turn.cancel',
      request_id: 'request-1',
      conversation_id: 'conversation-1',
    }),
  );

  assert.deepEqual(cancel, {
    protocol: RUNTIME_PROTOCOL,
    version: RUNTIME_PROTOCOL_VERSION,
    type: 'turn.cancel',
    request_id: 'request-1',
    conversation_id: 'conversation-1',
  });
});

test('serializes the shell execution lifecycle without provider tool names', () => {
  const started = runtimeEvent(request, 'execution.started', {
    execution_id: 'call-1',
    capability: 'shell.exec',
    program: 'sh',
    args: ['-c', 'node --version'],
    target: 'local',
  });
  const completed = runtimeEvent(request, 'execution.completed', {
    execution_id: 'call-1',
    capability: 'shell.exec',
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

test('serializes cancellation as a terminal turn event', () => {
  const cancelled = runtimeEvent(request, 'turn.cancelled', {
    content: 'partial response',
    message_id: 'message-1',
  });

  assert.deepEqual(JSON.parse(serializeRuntimeMessage(cancelled)), cancelled);
  assert.equal(cancelled.type, 'turn.cancelled');
});

test('serializes context updates as public events', () => {
  const updated = runtimeEvent(request, 'context.updated', {
    used_tokens: 6600,
    context_window: 256000,
  });

  assert.equal(updated.type, 'context.updated');
  assert.deepEqual(JSON.parse(serializeRuntimeMessage(updated)), updated);
});

test('serializes session metadata as a public event', () => {
  const updated = runtimeEvent(request, 'session.updated', {
    model: 'gpt-5.6-luna',
    provider: 'opencode-go',
  });

  assert.equal(updated.type, 'session.updated');
  assert.deepEqual(JSON.parse(serializeRuntimeMessage(updated)), updated);
});
