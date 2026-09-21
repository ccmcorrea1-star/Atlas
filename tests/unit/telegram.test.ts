import assert from 'node:assert/strict';
import { mkdir, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

import {
  TelegramAdapter,
  TelegramAuthorization,
  conversationIdForTelegram,
  groupIsTriggered,
  runtimeCommandFromText,
} from '../../clients/telegram/index.js';
import type {
  RuntimeAttachment,
  RuntimeEvent,
  RuntimeTurnContext,
} from '../../src/runtime/protocol.js';
import type {
  TelegramApi,
  TelegramBot,
  TelegramMessage,
  TelegramMediaGroupItem,
  TelegramRuntime,
  TelegramSentMessage,
  TelegramUpdate,
} from '../../clients/telegram/types.js';
import {
  splitTelegramText,
  telegramUtf16Length,
  renderTelegramMarkdown,
} from '../../clients/telegram/text.js';

class FakeApi implements TelegramApi {
  public readonly sent: Array<{ chatId: number | string; text: string }> = [];
  public readonly sentOptions: Array<Record<string, unknown> | undefined> = [];
  public readonly edits: Array<{ chatId: number | string; messageId: number; text: string }> = [];
  public readonly operations: Array<{ type: 'send' | 'edit'; at: number; text: string }> = [];
  public readonly markupEdits: Array<{ chatId: number | string; messageId: number }> = [];
  public readonly callbackAnswers: Array<{ id: string; text?: string }> = [];
  public readonly downloaded: string[] = [];
  public readonly sentAttachments: string[] = [];
  public readonly sentAlbums: TelegramMediaGroupItem[][] = [];
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
    this.operations.push({ type: 'send', at: Date.now(), text });
    this.sent.push({ chatId, text });
    this.sentOptions.push(options as Record<string, unknown> | undefined);
    return { message_id: this.nextMessageId++, chat: { id: chatId, type: 'private' } };
  }

  public async editMessageText(
    chatId: number | string,
    messageId: number,
    text: string,
  ): Promise<void> {
    this.operations.push({ type: 'edit', at: Date.now(), text });
    this.edits.push({ chatId, messageId, text });
  }

  public async sendPhoto(_chatId: number | string, filePath: string): Promise<void> {
    this.sentAttachments.push(`photo:${filePath}`);
  }

  public async sendVideo(_chatId: number | string, filePath: string): Promise<void> {
    this.sentAttachments.push(`video:${filePath}`);
  }

  public async sendAnimation(_chatId: number | string, filePath: string): Promise<void> {
    this.sentAttachments.push(`animation:${filePath}`);
  }

  public async sendMediaGroup(
    _chatId: number | string,
    items: TelegramMediaGroupItem[],
  ): Promise<void> {
    this.sentAlbums.push(items);
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

class RetryAfterApi extends FakeApi {
  public failedEditAt: number | undefined;
  private failNextEdit = true;

  public override async editMessageText(
    chatId: number | string,
    messageId: number,
    text: string,
  ): Promise<void> {
    if (this.failNextEdit) {
      this.failNextEdit = false;
      this.failedEditAt = Date.now();
      throw Object.assign(new Error('Too Many Requests'), { retry_after: 0.05 });
    }
    await super.editMessageText(chatId, messageId, text);
  }
}

class PartialDeliveryApi extends FakeApi {
  private failNextContinuation = true;

  public override async sendMessage(
    chatId: number | string,
    text: string,
    options?: { reply_markup?: { inline_keyboard: Array<Array<Record<string, unknown>>> } },
  ): Promise<TelegramSentMessage> {
    if (text.length === 4096 && this.failNextContinuation) {
      this.failNextContinuation = false;
      throw new Error('permanent delivery failure');
    }
    return super.sendMessage(chatId, text, options);
  }
}

class FakeRuntime implements TelegramRuntime {
  public readonly requests: Array<Record<string, unknown>> = [];
  public readonly reactions: Array<Record<string, unknown>> = [];
  private notificationHandler: ((event: RuntimeEvent) => void) | undefined;
  private activeResolve: ((event: RuntimeEvent) => void) | undefined;
  private activeCallback: ((event: RuntimeEvent) => void) | undefined;
  private approvalResolve: ((event: RuntimeEvent) => void) | undefined;
  private inputResolve: ((event: RuntimeEvent) => void) | undefined;

  public runTurn(
    request: {
      request_id: string;
      conversation_id: string;
      input: string;
      attachments?: RuntimeAttachment[];
      context?: RuntimeTurnContext;
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
      if (request.input === 'question') {
        onEvent({
          protocol: 'atlas-runtime',
          version: 1,
          type: 'input.requested',
          request_id: request.request_id,
          conversation_id: request.conversation_id,
          data: { input_id: 'question-1', prompt: 'Escolha uma opção.', choices: ['sim', 'não'] },
        });
        this.inputResolve = resolve;
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

  public subscribeNotifications(onEvent: (event: RuntimeEvent) => void): () => void {
    this.notificationHandler = onEvent;
    return () => {
      this.notificationHandler = undefined;
    };
  }

  public emitNotification(event: RuntimeEvent): void {
    this.notificationHandler?.(event);
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

  public async react(
    conversationId: string,
    messageId: string,
    action: 'added' | 'removed' | 'changed',
    reactions: string[],
    source: string,
    actorId?: string,
  ): Promise<RuntimeEvent> {
    this.reactions.push({ conversationId, messageId, action, reactions, source, actorId });
    return {
      protocol: 'atlas-runtime',
      version: 1,
      type: 'reaction.completed',
      request_id: 'reaction',
      conversation_id: conversationId,
      data: {
        message_id: messageId,
        action,
        reactions,
        source,
        ...(actorId === undefined ? {} : { actor_id: actorId }),
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

  public async respondInput(
    conversationId: string,
    inputId: string,
    value: string,
  ): Promise<RuntimeEvent> {
    this.inputResolve?.({
      protocol: 'atlas-runtime',
      version: 1,
      type: 'turn.completed',
      request_id: 'input-turn',
      conversation_id: conversationId,
      data: { content: value },
    });
    this.inputResolve = undefined;
    return {
      protocol: 'atlas-runtime',
      version: 1,
      type: 'input.resolved',
      request_id: 'input-response',
      conversation_id: conversationId,
      data: { input_id: inputId, value },
    };
  }
}

class ScriptedRuntime implements TelegramRuntime {
  public readonly requests: Array<Record<string, unknown>> = [];
  public readonly reactions: Array<Record<string, unknown>> = [];

  public constructor(
    private readonly script: (
      request: {
        request_id: string;
        conversation_id: string;
        input: string;
        attachments?: RuntimeAttachment[];
        context?: RuntimeTurnContext;
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
      context?: RuntimeTurnContext;
    },
    onEvent: (event: RuntimeEvent) => void,
  ): Promise<RuntimeEvent> {
    this.requests.push(request);
    return this.script(request, onEvent);
  }

  public async command(): Promise<RuntimeEvent> {
    throw new Error('ScriptedRuntime does not implement commands.');
  }

  public async react(
    conversationId: string,
    messageId: string,
    action: 'added' | 'removed' | 'changed',
    reactions: string[],
    source: string,
    actorId?: string,
  ): Promise<RuntimeEvent> {
    this.reactions.push({ conversationId, messageId, action, reactions, source, actorId });
    return {
      protocol: 'atlas-runtime',
      version: 1,
      type: 'reaction.completed',
      request_id: 'reaction',
      conversation_id: conversationId,
      data: {
        message_id: messageId,
        action,
        reactions,
        source,
        ...(actorId === undefined ? {} : { actor_id: actorId }),
      },
    };
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
  assert.equal(runtimeCommandFromText('/help'), 'help');
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

test('renderiza Markdown comum com escape seguro para MarkdownV2', () => {
  const rendered = renderTelegramMarkdown('# Título\n\n**forte** e `código`\n\nitem-a');
  assert.equal(rendered, '*Título*\n\n*forte* e `código`\n\nitem\\-a');
  assert.equal(
    renderTelegramMarkdown('[Atlas](https://atlas.local)'),
    '[Atlas](https://atlas.local)',
  );
  assert.equal(renderTelegramMarkdown('> citação'), '> citação');
});

test('responde /help sem criar uma chamada ao Runtime', async () => {
  const api = new FakeApi();
  const runtime = new FakeRuntime();
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });

  await adapter.handleUpdate({ update_id: 4, message: message({ text: '/help' }) });

  assert.equal(runtime.requests.length, 0);
  assert.match(api.sent.at(-1)?.text ?? '', /\/help/iu);
  assert.match(api.sent.at(-1)?.text ?? '', /\/status/iu);
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
      video: { file_id: 'video-1', mime_type: 'video/mp4' },
    }),
  });

  assert.equal(api.sent[0]?.text, '⏳');
  assert.ok(api.edits.some((edit) => edit.messageId === 100 && edit.text === 'Olá'));
  assert.equal(runtime.requests[0]?.conversation_id, 'telegram:123:thread:root');
  const attachments = runtime.requests[0]?.attachments as Array<Record<string, unknown>>;
  assert.equal(attachments[0]?.type, 'image');
  assert.equal(attachments[1]?.type, 'video');
  assert.equal(
    attachments[0]?.source && (attachments[0].source as Record<string, string>).platform,
    'telegram',
  );
  assert.deepEqual(api.sentAttachments, ['document:/tmp/output.txt']);
});

test('envia o contexto tipado da mensagem respondida ao Runtime', async () => {
  const api = new FakeApi();
  const runtime = new ScriptedRuntime(async () =>
    runtimeEvent('turn.completed', { content: 'contexto recebido' }),
  );
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });

  await adapter.handleUpdate({
    update_id: 20,
    message: message({
      message_id: 21,
      text: 'continue',
      reply_to_message: message({
        message_id: 20,
        text: 'mensagem original',
        from: { id: 8, username: 'ana' },
        photo: [{ file_id: 'photo-reply', width: 10, height: 10 }],
      }),
    }),
  });

  assert.deepEqual(runtime.requests[0]?.context, {
    reply_to: {
      source: 'telegram',
      message_id: '20',
      author: 'ana',
      text: 'mensagem original',
      media: ['photo'],
    },
  });
});

test('entrega vídeo e animação usando as APIs nativas do Telegram', async () => {
  const api = new FakeApi();
  const runtime = new ScriptedRuntime(async () =>
    runtimeEvent('turn.completed', {
      content: 'arquivos prontos',
      attachments: [
        {
          type: 'video',
          uri: 'file:///tmp/video.mp4',
          media_type: 'video/mp4',
          file_name: 'video.mp4',
        },
        {
          type: 'animation',
          uri: 'file:///tmp/animation.gif',
          media_type: 'image/gif',
          file_name: 'animation.gif',
        },
      ],
    }),
  );
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });

  await adapter.handleUpdate({ update_id: 5, message: message({ text: 'envie os arquivos' }) });

  assert.deepEqual(api.sentAttachments, ['video:/tmp/video.mp4', 'animation:/tmp/animation.gif']);
});
test('agrupa fotos e vídeos em um álbum do Telegram', async () => {
  const api = new FakeApi();
  const runtime = new ScriptedRuntime(async () =>
    runtimeEvent('turn.completed', {
      content: 'álbum pronto',
      attachments: [
        { type: 'image', uri: 'file:///tmp/photo.jpg', media_type: 'image/jpeg' },
        { type: 'video', uri: 'file:///tmp/video.mp4', media_type: 'video/mp4' },
      ],
    }),
  );
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });

  await adapter.handleUpdate({ update_id: 6, message: message({ text: 'envie o álbum' }) });

  assert.deepEqual(api.sentAlbums, [
    [
      { type: 'photo', filePath: '/tmp/photo.jpg' },
      { type: 'video', filePath: '/tmp/video.mp4' },
    ],
  ]);
});
test('agrupa mensagens de um álbum recebido em um único turno', async () => {
  const api = new FakeApi();
  const runtime = new ScriptedRuntime(async (request) =>
    runtimeEvent('turn.completed', {
      content: `recebidos ${request.attachments?.length ?? 0}`,
    }),
  );
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });
  const first = {
    update_id: 7,
    message: message({
      message_id: 70,
      media_group_id: 'album-1',
      text: 'álbum',
      photo: [{ file_id: 'photo-a', width: 10, height: 10 }],
    }),
  };
  const second = {
    update_id: 8,
    message: message({
      message_id: 71,
      media_group_id: 'album-1',
      text: undefined,
      photo: [{ file_id: 'photo-b', width: 20, height: 20 }],
    }),
  };

  await Promise.all([adapter.handleUpdate(first), adapter.handleUpdate(second)]);

  assert.equal(runtime.requests.length, 1);
  assert.equal((runtime.requests[0]?.attachments as unknown[]).length, 2);
});

