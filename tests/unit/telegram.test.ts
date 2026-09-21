import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';

import {
  TelegramAdapter,
  TelegramAuthorization,
  conversationIdForTelegram,
  groupIsTriggered,
  runtimeCommandFromText,
} from '../../clients/telegram/index.js';
import type { RuntimeAttachment, RuntimeEvent } from '../../src/runtime/protocol.js';
import type {
  TelegramApi,
  TelegramBot,
  TelegramMessage,
  TelegramRuntime,
  TelegramSentMessage,
  TelegramUpdate,
} from '../../clients/telegram/types.js';

class FakeApi implements TelegramApi {
  public readonly sent: Array<{ chatId: number | string; text: string }> = [];
  public readonly sentOptions: Array<Record<string, unknown> | undefined> = [];
  public readonly edits: Array<{ chatId: number | string; messageId: number; text: string }> = [];
  public readonly markupEdits: Array<{ chatId: number | string; messageId: number }> = [];
  public readonly callbackAnswers: Array<{ id: string; text?: string }> = [];
  public readonly downloaded: string[] = [];
  public readonly sentAttachments: string[] = [];
  private nextMessageId = 100;

  public async getMe(): Promise<TelegramBot> {
    return { id: 42, username: 'atlas_bot' };
  }

  public async getUpdates(
    _offset: number | undefined,
    _timeoutSeconds: number,
  ): Promise<TelegramUpdate[]> {
    return [];
  }

  public async sendMessage(
    chatId: number | string,
    text: string,
    options?: { reply_markup?: { inline_keyboard: Array<Array<Record<string, unknown>>> } },
  ): Promise<TelegramSentMessage> {
    this.sent.push({ chatId, text });
    this.sentOptions.push(options as Record<string, unknown> | undefined);
    return { message_id: this.nextMessageId++, chat: { id: chatId, type: 'private' } };
  }

  public async editMessageText(
    chatId: number | string,
    messageId: number,
    text: string,
  ): Promise<void> {
    this.edits.push({ chatId, messageId, text });
  }

  public async sendPhoto(_chatId: number | string, filePath: string): Promise<void> {
    this.sentAttachments.push(`photo:${filePath}`);
  }

  public async sendDocument(_chatId: number | string, filePath: string): Promise<void> {
    this.sentAttachments.push(`document:${filePath}`);
  }

  public async editMessageReplyMarkup(chatId: number | string, messageId: number): Promise<void> {
    this.markupEdits.push({ chatId, messageId });
  }

  public async answerCallbackQuery(id: string, text?: string): Promise<void> {
    this.callbackAnswers.push({ id, ...(text === undefined ? {} : { text }) });
  }

  public async getFile(fileId: string): Promise<{ file_path: string; file_size?: number }> {
    return { file_path: `${fileId}.jpg`, file_size: 3 };
  }

  public async downloadFile(filePath: string, destination: string): Promise<void> {
    this.downloaded.push(filePath);
    await mkdir(join(destination, '..'), { recursive: true });
    await writeFile(destination, 'img');
  }
}

class FakeRuntime implements TelegramRuntime {
  public readonly requests: Array<Record<string, unknown>> = [];
  private activeResolve: ((event: RuntimeEvent) => void) | undefined;
  private activeCallback: ((event: RuntimeEvent) => void) | undefined;
  private approvalResolve: ((event: RuntimeEvent) => void) | undefined;

  public runTurn(
    request: {
      request_id: string;
      conversation_id: string;
      input: string;
      attachments?: RuntimeAttachment[];
    },
    onEvent: (event: RuntimeEvent) => void,
  ): Promise<RuntimeEvent> {
    this.requests.push(request);
    return new Promise((resolve) => {
      this.activeResolve = resolve;
      this.activeCallback = onEvent;
      const delta = {
        protocol: 'atlas-runtime' as const,
        version: 1 as const,
        type: 'message.delta' as const,
        request_id: request.request_id,
        conversation_id: request.conversation_id,
        data: { message_id: 'm1', delta: 'Olá' },
      };
      onEvent(delta);
      const completed = {
        protocol: 'atlas-runtime' as const,
        version: 1 as const,
        type: 'message.completed' as const,
        request_id: request.request_id,
        conversation_id: request.conversation_id,
        data: { message_id: 'm1', content: 'Olá' },
      };
      onEvent(completed);
      if (request.input === 'approval') {
        onEvent({
          protocol: 'atlas-runtime',
          version: 1,
          type: 'approval.requested',
          request_id: request.request_id,
          conversation_id: request.conversation_id,
          data: { approval_id: 'approval-1', tool_name: 'shell.exec', reason: 'Executar comando.' },
        });
        this.approvalResolve = resolve;
        return;
      }
      if (request.input !== 'wait') {
        resolve({
          ...completed,
          type: 'turn.completed',
          data: {
            content: 'Olá',
            attachments: [
              {
                type: 'document',
                uri: 'file:///tmp/output.txt',
                media_type: 'text/plain',
                file_name: 'output.txt',
              },
            ],
          },
        });
      }
    });
  }

