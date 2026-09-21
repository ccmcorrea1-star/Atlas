import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  RUNTIME_COMMANDS,
  RUNTIME_PROTOCOL,
  RUNTIME_PROTOCOL_VERSION,
  parseRuntimeMessage,
  runtimeEvent,
  runtimeErrorEvent,
  runtimeLifecycleEvent,
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

test('parses a turn discard with the original request identity', () => {
  assert.deepEqual(
    parseRuntimeMessage(
      JSON.stringify({
        protocol: RUNTIME_PROTOCOL,
        version: RUNTIME_PROTOCOL_VERSION,
        type: 'turn.discard',
        request_id: 'request-1',
        conversation_id: 'conversation-1',
      }),
    ),
    {
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'turn.discard',
      request_id: 'request-1',
      conversation_id: 'conversation-1',
    },
  );
});

test('serializes recoverable ambiguous execution errors with a typed code', () => {
  assert.deepEqual(runtimeErrorEvent('interrupted', request, 'ambiguous_execution'), {
    protocol: RUNTIME_PROTOCOL,
    version: RUNTIME_PROTOCOL_VERSION,
    type: 'error',
    request_id: 'request-1',
    conversation_id: 'conversation-1',
    data: { code: 'ambiguous_execution', message: 'interrupted' },
  });
});
test('serializes a restart-interrupted session snapshot without replay semantics', () => {
  const event = runtimeEvent(request, 'command.completed', {
    command: 'status',
    message: 'Sessão interrompida durante restart.',
    session: {
      id: 'conversation-1',
      model: 'gpt-5.6-luna',
      provider: 'opencode-go',
      status: 'restart_interrupted',
      interrupted_request_id: 'interrupted-request',
    },
  });

  assert.equal((event.data.session as { status: string }).status, 'restart_interrupted');
  assert.equal(
    (event.data.session as { interrupted_request_id: string }).interrupted_request_id,
    'interrupted-request',
  );
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

test('serializes restart and operation resumption as public lifecycle events', () => {
  const events = [
    runtimeLifecycleEvent('runtime.restarting', request.conversation_id, { reason: 'handoff' }),
    runtimeLifecycleEvent('runtime.ready', request.conversation_id),
    runtimeLifecycleEvent('operation.resuming', request.conversation_id, {
      operation_id: 'operation-1',
    }),
    runtimeLifecycleEvent('operation.resumed', request.conversation_id, {
      operation_id: 'operation-1',
    }),
  ];

  assert.deepEqual(
    events.map((event) => event.type),
    ['runtime.restarting', 'runtime.ready', 'operation.resuming', 'operation.resumed'],
  );
  assert.equal('reason' in events[0].data ? events[0].data.reason : undefined, 'handoff');
  assert.equal('base_commit' in events[0].data, false);
  assert.deepEqual(JSON.parse(serializeRuntimeMessage(events[3])), events[3]);
});

test('serializes the reasoning lifecycle with a stable reasoning id', () => {
  const events = [
    runtimeEvent(request, 'reasoning-start', { reasoning_id: 'reasoning-1' }),
    runtimeEvent(request, 'reasoning-delta', {
      reasoning_id: 'reasoning-1',
      delta: '**Inspect the error path**',
    }),
    runtimeEvent(request, 'reasoning-end', { reasoning_id: 'reasoning-1' }),
  ];

  assert.deepEqual(
    events.map((event) => JSON.parse(serializeRuntimeMessage(event))),
    events,
  );
  assert.deepEqual(
    events.map((event) => event.type),
    ['reasoning-start', 'reasoning-delta', 'reasoning-end'],
  );
});

test('parses typed attachments without encoding them in input text', () => {
  const parsed = parseRuntimeMessage(
    JSON.stringify({
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'turn.request',
      request_id: 'request-1',
      conversation_id: 'conversation-1',
      input: 'Analise o arquivo.',
      attachments: [
        {
          type: 'document',
          uri: 'file:///tmp/report.pdf',
          media_type: 'application/pdf',
          file_name: 'report.pdf',
          size_bytes: 12,
          source: { platform: 'telegram', file_id: 'file-1' },
        },
      ],
    }),
  );

  assert.equal(parsed.type, 'turn.request');
  assert.deepEqual(parsed.attachments?.[0], {
    type: 'document',
    uri: 'file:///tmp/report.pdf',
    media_type: 'application/pdf',
    file_name: 'report.pdf',
    size_bytes: 12,
    source: { platform: 'telegram', file_id: 'file-1' },
  });
  assert.equal(parsed.input, 'Analise o arquivo.');
});

test('parses typed reply context without altering the user input', () => {
  const parsed = parseRuntimeMessage(
    JSON.stringify({
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'turn.request',
      request_id: 'request-2',
      conversation_id: 'conversation-1',
      input: 'responda a isso',
      context: {
        reply_to: {
          source: 'telegram',
          message_id: '42',
          author: 'Caio',
          text: 'mensagem anterior',
          media: ['photo'],
        },
      },
    }),
  );

  assert.equal(parsed.type, 'turn.request');
  if (parsed.type !== 'turn.request') {
    return;
  }
  assert.deepEqual(parsed.context, {
    reply_to: {
      source: 'telegram',
      message_id: '42',
      author: 'Caio',
      text: 'mensagem anterior',
      media: ['photo'],
    },
  });
  assert.equal(parsed.input, 'responda a isso');
});

test('parses locations and venues as structured attachments', () => {
  const parsed = parseRuntimeMessage(
    JSON.stringify({
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'turn.request',
      request_id: 'request-location',
      conversation_id: 'conversation-1',
      input: 'onde fica?',
      attachments: [
        {
          type: 'location',
          uri: 'geo:-23.55,-46.63',
          media_type: 'application/vnd.atlas.location',
          description: 'latitude -23.55, longitude -46.63',
        },
        {
          type: 'venue',
          uri: 'geo:-23.56,-46.64',
          media_type: 'application/vnd.atlas.venue',
          description: 'Praça — Rua A',
        },
      ],
    }),
  );

  assert.equal(parsed.type, 'turn.request');
  if (parsed.type !== 'turn.request') {
    return;
  }
  assert.deepEqual(
    parsed.attachments?.map((attachment) => attachment.type),
    ['location', 'venue'],
  );
  assert.equal(parsed.attachments?.[1]?.description, 'Praça — Rua A');
});

test('parses reaction requests as a separate Runtime family', () => {
  const parsed = parseRuntimeMessage(
    JSON.stringify({
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'reaction.request',
      request_id: 'reaction-1',
      conversation_id: 'telegram:123:thread:root',
      message_id: '51',
      action: 'added',
      reactions: ['👍'],
      source: 'telegram',
      actor_id: '7',
    }),
  );

  assert.deepEqual(parsed, {
    protocol: RUNTIME_PROTOCOL,
    version: RUNTIME_PROTOCOL_VERSION,
    type: 'reaction.request',
    request_id: 'reaction-1',
    conversation_id: 'telegram:123:thread:root',
    message_id: '51',
    action: 'added',
    reactions: ['👍'],
    source: 'telegram',
    actor_id: '7',
  });
});

test('parses inline requests while preserving empty query and offset', () => {
  const parsed = parseRuntimeMessage(
    JSON.stringify({
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'inline.request',
      request_id: 'inline-1',
      conversation_id: 'telegram:inline:7',
      query_id: 'telegram-query-1',
      user_id: '7',
      query: '',
      offset: '',
      chat_type: 'sender',
    }),
  );

  assert.deepEqual(parsed, {
    protocol: RUNTIME_PROTOCOL,
    version: RUNTIME_PROTOCOL_VERSION,
    type: 'inline.request',
    request_id: 'inline-1',
    conversation_id: 'telegram:inline:7',
    query_id: 'telegram-query-1',
    user_id: '7',
    query: '',
    offset: '',
    chat_type: 'sender',
  });
});

test('parses topic service events as a separate Runtime family', () => {
  const parsed = parseRuntimeMessage(
    JSON.stringify({
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'topic.request',
      request_id: 'topic-1',
      conversation_id: 'telegram:123:thread:88',
      topic_id: '88',
      message_id: '60',
      action: 'created',
      source: 'telegram',
      name: 'Filmes',
      icon_color: 123456,
    }),
  );

  assert.deepEqual(parsed, {
    protocol: RUNTIME_PROTOCOL,
    version: RUNTIME_PROTOCOL_VERSION,
    type: 'topic.request',
    request_id: 'topic-1',
    conversation_id: 'telegram:123:thread:88',
    topic_id: '88',
    message_id: '60',
    action: 'created',
    source: 'telegram',
    name: 'Filmes',
    icon_color: 123456,
  });
});

test('parses notification subscriptions and publications', () => {
  const subscription = parseRuntimeMessage(
    JSON.stringify({
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'notification.subscribe',
      request_id: 'notification-subscribe',
      conversation_id: '*',
    }),
  );
  const publication = parseRuntimeMessage(
    JSON.stringify({
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'notification.publish',
      request_id: 'notification-publish',
      notification_id: 'notification-1',
      conversation_id: 'telegram:123:thread:root',
      content: 'Download concluído.',
      source: 'torrent',
      title: 'Atlas',
      level: 'success',
    }),
  );

  assert.equal(subscription.type, 'notification.subscribe');
  assert.equal(subscription.conversation_id, '*');
  assert.deepEqual(publication, {
    protocol: RUNTIME_PROTOCOL,
    version: RUNTIME_PROTOCOL_VERSION,
    type: 'notification.publish',
    request_id: 'notification-publish',
    notification_id: 'notification-1',
    conversation_id: 'telegram:123:thread:root',
    content: 'Download concluído.',
    source: 'torrent',
    title: 'Atlas',
    level: 'success',
  });
});

test('accepts only commands exposed by the Runtime catalog', () => {
  assert.deepEqual(
    RUNTIME_COMMANDS.map((command) => command.name),
    ['new', 'status', 'stop'],
  );
  const parsed = parseRuntimeMessage(
    JSON.stringify({
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'command.request',
      request_id: 'command-1',
      conversation_id: 'conversation-1',
      command: 'stop',
    }),
  );
  assert.equal(parsed.type, 'command.request');
  assert.equal(parsed.command, 'stop');
});

test('keeps approval responses typed and rejects non-boolean decisions', () => {
  assert.throws(
    () =>
      parseRuntimeMessage(
        JSON.stringify({
          protocol: RUNTIME_PROTOCOL,
          version: RUNTIME_PROTOCOL_VERSION,
          type: 'approval.respond',
          request_id: 'approval-1',
          conversation_id: 'conversation-1',
          approval_id: 'approval-1',
          approved: 'yes',
        }),
      ),
    /approved.*boolean/,
  );
});

test('parses generic input responses independently of Telegram', () => {
  const parsed = parseRuntimeMessage(
    JSON.stringify({
      protocol: RUNTIME_PROTOCOL,
      version: RUNTIME_PROTOCOL_VERSION,
      type: 'input.respond',
      request_id: 'input-response-1',
      conversation_id: 'conversation-1',
      input_id: 'choice-1',
      value: 'Use the safe option',
    }),
  );

  assert.deepEqual(parsed, {
    protocol: RUNTIME_PROTOCOL,
    version: RUNTIME_PROTOCOL_VERSION,
    type: 'input.respond',
    request_id: 'input-response-1',
    conversation_id: 'conversation-1',
    input_id: 'choice-1',
    value: 'Use the safe option',
  });
});