test('materializa locations e venues como anexos estruturados', async () => {
  const api = new FakeApi();
  const runtime = new ScriptedRuntime(async (request) =>
    runtimeEvent('turn.completed', {
      content: String(request.attachments?.length ?? 0),
    }),
  );
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });

  await adapter.handleUpdate({
    update_id: 22,
    message: message({
      message_id: 22,
      text: 'analise estes locais',
      location: { latitude: -23.55, longitude: -46.63, horizontal_accuracy: 12 },
      venue: {
        location: { latitude: -23.56, longitude: -46.64 },
        title: 'Praça Central',
        address: 'Rua A, 10',
      },
    }),
  });

  const attachments = runtime.requests[0]?.attachments as Array<Record<string, unknown>>;
  assert.deepEqual(
    attachments.map((attachment) => attachment.type),
    ['location', 'venue'],
  );
  assert.equal(attachments[0]?.uri, 'geo:-23.55,-46.63');
  assert.match(String(attachments[1]?.description), /Praça Central/iu);
  assert.deepEqual(api.downloaded, []);
});

test('aceita posts de canal e transforma stickers em imagens tipadas', async () => {
  const api = new FakeApi();
  const runtime = new ScriptedRuntime(async (request) =>
    runtimeEvent('turn.completed', {
      content: request.attachments?.[0]?.media_type ?? 'sem anexo',
    }),
  );
  const adapter = new TelegramAdapter({ api, runtime, allowedChats: [-100] });

  await adapter.handleUpdate({
    update_id: 9,
    channel_post: {
      message_id: 90,
      chat: { id: -100, type: 'channel' },
      text: 'sticker',
      sticker: { file_id: 'sticker-1' },
    },
  });

  assert.equal(runtime.requests.length, 1);
  assert.equal(
    (runtime.requests[0]?.attachments as Array<{ media_type: string }>)[0]?.media_type,
    'image/webp',
  );
});