  public async command(
    conversationId: string,
    command: 'new' | 'status' | 'stop',
  ): Promise<RuntimeEvent> {
    if (
      command === 'stop' &&
      this.activeResolve !== undefined &&
      this.activeCallback !== undefined
    ) {
      const cancelled: RuntimeEvent = {
        protocol: 'atlas-runtime',
        version: 1,
        type: 'turn.cancelled',
        request_id: 'active',
        conversation_id: conversationId,
        data: { content: 'Olá' },
      };
      this.activeCallback(cancelled);
      this.activeResolve(cancelled);
    }
    return {
      protocol: 'atlas-runtime',
      version: 1,
      type: 'command.completed',
      request_id: 'command',
      conversation_id: conversationId,
      data: {
        command,
        message: command === 'status' ? 'Sessão ativa.' : 'Turno cancelado.',
        session: {
          id: conversationId,
          model: 'model',
          provider: 'provider',
          status: command === 'stop' ? 'cancelled' : 'running',
        },
      },
    };
  }

  public async respondApproval(
    conversationId: string,
    approvalId: string,
    approved: boolean,
    comment?: string,
  ): Promise<RuntimeEvent> {
    this.approvalResolve?.({
      protocol: 'atlas-runtime',
      version: 1,
      type: 'turn.completed',
      request_id: 'approval-turn',
      conversation_id: conversationId,
      data: { content: approved ? 'Aprovado' : 'Rejeitado' },
    });
    this.approvalResolve = undefined;
    return {
      protocol: 'atlas-runtime',
      version: 1,
      type: 'approval.resolved',
      request_id: 'approval-response',
      conversation_id: conversationId,
      data: { approval_id: approvalId, approved, ...(comment === undefined ? {} : { comment }) },
    };
  }
}

class ScriptedRuntime implements TelegramRuntime {
  public readonly requests: Array<Record<string, unknown>> = [];

  public constructor(
    private readonly script: (
      request: {
        request_id: string;
        conversation_id: string;
        input: string;
        attachments?: RuntimeAttachment[];
      },
      onEvent: (event: RuntimeEvent) => void,
    ) => Promise<RuntimeEvent>,
  ) {}

  public runTurn(
    request: {
      request_id: string;
      conversation_id: string;
      input: string;
      attachments?: RuntimeAttachment[];
    },
    onEvent: (event: RuntimeEvent) => void,
  ): Promise<RuntimeEvent> {
    this.requests.push(request);
    return this.script(request, onEvent);
  }

  public async command(): Promise<RuntimeEvent> {
    throw new Error('ScriptedRuntime does not implement commands.');
  }

  public async respondApproval(): Promise<RuntimeEvent> {
    throw new Error('ScriptedRuntime does not implement approvals.');
  }
}

class DelayedEditApi extends FakeApi {
  public readonly firstEditStarted: Promise<void>;
  private readonly releaseFirstEdit: Promise<void>;
  private resolveFirstEditStarted: (() => void) | undefined;
  private resolveReleaseFirstEdit: (() => void) | undefined;
  private editCount = 0;

  public constructor() {
    super();
    this.firstEditStarted = new Promise((resolve) => {
      this.resolveFirstEditStarted = resolve;
    });
    this.releaseFirstEdit = new Promise((resolve) => {
      this.resolveReleaseFirstEdit = resolve;
    });
  }

  public release(): void {
    this.resolveReleaseFirstEdit?.();
  }

  public override async editMessageText(
    chatId: number | string,
    messageId: number,
    text: string,
  ): Promise<void> {
    this.editCount += 1;
    if (this.editCount === 1) {
      this.resolveFirstEditStarted?.();
      await this.releaseFirstEdit;
    }
    await super.editMessageText(chatId, messageId, text);
  }
}

function runtimeEvent(type: RuntimeEvent['type'], data: Record<string, unknown>): RuntimeEvent {
  return {
    protocol: 'atlas-runtime',
    version: 1,
    type,
    data,
  };
}

function lastEdits(api: FakeApi): Map<number, string> {
  const result = new Map<number, string>();
  for (const edit of api.edits) {
    result.set(edit.messageId, edit.text);
  }
  return result;
}

