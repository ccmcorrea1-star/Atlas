import { randomUUID } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { basename, dirname, join } from 'node:path';
import { tmpdir } from 'node:os';
import { pathToFileURL, fileURLToPath } from 'node:url';

import type {
  RuntimeAttachment,
  RuntimeCommandCompletedData,
  RuntimeEvent,
  RuntimeTurnCompletedData,
} from '../../src/runtime/protocol.js';
import { FetchTelegramApi } from './api.js';
import { TelegramChatBudget } from './budget.js';
import { TelegramStreamConsumer } from './stream-consumer.js';
import {
  TelegramAuthorization,
  conversationIdForTelegram,
  groupIsTriggered,
  runtimeCommandFromText,
  stripBotMention,
} from './routing.js';
import type {
  TelegramApi,
  TelegramBot,
  TelegramCallbackQuery,
  TelegramChat,
  TelegramFileRef,
  TelegramMessage,
  TelegramRuntime,
  TelegramUpdate,
} from './types.js';

const MAX_TELEGRAM_ATTACHMENT_BYTES = 20 * 1024 * 1024;
const MAX_TELEGRAM_TEXT_LENGTH = 4096;
const APPROVAL_CALLBACK_PREFIX = 'atlas:approval:';

export type TelegramAdapterOptions = {
  token?: string;
  runtime: TelegramRuntime;
  api?: TelegramApi;
  socketPath?: string;
  authorization?: TelegramAuthorization;
  allowedUsers?: Iterable<number | string>;
  allowedChats?: Iterable<number | string>;
  allowAll?: boolean;
  homeChatId?: number | string;
  downloadDirectory?: string;
  statePath?: string;
};

type ActiveTurn = {
  chatId: number | string;
  stream: TelegramStreamConsumer;
  closed: boolean;
};

type PendingTelegramApproval = {
  token: string;
  conversationId: string;
  approvalId: string;
  chat: TelegramChat;
  userId?: number;
  promptMessageId?: number;
};

type MaterializedAttachments = {
  attachments: RuntimeAttachment[];
  directory?: string;
};

type TelegramState = {
  offset?: number;
  pending: TelegramUpdate[];
  processed_message_ids?: string[];
};

export class TelegramAdapter {
  private readonly api: TelegramApi;
  private readonly authorization: TelegramAuthorization;
  private readonly homeChatId: number | string | undefined;
  private readonly downloadDirectory: string;
  private readonly statePath: string | undefined;
  private readonly activeTurns = new Map<string, ActiveTurn>();
  private readonly conversationQueues = new Map<string, Promise<void>>();
  private readonly pendingApprovals = new Map<string, PendingTelegramApproval>();
  private readonly chatBudgets = new TelegramChatBudget();
  private readonly pendingUpdates = new Map<number, TelegramUpdate>();
  private readonly processingUpdates = new Set<number>();
  private readonly processedMessageIds = new Set<string>();
  private readonly processingMessageIds = new Map<string, Promise<void>>();
  private stateWrite: Promise<void> = Promise.resolve();
  private bot: TelegramBot | undefined;
  private running = false;
  private offset: number | undefined;

  public constructor(private readonly options: TelegramAdapterOptions) {
    this.api = options.api ?? new FetchTelegramApi(options.token ?? '');
    this.authorization =
      options.authorization ??
      new TelegramAuthorization({
        allowedUsers: options.allowedUsers,
        allowedChats: options.allowedChats,
        allowAll: options.allowAll,
      });
    this.homeChatId = options.homeChatId;
    this.downloadDirectory = options.downloadDirectory ?? join(tmpdir(), 'atlas-telegram');
    this.statePath = options.statePath;
  }

  public async start(): Promise<void> {
    this.running = true;
    await this.loadState();
    let retryDelayMs = 1_000;
    while (this.running) {
      try {
        if (this.bot === undefined) {
          this.bot = await this.api.getMe();
        }
        await mkdir(this.downloadDirectory, { recursive: true, mode: 0o700 });
        const updates = await this.api.getUpdates(this.offset, 30);
        retryDelayMs = 1_000;
        for (const update of updates) {
          this.pendingUpdates.set(update.update_id, update);
          this.offset = Math.max(this.offset ?? 0, update.update_id + 1);
        }
        if (updates.length > 0) {
          await this.persistState();
        }
        for (const update of this.pendingUpdates.values()) {
          void this.processPendingUpdate(update);
        }
      } catch (error) {
        console.error(
          `Atlas Telegram polling failed: ${error instanceof Error ? error.message : String(error)}`,
        );
        await new Promise((resolve) => setTimeout(resolve, retryDelayMs));
        retryDelayMs = Math.min(retryDelayMs * 2, 15_000);
      }
    }
  }