test('rematerializa um anexo pendente após recuperação do Runtime', async () => {
  const statePath = join('/tmp', `atlas-telegram-attachment-${Date.now()}-${Math.random()}.json`);
  const api = new FakeApi();
  const update = {
    update_id: 2,
    message: message({
      message_id: 2,
      text: 'analise',
      document: { file_id: 'document-1', file_name: 'arquivo.txt', mime_type: 'text/plain' },
    }),
  };
  const failedRuntime = new ScriptedRuntime(async () => {
    throw new Error('Runtime caiu antes do terminal');
  });
  try {
    const firstAdapter = new TelegramAdapter({
      api,
      runtime: failedRuntime,
      allowedUsers: [7],
      statePath,
      downloadDirectory: '/tmp/atlas-telegram-recovery',
    });
    await assert.rejects(firstAdapter.handleUpdate(update), /Runtime caiu/);

    let recoveredPath: string | undefined;
    const recoveredRuntime = new ScriptedRuntime(async (request, onEvent) => {
      const attachment = request.attachments?.[0];
      assert.ok(attachment);
      recoveredPath = attachment.uri;
      await stat(fileURLToPath(attachment.uri));
      onEvent(runtimeEvent('message.completed', { message_id: 'm1', content: 'recuperado' }));
      return runtimeEvent('turn.completed', { message_id: 'm1', content: 'recuperado' });
    });
    const secondAdapter = new TelegramAdapter({
      api,
      runtime: recoveredRuntime,
      allowedUsers: [7],
      statePath,
      downloadDirectory: '/tmp/atlas-telegram-recovery',
    });
    await secondAdapter.handleUpdate({ ...update, update_id: 3 });

    assert.ok(recoveredPath);
    assert.ok(api.downloaded.filter((path) => path === 'document-1.jpg').length >= 2);
    assert.doesNotMatch(await readFile(statePath, 'utf8'), /file:\/\/\/.*turn-/u);
  } finally {
    await rm(statePath, { force: true });
    await rm('/tmp/atlas-telegram-recovery', { recursive: true, force: true });
  }
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
  assert.ok(api.sent.some((entry) => entry.text === 'outro texto'));
  assert.equal(edits.size, 1);
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

  assert.equal(lastEdits(api).get(100), 'resposta');
});