async function waitFor(milliseconds: number): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function message(overrides: Partial<TelegramMessage> = {}): TelegramMessage {
  return {
    message_id: 1,
    chat: { id: 123, type: 'private' },
    from: { id: 7, first_name: 'Caio' },
    text: 'olá',
    ...overrides,
  };
}

test('routes chat and topics to stable independent conversations', () => {
  assert.equal(conversationIdForTelegram(123), 'telegram:123:thread:root');
  assert.equal(conversationIdForTelegram(123, 9), 'telegram:123:thread:9');
  assert.notEqual(conversationIdForTelegram(123, 9), conversationIdForTelegram(123, 10));
  assert.equal(conversationIdForTelegram(-100123, 9), 'telegram:-100123:thread:9');
});

test('denies access by default and accepts explicit user or chat allowlists', () => {
  const privateMessage = message();
  assert.equal(new TelegramAuthorization().allows(privateMessage), false);
  assert.equal(new TelegramAuthorization({ allowedUsers: [7] }).allows(privateMessage), true);
  assert.equal(new TelegramAuthorization({ allowedChats: [123] }).allows(privateMessage), true);
});

test('derives commands from the Runtime registry and gates groups by mention or reply', () => {
  assert.equal(runtimeCommandFromText('/status@atlas_bot'), 'status');
  assert.equal(runtimeCommandFromText('/unknown'), undefined);
  const group = message({ chat: { id: -10, type: 'supergroup' }, text: 'oi' });
  assert.equal(groupIsTriggered(group, { id: 42, username: 'atlas_bot' }), false);
  assert.equal(
    groupIsTriggered(
      { ...group, text: '@atlas_bot oi', entities: [{ type: 'mention', offset: 0, length: 10 }] },
      { id: 42, username: 'atlas_bot' },
    ),
    true,
  );
  assert.equal(
    groupIsTriggered(
      {
        ...group,
        text: undefined,
        caption: '@atlas_bot foto',
        caption_entities: [{ type: 'mention', offset: 0, length: 10 }],
      },
      { id: 42, username: 'atlas_bot' },
    ),
    true,
  );
  assert.equal(
    groupIsTriggered(
      { ...group, reply_to_message: message({ from: { id: 42, is_bot: true } }) },
      { id: 42 },
    ),
    true,
  );
});

test('streams deltas by editing one Telegram message and preserves typed attachments', async () => {
  const api = new FakeApi();
  const runtime = new FakeRuntime();
  const adapter = new TelegramAdapter({
    api,
    runtime,
    allowedUsers: [7],
    downloadDirectory: '/tmp/atlas-telegram-test',
  });
  await adapter.handleUpdate({
    update_id: 1,
    message: message({
      text: 'veja',
      photo: [{ file_id: 'photo-1', width: 10, height: 10 }],
    }),
  });

  assert.equal(api.sent[0]?.text, '⏳');
  assert.ok(api.edits.some((edit) => edit.messageId === 100 && edit.text === 'Olá'));
  assert.equal(runtime.requests[0]?.conversation_id, 'telegram:123:thread:root');
  const attachments = runtime.requests[0]?.attachments as Array<Record<string, unknown>>;
  assert.equal(attachments[0]?.type, 'image');
  assert.equal(
    attachments[0]?.source && (attachments[0].source as Record<string, string>).platform,
    'telegram',
  );
  assert.deepEqual(api.sentAttachments, ['document:/tmp/output.txt']);
});

test('keeps rapid out-of-order snapshots isolated by message_id', async () => {
  const api = new FakeApi();
  const runtime = new ScriptedRuntime(async (_request, onEvent) => {
    onEvent(runtimeEvent('message.delta', { message_id: 'm1', delta: 'um' }));
    onEvent(runtimeEvent('message.delta', { message_id: 'm1', delta: ' dois' }));
    onEvent(runtimeEvent('message.delta', { message_id: 'm2', delta: 'outro' }));
    onEvent(runtimeEvent('message.completed', { message_id: 'm1', content: 'um dois' }));
    onEvent(runtimeEvent('message.delta', { message_id: 'm2', delta: ' texto' }));
    onEvent(runtimeEvent('message.completed', { message_id: 'm2', content: 'outro texto' }));
    return runtimeEvent('turn.completed', {
      message_id: 'm2',
      content: 'outro texto',
    });
  });
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });

  await adapter.handleUpdate({ update_id: 10, message: message({ text: 'duas mensagens' }) });

  const edits = lastEdits(api);
  assert.equal(edits.get(100), 'um dois');
  assert.equal(edits.get(101), 'outro texto');
  assert.equal(edits.size, 2);
});