  public stop(): void {
    this.running = false;
  }

  public async handleUpdate(update: TelegramUpdate): Promise<void> {
    if (update.callback_query !== undefined) {
      await this.handleCallbackQuery(update.callback_query);
      return;
    }
    const message = update.message;
    if (message === undefined || message.from?.is_bot === true) {
      return;
    }
    const messageKey = telegramMessageKey(message);
    if (this.processedMessageIds.has(messageKey)) {
      return;
    }
    const processing = this.processingMessageIds.get(messageKey);
    if (processing !== undefined) {
      await processing;
      return;
    }

    let resolveProcessing: (() => void) | undefined;
    let rejectProcessing: ((error: unknown) => void) | undefined;
    const processingPromise = new Promise<void>((resolve, reject) => {
      resolveProcessing = resolve;
      rejectProcessing = reject;
    });
    this.processingMessageIds.set(messageKey, processingPromise);
    const processMessage = async (): Promise<void> => {
      try {
        await this.handleMessage(message);
        this.rememberProcessedMessage(messageKey);
        resolveProcessing?.();
      } catch (error) {
        rejectProcessing?.(error);
        throw error;
      } finally {
        if (this.processingMessageIds.get(messageKey) === processingPromise) {
          this.processingMessageIds.delete(messageKey);
        }
      }
    };
    void processMessage().catch(() => undefined);
    await processingPromise;
  }

  private async handleMessage(message: TelegramMessage): Promise<void> {
    if (!this.authorization.allows(message)) {
      return;
    }
    if (this.bot === undefined) {
      this.bot = await this.api.getMe();
    }
    if (!groupIsTriggered(message, this.bot)) {
      return;
    }

    const botUsername = this.bot.username;
    const text = message.text ?? message.caption ?? '';
    const command = runtimeCommandFromText(text);
    const conversationId = conversationIdForTelegram(message.chat.id, message.message_thread_id);
    if (command !== undefined) {
      await this.handleCommand(message, conversationId, command);
      return;
    }

    await this.enqueueConversation(conversationId, async () => {
      const materialized = await this.materializeAttachments(message);
      const { attachments } = materialized;
      const input =
        stripBotMention(text, botUsername) || (attachments.length > 0 ? 'Analise os anexos.' : '');
      if (!input) {
        await this.removeMaterializedDirectory(materialized.directory);
        return;
      }
      try {
        await this.handleTurn(message, conversationId, input, attachments);
      } finally {
        await this.removeMaterializedDirectory(materialized.directory);
      }
    });
  }

  public async notifyHome(text: string): Promise<void> {
    if (this.homeChatId === undefined) {
      return;
    }
    await this.sendText(this.homeChatId, text);
  }

  private async handleCommand(
    message: TelegramMessage,
    conversationId: string,
    command: 'new' | 'status' | 'stop',
  ): Promise<void> {
    const event = await this.options.runtime.command(conversationId, command);
    if (event.type !== 'command.completed') {
      await this.sendText(message.chat.id, eventMessage(event));
      return;
    }
    const data = event.data as unknown as RuntimeCommandCompletedData;
    await this.sendText(message.chat.id, formatCommandResult(data));
  }

  private async handleTurn(
    message: TelegramMessage,
    conversationId: string,
    input: string,
    attachments: RuntimeAttachment[],
  ): Promise<void> {
    const requestId = randomUUID();
    const reply = await this.sendMessage(message.chat.id, '⏳');
    const stream = new TelegramStreamConsumer(
      this.api,
      this.chatBudgets,
      message.chat.id,
      reply.message_id,
    );
    const active: ActiveTurn = {
      chatId: message.chat.id,
      stream,
      closed: false,
    };
    this.activeTurns.set(conversationId, active);

    const onEvent = (event: RuntimeEvent) => {
      if (active.closed) {
        return;
      }
      if (event.type === 'message.delta') {
        stream.pushDelta(stringField(event.data.message_id), stringField(event.data.delta));
      } else if (event.type === 'message.completed') {
        stream.pushCompleted(stringField(event.data.message_id), stringField(event.data.content));
      } else if (event.type === 'approval.requested') {
        void this.sendApprovalRequest(message, conversationId, event).catch((error: unknown) => {
          console.error(
            `Atlas Telegram approval notification failed: ${error instanceof Error ? error.message : String(error)}`,
          );
        });
      }
    };

    try {
      const terminal = await this.options.runtime.runTurn(
        {
          request_id: requestId,
          conversation_id: conversationId,
          input,
          ...(attachments.length === 0 ? {} : { attachments }),
        },
        (event) => void onEvent(event),
      );
      await this.finishTurn(active, terminal);
    } finally {
      this.clearPendingApprovals(conversationId);
      if (this.activeTurns.get(conversationId) === active) {
        this.activeTurns.delete(conversationId);
      }
    }
  }