test('uses Telegram UTF-16 units without splitting a surrogate pair', () => {
  const content = `${'x'.repeat(4_095)}😀${'y'.repeat(4_095)}`;
  const chunks = splitTelegramText(content);
  assert.equal(chunks[0], 'x'.repeat(4_095));
  assert.equal(telegramUtf16Length(chunks[0] ?? ''), 4_095);
  assert.ok(chunks.every((chunk) => telegramUtf16Length(chunk) <= 4_096));
  assert.equal(chunks.join(''), content);
});

test('persists the Telegram message identity across an adapter restart', async () => {
  const statePath = join('/tmp', `atlas-telegram-state-${Date.now()}-${Math.random()}.json`);
  const api = new FakeApi();
  const runtime = new ScriptedRuntime(async (_request, onEvent) => {
    onEvent(runtimeEvent('message.completed', { message_id: 'm1', content: 'persistido' }));
    return runtimeEvent('turn.completed', { message_id: 'm1', content: 'persistido' });
  });
  const update = { update_id: 100, message: message({ message_id: 55, text: 'persistir' }) };
  try {
    await new TelegramAdapter({ api, runtime, allowedUsers: [7], statePath }).handleUpdate(update);
    await new TelegramAdapter({ api, runtime, allowedUsers: [7], statePath }).handleUpdate(update);
    assert.equal(runtime.requests.length, 1);
  } finally {
    await rm(statePath, { force: true });
  }
});