test('does not regress a message when completion arrives after deltas', async () => {
  const api = new FakeApi();
  const runtime = new ScriptedRuntime(async (_request, onEvent) => {
    onEvent(runtimeEvent('message.delta', { message_id: 'm1', delta: 'resposta ' }));
    onEvent(runtimeEvent('message.delta', { message_id: 'm1', delta: 'completa' }));
    onEvent(runtimeEvent('message.completed', { message_id: 'm1', content: 'resposta' }));
    return runtimeEvent('turn.completed', { message_id: 'm1', content: 'resposta' });
  });
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });

  await adapter.handleUpdate({ update_id: 11, message: message({ text: 'completo' }) });

  assert.equal(lastEdits(api).get(100), 'resposta completa');
});

for (const [name, length, expectedChunks] of [
  ['4096', 4097, 2],
  ['8192', 8193, 3],
] as const) {
  test(`streams responses larger than ${name} characters without truncation`, async () => {
    const api = new FakeApi();
    const content = 'x'.repeat(length);
    const runtime = new ScriptedRuntime(async (_request, onEvent) => {
      onEvent(runtimeEvent('message.delta', { message_id: 'm1', delta: content }));
      return runtimeEvent('turn.completed', { message_id: 'm1', content });
    });
    const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });

    await adapter.handleUpdate({ update_id: length, message: message({ text: `long ${name}` }) });

    const edits = lastEdits(api);
    assert.equal(edits.size, expectedChunks);
    assert.equal([...edits.values()].join(''), content);
    assert.ok([...edits.values()].every((chunk) => Array.from(chunk).length <= 4096));
  });
}

test('renders the newest snapshot after a delayed edit', async () => {
  const api = new DelayedEditApi();
  const runtime = new ScriptedRuntime(async (_request, onEvent) => {
    onEvent(runtimeEvent('message.delta', { message_id: 'm1', delta: 'antigo' }));
    await waitFor(1_050);
    await api.firstEditStarted;
    onEvent(runtimeEvent('message.delta', { message_id: 'm1', delta: ' e novo' }));
    api.release();
    return runtimeEvent('turn.completed', { message_id: 'm1', content: 'antigo e novo' });
  });
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });

  await adapter.handleUpdate({ update_id: 12, message: message({ text: 'atualize' }) });

  const messageEdits = api.edits.filter((edit) => edit.messageId === 100);
  assert.deepEqual(
    messageEdits.map((edit) => edit.text),
    ['antigo', 'antigo e novo'],
  );
});

test('routes status and stop while a turn is active', async () => {
  const api = new FakeApi();
  const runtime = new FakeRuntime();
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });
  const active = adapter.handleUpdate({ update_id: 1, message: message({ text: 'wait' }) });
  await new Promise((resolve) => setTimeout(resolve, 20));
  await adapter.handleUpdate({
    update_id: 2,
    message: message({ message_id: 2, text: '/status' }),
  });
  await adapter.handleUpdate({ update_id: 3, message: message({ message_id: 3, text: '/stop' }) });
  await active;

  assert.ok(api.sent.some((entry) => entry.text.includes('Sessão ativa.')));
  assert.ok(api.sent.some((entry) => entry.text.includes('Turno cancelado.')));
  assert.ok(api.edits.some((edit) => edit.text === 'Olá'));
});

test('resolves approvals through Telegram callback buttons', async () => {
  const api = new FakeApi();
  const runtime = new FakeRuntime();
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });
  const active = adapter.handleUpdate({ update_id: 1, message: message({ text: 'approval' }) });
  await new Promise((resolve) => setTimeout(resolve, 20));

  const approvalMessage = api.sent.find((entry) => entry.text.includes('Aprovação necessária'));
  assert.ok(approvalMessage);
  const approvalOptions = api.sentOptions.find(
    (options) => options?.reply_markup !== undefined,
  ) as { reply_markup: { inline_keyboard: Array<Array<{ callback_data?: string }>> } };
  const callbackData = approvalOptions.reply_markup.inline_keyboard[0]?.[0]?.callback_data;
  assert.ok(callbackData);

  await adapter.handleUpdate({
    update_id: 2,
    callback_query: {
      id: 'callback-1',
      from: { id: 7 },
      data: callbackData,
      message: {
        message_id: 101,
        chat: { id: 123, type: 'private' },
      },
    },
  });
  await active;

  assert.deepEqual(api.callbackAnswers, [{ id: 'callback-1', text: 'Aprovado.' }]);
  assert.deepEqual(api.markupEdits, [{ chatId: 123, messageId: 101 }]);
});