  private async finishTurn(active: ActiveTurn, terminal: RuntimeEvent): Promise<void> {
    active.closed = true;
    await active.stream.finish(terminal);
    if (terminal.type === 'turn.completed' || terminal.type === 'turn.cancelled') {
      const data = terminal.data as unknown as RuntimeTurnCompletedData;
      await this.deliverAttachments(active.chatId, data.attachments);
    }
  }

  private enqueueConversation(conversationId: string, task: () => Promise<void>): Promise<void> {
    const previous = this.conversationQueues.get(conversationId) ?? Promise.resolve();
    const current = previous.catch(() => undefined).then(task);
    this.conversationQueues.set(conversationId, current);
    void current.finally(() => {
      if (this.conversationQueues.get(conversationId) === current) {
        this.conversationQueues.delete(conversationId);
      }
    });
    return current;
  }

  private rememberProcessedMessage(messageId: string): void {
    this.processedMessageIds.add(messageId);
    while (this.processedMessageIds.size > 10_000) {
      const oldest = this.processedMessageIds.values().next().value as string | undefined;
      if (oldest === undefined) {
        break;
      }
      this.processedMessageIds.delete(oldest);
    }
  }

  private async processPendingUpdate(update: TelegramUpdate): Promise<void> {
    if (this.processingUpdates.has(update.update_id)) {
      return;
    }
    this.processingUpdates.add(update.update_id);
    try {
      await this.handleUpdate(update);
      this.pendingUpdates.delete(update.update_id);
      await this.persistState();
    } catch (error) {
      console.error(
        `Atlas Telegram update failed: ${error instanceof Error ? error.message : String(error)}`,
      );
    } finally {
      this.processingUpdates.delete(update.update_id);
    }
  }