test('resumes long delivery from the first unconfirmed chunk', async () => {
  const api = new PartialDeliveryApi();
  const content = 'z'.repeat(8_193);
  const runtime = new ScriptedRuntime(async (_request, onEvent) => {
    onEvent(runtimeEvent('message.completed', { message_id: 'm1', content }));
    return runtimeEvent('turn.completed', { message_id: 'm1', content });
  });
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });
  const update = { update_id: 101, message: message({ message_id: 56, text: 'entrega parcial' }) };
  await assert.rejects(adapter.handleUpdate(update), /permanent delivery failure/);
  await adapter.handleUpdate({ ...update, update_id: 102 });

  const sentChunks = api.sent.slice(1).map((entry) => entry.text);
  assert.equal(sentChunks.length, 2);
  assert.equal(sentChunks.join(''), content.slice(4_096));
});

test('keeps outbound responses in the original topic', async () => {
  const api = new FakeApi();
  const runtime = new ScriptedRuntime(async (_request, onEvent) => {
    onEvent(runtimeEvent('message.completed', { message_id: 'm1', content: 'no tópico' }));
    return runtimeEvent('turn.completed', { message_id: 'm1', content: 'no tópico' });
  });
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });
  await adapter.handleUpdate({
    update_id: 103,
    message: message({ message_thread_id: 9, text: 'topic' }),
  });

  assert.equal(api.sentOptions[0]?.message_thread_id, 9);
  assert.ok(api.sentOptions.some((options) => options?.message_thread_id === 9));
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
    const delivered = [edits.get(100) ?? '', ...api.sent.slice(1).map((entry) => entry.text)].join(
      '',
    );
    assert.equal(delivered, content);
    assert.equal(edits.size, 1);
    assert.equal(api.sent.length, expectedChunks);
    assert.ok(api.sent.slice(1).every((entry) => Array.from(entry.text).length <= 4096));
  });
}

test('serializes many deltas without losing the complete response', async () => {
  const api = new FakeApi();
  const content = Array.from({ length: 120 }, (_, index) => `delta-${index}`).join(' ');
  const runtime = new ScriptedRuntime(async (_request, onEvent) => {
    for (const [index, delta] of content.split(' ').entries()) {
      onEvent(
        runtimeEvent('message.delta', {
          message_id: 'm1',
          delta: `${index === 0 ? '' : ' '}${delta}`,
        }),
      );
    }
    return runtimeEvent('turn.completed', { message_id: 'm1', content });
  });
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });

  await adapter.handleUpdate({ update_id: 20, message: message({ text: 'muitos deltas' }) });

  assert.equal(api.edits.at(-1)?.text, renderTelegramMarkdown(content));
});

test('keeps an oversized preview in one message until finalization', async () => {
  const api = new FakeApi();
  const content = 'x'.repeat(8_193);
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const runtime = new ScriptedRuntime(async (_request, onEvent) => {
    onEvent(runtimeEvent('message.delta', { message_id: 'm1', delta: content }));
    await gate;
    return runtimeEvent('turn.completed', { message_id: 'm1', content });
  });
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });
  const active = adapter.handleUpdate({ update_id: 21, message: message({ text: 'preview' }) });

  await waitFor(1_100);
  assert.equal(api.sent.length, 1);
  assert.equal(api.edits[0]?.text, content.slice(0, 4_096));
  release();
  await active;

  assert.equal(api.sent.length, 3);
  assert.equal(
    api.sent
      .slice(1)
      .map((entry) => entry.text)
      .join(''),
    content.slice(4_096),
  );
});

test('ignores an identical final edit after an intermediate preview', async () => {
  const api = new FakeApi();
  const runtime = new ScriptedRuntime(async (_request, onEvent) => {
    onEvent(runtimeEvent('message.delta', { message_id: 'm1', delta: 'igual' }));
    await waitFor(1_100);
    return runtimeEvent('turn.completed', { message_id: 'm1', content: 'igual' });
  });
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });

  await adapter.handleUpdate({ update_id: 22, message: message({ text: 'igual' }) });

  assert.deepEqual(
    api.edits.map((edit) => edit.text),
    ['igual'],
  );
});

test('shares one budget between the initial send, final edit and continuation sends', async () => {
  const api = new FakeApi();
  const content = 'y'.repeat(4_097);
  const runtime = new ScriptedRuntime(async (_request, onEvent) => {
    onEvent(runtimeEvent('message.delta', { message_id: 'm1', delta: content }));
    return runtimeEvent('turn.completed', { message_id: 'm1', content });
  });
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });

  await adapter.handleUpdate({ update_id: 23, message: message({ text: 'budget' }) });

  assert.deepEqual(
    api.operations.map((operation) => operation.type),
    ['send', 'edit', 'send'],
  );
  assert.ok(
    api.operations.every(
      (operation, index, all) =>
        index === 0 || operation.at - (all[index - 1]?.at ?? operation.at) >= 900,
    ),
  );
});

test('waits for retry_after before retrying a flooded edit', async () => {
  const api = new RetryAfterApi();
  const runtime = new ScriptedRuntime(async (_request, onEvent) => {
    onEvent(runtimeEvent('message.delta', { message_id: 'm1', delta: 'retry' }));
    await waitFor(1_100);
    return runtimeEvent('turn.completed', { message_id: 'm1', content: 'retry' });
  });
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });

  await adapter.handleUpdate({ update_id: 24, message: message({ text: 'flood' }) });

  const successfulEdit = api.operations.find((operation) => operation.type === 'edit');
  assert.ok(api.failedEditAt !== undefined);
  assert.ok(successfulEdit !== undefined);
  assert.ok(successfulEdit.at - api.failedEditAt >= 40);
  assert.equal(api.edits.at(-1)?.text, 'retry');
});

test('deduplicates the same Telegram message across different update ids', async () => {
  const api = new FakeApi();
  const runtime = new ScriptedRuntime(async (_request, onEvent) => {
    onEvent(runtimeEvent('message.delta', { message_id: 'm1', delta: 'uma vez' }));
    return runtimeEvent('turn.completed', { message_id: 'm1', content: 'uma vez' });
  });
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });
  const first = { update_id: 25, message: message({ message_id: 55, text: 'reconnect' }) };
  const second = { update_id: 26, message: message({ message_id: 55, text: 'reconnect' }) };

  await Promise.all([adapter.handleUpdate(first), adapter.handleUpdate(second)]);

  assert.equal(runtime.requests.length, 1);
});

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