  private async loadState(): Promise<void> {
    if (this.statePath === undefined) {
      return;
    }
    try {
      const parsed = JSON.parse(await readFile(this.statePath, 'utf8')) as Partial<TelegramState>;
      if (
        parsed.offset !== undefined &&
        Number.isSafeInteger(parsed.offset) &&
        parsed.offset >= 0
      ) {
        this.offset = parsed.offset;
      }
      if (Array.isArray(parsed.pending)) {
        for (const update of parsed.pending) {
          if (
            update !== null &&
            typeof update === 'object' &&
            Number.isSafeInteger(update.update_id)
          ) {
            this.pendingUpdates.set(update.update_id, update);
          }
        }
      }
      if (Array.isArray(parsed.processed_message_ids)) {
        for (const messageId of parsed.processed_message_ids) {
          if (typeof messageId === 'string' && messageId) {
            this.processedMessageIds.add(messageId);
          }
        }
      }
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') {
        console.error(
          `Atlas Telegram state could not be loaded: ${error instanceof Error ? error.message : String(error)}`,
        );
      }
    }
  }

  private persistState(): Promise<void> {
    if (this.statePath === undefined) {
      return Promise.resolve();
    }
    this.stateWrite = this.stateWrite.then(async () => {
      await mkdir(dirname(this.statePath as string), { recursive: true, mode: 0o700 });
      const temporaryPath = `${this.statePath}.tmp`;
      const state: TelegramState = {
        ...(this.offset === undefined ? {} : { offset: this.offset }),
        pending: [...this.pendingUpdates.values()],
        processed_message_ids: [...this.processedMessageIds],
      };
      await writeFile(temporaryPath, JSON.stringify(state), { mode: 0o600 });
      await rename(temporaryPath, this.statePath as string);
    });
    return this.stateWrite;
  }

  private async handleCallbackQuery(query: TelegramCallbackQuery): Promise<void> {
    const parsed = parseApprovalCallback(query.data);
    if (parsed === undefined) {
      await this.answerCallback(query.id, 'Ação desconhecida.');
      return;
    }
    const pending = this.pendingApprovals.get(parsed.token);
    if (pending === undefined) {
      await this.answerCallback(query.id, 'Esta aprovação já expirou.');
      return;
    }
    const message = query.message;
    const authorized = this.authorization.allows({
      message_id: message?.message_id ?? pending.promptMessageId ?? 0,
      chat: pending.chat,
      from: query.from,
    });
    if (
      !authorized ||
      (pending.userId !== undefined && pending.userId !== query.from.id) ||
      (message !== undefined && String(message.chat.id) !== String(pending.chat.id))
    ) {
      await this.answerCallback(query.id, 'Você não pode responder esta aprovação.');
      return;
    }

    const event = await this.options.runtime.respondApproval(
      pending.conversationId,
      pending.approvalId,
      parsed.approved,
    );
    if (event.type === 'approval.resolved') {
      this.pendingApprovals.delete(parsed.token);
      if (message !== undefined && this.api.editMessageReplyMarkup !== undefined) {
        await this.api.editMessageReplyMarkup(message.chat.id, message.message_id, {
          inline_keyboard: [],
        });
      }
      await this.answerCallback(query.id, parsed.approved ? 'Aprovado.' : 'Rejeitado.');
      return;
    }
    await this.answerCallback(query.id, eventMessage(event));
  }

  private async sendApprovalRequest(
    message: TelegramMessage,
    conversationId: string,
    event: RuntimeEvent,
  ): Promise<void> {
    const approvalId = stringField(event.data.approval_id);
    if (!approvalId) {
      return;
    }
    const token = randomUUID();
    const pending: PendingTelegramApproval = {
      token,
      conversationId,
      approvalId,
      chat: message.chat,
      userId: message.from?.id,
    };
    this.pendingApprovals.set(token, pending);
    try {
      const prompt = await this.sendMessage(message.chat.id, formatApproval(event), {
        reply_markup: {
          inline_keyboard: [
            [
              { text: 'Aprovar', callback_data: `${APPROVAL_CALLBACK_PREFIX}${token}:yes` },
              { text: 'Rejeitar', callback_data: `${APPROVAL_CALLBACK_PREFIX}${token}:no` },
            ],
          ],
        },
      });
      pending.promptMessageId = prompt.message_id;
    } catch (error) {
      this.pendingApprovals.delete(token);
      throw error;
    }
  }

  private async answerCallback(callbackQueryId: string, text: string): Promise<void> {
    if (this.api.answerCallbackQuery !== undefined) {
      await this.api.answerCallbackQuery(callbackQueryId, text.slice(0, 200));
    }
  }

  private clearPendingApprovals(conversationId: string): void {
    for (const [token, pending] of this.pendingApprovals) {
      if (pending.conversationId === conversationId) {
        this.pendingApprovals.delete(token);
      }
    }
  }

  private async sendText(chatId: number | string, text: string): Promise<void> {
    for (const chunk of splitTelegramText(text || ' ')) {
      await this.sendMessage(chatId, chunk);
    }
  }

  private sendMessage(
    chatId: number | string,
    text: string,
    options: Parameters<TelegramApi['sendMessage']>[2] = {},
  ): ReturnType<TelegramApi['sendMessage']> {
    return this.chatBudgets.enqueue(chatId, () => this.api.sendMessage(chatId, text, options));
  }

  private async removeMaterializedDirectory(directory: string | undefined): Promise<void> {
    if (directory !== undefined) {
      await rm(directory, { recursive: true, force: true });
    }
  }

  private async deliverAttachments(
    chatId: number | string,
    attachments: RuntimeAttachment[] | undefined,
  ): Promise<void> {
    for (const attachment of attachments ?? []) {
      if (!attachment.uri.startsWith('file://')) {
        continue;
      }
      const filePath = fileURLToPath(attachment.uri);
      const caption = attachment.file_name;
      if (attachment.type === 'image' && this.api.sendPhoto !== undefined) {
        await this.api.sendPhoto(chatId, filePath, caption);
      } else if (attachment.type === 'voice' && this.api.sendVoice !== undefined) {
        await this.api.sendVoice(chatId, filePath, caption);
      } else if (attachment.type === 'audio' && this.api.sendAudio !== undefined) {
        await this.api.sendAudio(chatId, filePath, caption);
      } else if (attachment.type === 'document' && this.api.sendDocument !== undefined) {
        await this.api.sendDocument(chatId, filePath, caption);
      }
    }
  }

  private async materializeAttachments(message: TelegramMessage): Promise<MaterializedAttachments> {
    await mkdir(this.downloadDirectory, { recursive: true, mode: 0o700 });
    const refs: Array<{
      type: RuntimeAttachment['type'];
      ref: TelegramFileRef;
      mediaType: string;
    }> = [];
    if (message.voice !== undefined) {
      refs.push({
        type: 'voice',
        ref: message.voice,
        mediaType: message.voice.mime_type ?? 'audio/ogg',
      });
    }
    if (message.audio !== undefined) {
      refs.push({
        type: 'audio',
        ref: message.audio,
        mediaType: message.audio.mime_type ?? 'audio/mpeg',
      });
    }
    const photo = message.photo?.at(-1);
    if (photo !== undefined) {
      refs.push({ type: 'image', ref: photo, mediaType: 'image/jpeg' });
    }
    if (message.document !== undefined) {
      refs.push({
        type: 'document',
        ref: message.document,
        mediaType: message.document.mime_type ?? 'application/octet-stream',
      });
    }

    if (refs.length === 0) {
      return { attachments: [] };
    }

    const directory = await mkdtemp(join(this.downloadDirectory, 'turn-'));
    const attachments: RuntimeAttachment[] = [];
    try {
      for (const [index, item] of refs.entries()) {
        const file = await this.api.getFile(item.ref.file_id);
        const size = file.file_size ?? item.ref.file_size;
        if (size !== undefined && size > MAX_TELEGRAM_ATTACHMENT_BYTES) {
          throw new Error(
            `Telegram attachment exceeds the ${MAX_TELEGRAM_ATTACHMENT_BYTES}-byte limit.`,
          );
        }
        const name =
          item.ref.file_name?.trim() || basename(file.file_path) || `${item.ref.file_id}.bin`;
        const safeName = `${index}-${name.replace(/[^a-zA-Z0-9._-]/gu, '_')}`;
        const destination = join(directory, safeName);
        await this.api.downloadFile(file.file_path, destination, MAX_TELEGRAM_ATTACHMENT_BYTES);
        attachments.push({
          type: item.type,
          uri: pathToFileURL(destination).href,
          media_type: item.mediaType,
          file_name: name,
          ...(size === undefined ? {} : { size_bytes: size }),
          source: { platform: 'telegram', file_id: item.ref.file_id },
        });
      }
    } catch (error) {
      await this.removeMaterializedDirectory(directory);
      throw error;
    }
    return { attachments, directory };
  }
}