test('processa uma edição como nova revisão e deduplica a mesma revisão', async () => {
  const api = new FakeApi();
  const runtime = new ScriptedRuntime(async (request) =>
    runtimeEvent('turn.completed', { content: `resposta: ${request.input}` }),
  );
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });
  const original = message({ message_id: 44, text: 'texto original' });
  const edited = message({ message_id: 44, text: 'texto corrigido' });

  await adapter.handleUpdate({ update_id: 44, message: original });
  await adapter.handleUpdate({ update_id: 45, edited_message: edited });
  await adapter.handleUpdate({ update_id: 46, edited_message: edited });

  assert.equal(runtime.requests.length, 2);
  assert.equal(runtime.requests[0]?.input, 'texto original');
  assert.equal(runtime.requests[1]?.input, 'texto corrigido');
});

test('normaliza reações de usuário e contagens sem criar turno de texto', async () => {
  const api = new FakeApi();
  const runtime = new FakeRuntime();
  const adapter = new TelegramAdapter({ runtime, api, allowedUsers: [7], allowedChats: [123] });

  await adapter.handleUpdate({
    update_id: 50,
    message_reaction: {
      chat: { id: 123, type: 'private' },
      message_id: 51,
      user: { id: 7, first_name: 'Caio' },
      date: 1,
      old_reaction: [],
      new_reaction: [{ type: 'emoji', emoji: '👍' }],
    },
  });
  await adapter.handleUpdate({
    update_id: 51,
    message_reaction_count: {
      chat: { id: 123, type: 'private' },
      message_id: 51,
      date: 2,
      reactions: [{ type: { type: 'emoji', emoji: '👍' }, total_count: 3 }],
    },
  });

  assert.equal(runtime.requests.length, 0);
  assert.equal(runtime.reactions.length, 2);
  assert.deepEqual(runtime.reactions[0], {
    conversationId: 'telegram:123:thread:root',
    messageId: '51',
    action: 'added',
    reactions: ['👍'],
    source: 'telegram',
    actorId: '7',
  });
  assert.deepEqual(runtime.reactions[1], {
    conversationId: 'telegram:123:thread:root',
    messageId: '51',
    action: 'changed',
    reactions: ['👍:3'],
    source: 'telegram',
    actorId: undefined,
  });
});

test('entrega notificações assíncronas no chat da conversa sem criar turno', async () => {
  const api = new FakeApi();
  const runtime = new FakeRuntime();
  const adapter = new TelegramAdapter({ runtime, api, allowedUsers: [7], allowedChats: [123] });

  runtime.emitNotification({
    protocol: 'atlas-runtime',
    version: 1,
    type: 'notification.created',
    request_id: 'notification-test',
    conversation_id: 'telegram:123:thread:root',
    data: {
      notification_id: 'notification-1',
      conversation_id: 'telegram:123:thread:root',
      content: 'Download concluído.',
      source: 'torrent',
      title: 'Atlas',
      level: 'success',
    },
  });
  await new Promise((resolve) => setImmediate(resolve));

  assert.equal(runtime.requests.length, 0);
  assert.deepEqual(api.sent, [{ chatId: 123, text: 'Atlas\n\nDownload concluído.' }]);
  await adapter.stop();
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

  assert.ok(api.sent.some((entry) => entry.text.includes(renderTelegramMarkdown('Sessão ativa.'))));
  assert.ok(
    api.sent.some((entry) => entry.text.includes(renderTelegramMarkdown('Turno cancelado.'))),
  );
  assert.ok(api.edits.some((edit) => edit.text === 'Olá'));
});

test('resolves approvals through Telegram callback buttons', async () => {
  const api = new FakeApi();
  const runtime = new FakeRuntime();
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });
  const active = adapter.handleUpdate({ update_id: 1, message: message({ text: 'approval' }) });
  await waitFor(1_050);

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

test('does not restore resolved approval or input callbacks after a bot restart', async () => {
  const statePath = join('/tmp', `atlas-telegram-pending-${Date.now()}-${Math.random()}.json`);
  const api = new FakeApi();
  const runtime = new FakeRuntime();
  try {
    const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7], statePath });
    const approvalTurn = adapter.handleUpdate({
      update_id: 40,
      message: message({ message_id: 40, text: 'approval' }),
    });
    await waitFor(1_050);
    const approvalOptions = api.sentOptions.find(
      (options) => options?.reply_markup !== undefined,
    ) as { reply_markup: { inline_keyboard: Array<Array<{ callback_data?: string }>> } };
    const approvalCallback = approvalOptions.reply_markup.inline_keyboard[0]?.[0]?.callback_data;
    assert.ok(approvalCallback);
    await runtime.respondApproval('telegram:123:thread:root', 'approval-1', true);
    await approvalTurn;

    const restarted = new TelegramAdapter({ api, runtime, allowedUsers: [7], statePath });
    await restarted.handleUpdate({
      update_id: 41,
      callback_query: {
        id: 'stale-approval',
        from: { id: 7 },
        data: approvalCallback,
        message: { message_id: 101, chat: { id: 123, type: 'private' } },
      },
    });
    assert.deepEqual(api.callbackAnswers.at(-1), {
      id: 'stale-approval',
      text: 'Esta aprovação já expirou.',
    });

    const inputTurn = restarted.handleUpdate({
      update_id: 42,
      message: message({ message_id: 42, text: 'question' }),
    });
    await waitFor(1_050);
    const inputOptions = api.sentOptions.find(
      (options, index) => index > 1 && options?.reply_markup !== undefined,
    ) as { reply_markup: { inline_keyboard: Array<Array<{ callback_data?: string }>> } };
    const inputCallback = inputOptions.reply_markup.inline_keyboard[0]?.[0]?.callback_data;
    assert.ok(inputCallback);
    await runtime.respondInput('telegram:123:thread:root', 'question-1', 'sim');
    await inputTurn;

    const restartedAgain = new TelegramAdapter({ api, runtime, allowedUsers: [7], statePath });
    await restartedAgain.handleUpdate({
      update_id: 43,
      callback_query: {
        id: 'stale-input',
        from: { id: 7 },
        data: inputCallback,
        message: { message_id: 102, chat: { id: 123, type: 'private' } },
      },
    });
    assert.deepEqual(api.callbackAnswers.at(-1), {
      id: 'stale-input',
      text: 'Esta pergunta já expirou.',
    });
  } finally {
    await rm(statePath, { force: true });
  }
});

test('renders generic Runtime input choices and accepts a Telegram callback', async () => {
  const api = new FakeApi();
  const runtime = new FakeRuntime();
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });
  const active = adapter.handleUpdate({ update_id: 30, message: message({ text: 'question' }) });
  await waitFor(1_050);

  const inputOptions = api.sentOptions.find((options) => options?.reply_markup !== undefined) as {
    reply_markup: { inline_keyboard: Array<Array<{ callback_data?: string }>> };
  };
  const callbackData = inputOptions.reply_markup.inline_keyboard[0]?.[0]?.callback_data;
  assert.ok(callbackData);

  await adapter.handleUpdate({
    update_id: 31,
    callback_query: {
      id: 'input-callback',
      from: { id: 7 },
      data: callbackData,
      message: { message_id: 101, chat: { id: 123, type: 'private' } },
    },
  });
  await active;
  assert.ok(api.callbackAnswers.some((answer) => answer.id === 'input-callback'));
});

test('accepts typed text as a fallback for a generic Runtime input', async () => {
  const api = new FakeApi();
  const runtime = new FakeRuntime();
  const adapter = new TelegramAdapter({ api, runtime, allowedUsers: [7] });
  const active = adapter.handleUpdate({ update_id: 32, message: message({ text: 'question' }) });
  await waitFor(1_050);

  await adapter.handleUpdate({
    update_id: 33,
    message: message({ message_id: 102, text: 'manual' }),
  });
  await active;
  assert.equal(runtime.requests.length, 1);
});