function stringField(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function telegramMessageKey(message: TelegramMessage): string {
  return `${String(message.chat.id)}:${message.message_id}`;
}

function splitTelegramText(text: string): string[] {
  const characters = Array.from(text);
  if (characters.length === 0) {
    return [' '];
  }
  const chunks: string[] = [];
  for (let index = 0; index < characters.length; index += MAX_TELEGRAM_TEXT_LENGTH) {
    chunks.push(characters.slice(index, index + MAX_TELEGRAM_TEXT_LENGTH).join(''));
  }
  return chunks;
}

function parseApprovalCallback(
  data: string | undefined,
): { token: string; approved: boolean } | undefined {
  if (data === undefined || !data.startsWith(APPROVAL_CALLBACK_PREFIX)) {
    return undefined;
  }
  const value = data.slice(APPROVAL_CALLBACK_PREFIX.length);
  const separator = value.lastIndexOf(':');
  if (separator <= 0) {
    return undefined;
  }
  const token = value.slice(0, separator);
  const action = value.slice(separator + 1);
  if (action !== 'yes' && action !== 'no') {
    return undefined;
  }
  return { token, approved: action === 'yes' };
}

function eventMessage(event: RuntimeEvent): string {
  return stringField(event.data.message) || 'O Runtime não concluiu a operação.';
}

function formatCommandResult(data: RuntimeCommandCompletedData): string {
  const status = data.session.status === 'running' ? 'em execução' : data.session.status;
  return `${data.message}\nSessão: ${data.session.id}\nEstado: ${status}`;
}

function formatApproval(event: RuntimeEvent): string {
  const tool = stringField(event.data.tool_name);
  const reason = stringField(event.data.reason);
  return `Aprovação necessária para ${tool}: ${reason}`;
}
