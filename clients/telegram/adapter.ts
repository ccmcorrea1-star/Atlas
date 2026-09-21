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
import type { TelegramDeliveryLedger } from './stream-consumer.js';
import {
  TELEGRAM_CAPTION_LIMIT,
  renderTelegramMarkdown,
  splitTelegramText,
  truncateTelegramText,
} from './text.js';
import {
  TelegramAuthorization,
  conversationIdForTelegram,
  groupIsTriggered,
  runtimeCommandFromText,
  stripBotMention,
  TELEGRAM_COMMANDS,
  type TelegramCommandName,
} from './routing.js';
import type {
  TelegramApi,
  TelegramBot,
  TelegramBotCommandScope,
  TelegramCallbackQuery,
  TelegramChat,
  TelegramFileRef,
  TelegramMediaGroupItem,
  TelegramMessage,
  TelegramRuntime,
  TelegramUpdate,
} from './types.js';

const MAX_TELEGRAM_ATTACHMENT_BYTES = 20 * 1024 * 1024;
const APPROVAL_CALLBACK_PREFIX = 'atlas:approval:';
const INPUT_CALLBACK_PREFIX = 'atlas:input:';

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
  threadId?: number;
  requestId: string;
  messageKey: string;
  stream: TelegramStreamConsumer;
  closed: boolean;
};

type PendingTelegramApproval = {
  token: string;
  requestId?: string;
  conversationId: string;
  approvalId: string;
  chat: TelegramChat;
  userId?: number;
  promptMessageId?: number;
};

type PendingTelegramInput = {
  token: string;
  requestId?: string;
  conversationId: string;
  inputId: string;
  chat: TelegramChat;
  userId?: number;
};

type MaterializedAttachments = {
  attachments: RuntimeAttachment[];
  directory?: string;
};

type TelegramAttachmentReference = {
  type: RuntimeAttachment['type'];
  fileId: string;
  mediaType: string;
  fileName?: string;
  sizeBytes?: number;
};

type TelegramState = {
  offset?: number;
  pending: TelegramUpdate[];
  processed_message_ids?: string[];
  turns?: TelegramTurnState[];
  pending_approvals?: PendingTelegramApproval[];
  pending_inputs?: PendingTelegramInput[];
};

type TelegramTurnState = {
  messageKey: string;
  requestId: string;
  conversationId: string;
  chatId: number | string;
  threadId?: number;
  input: string;
  attachment_refs?: TelegramAttachmentReference[];
  attachments?: RuntimeAttachment[];
  replyMessageId?: number;
  status: 'pending' | 'terminal';
  delivery?: TelegramDeliveryLedger;
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
  private readonly pendingInputs = new Map<string, PendingTelegramInput>();
  private readonly chatBudgets = new TelegramChatBudget();
  private readonly pendingUpdates = new Map<number, TelegramUpdate>();
  private readonly processingUpdates = new Set<number>();
  private readonly processedMessageIds = new Set<string>();
  private readonly processingMessageIds = new Map<string, Promise<void>>();
  private readonly turns = new Map<string, TelegramTurnState>();
  private stateWrite: Promise<void> = Promise.resolve();
  private bot: TelegramBot | undefined;
  private running = false;
  private pollingPromise: Promise<void> | undefined;
  private pollController: AbortController | undefined;
  private offset: number | undefined;
  private lastPollAt = 0;
  private lastIdentityRefreshAt = 0;
  private connected = false;
  private readonly stateLoaded: Promise<void>;

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
    this.stateLoaded = this.loadState();
  }

  public async start(): Promise<void> {
    if (this.pollingPromise !== undefined) {
      return this.pollingPromise;
    }
    this.running = true;
    this.pollingPromise = this.runPolling();
    return this.pollingPromise;
  }

  private async runPolling(): Promise<void> {
    await this.stateLoaded;
    let retryDelayMs = 1_000;
    let conflictCount = 0;
    this.pollController = new AbortController();
    try {
      while (this.running) {
        try {
          if (this.bot === undefined) {
            this.bot = await this.api.getMe();
            this.lastIdentityRefreshAt = Date.now();
            await this.registerCommandMenu();
          }
          await mkdir(this.downloadDirectory, { recursive: true, mode: 0o700 });
          const pollStartedAt = Date.now();
          const updates = await withTimeout(
            this.api.getUpdates(this.offset, 30, this.pollController.signal),
            45_000,
            'Telegram polling stalled.',
            this.pollController.signal,
          );
          this.lastPollAt = Date.now();
          this.lastPollAt = Math.max(this.lastPollAt, pollStartedAt);
          this.connected = true;
          conflictCount = 0;
          if (Date.now() - this.lastIdentityRefreshAt > 15 * 60_000) {
            this.bot = await this.api.getMe();
            this.lastIdentityRefreshAt = Date.now();
          }
          retryDelayMs = 1_000;
          for (const update of updates) {
            if (!isValidTelegramUpdate(update)) {
              console.error('Atlas Telegram dropped an invalid update.');
              this.offset = Math.max(this.offset ?? 0, updateIdOf(update) + 1);
              continue;
            }
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
          if (!this.running || isAbortError(error)) {
            break;
          }
          this.connected = false;
          if (isConflictError(error)) {
            conflictCount += 1;
            if (conflictCount >= 3) {
              console.error('Atlas Telegram polling conflict persists; backing off.');
            }
          }
          console.error(`Atlas Telegram polling failed: ${safeErrorMessage(error)}`);
          await sleepWithAbort(retryDelayMs, this.pollController.signal);
          retryDelayMs = Math.min(retryDelayMs * 2, 15_000);
        }
      }
    } finally {
      this.connected = false;
      this.pollController = undefined;
      await this.persistState().catch((error: unknown) => {
        console.error(`Atlas Telegram state flush failed: ${safeErrorMessage(error)}`);
      });
      this.pollingPromise = undefined;
    }
  }

  private async registerCommandMenu(): Promise<void> {
    if (this.api.setMyCommands === undefined) {
      return;
    }
    const commands = TELEGRAM_COMMANDS.map((command) => ({
      command: command.name,
      description: command.description,
    }));
    const scopes: TelegramBotCommandScope[] = [
      { type: 'default' },
      { type: 'all_private_chats' },
      { type: 'all_group_chats' },
    ];
    for (const scope of scopes) {
      await this.api.setMyCommands(commands, scope);
      if (this.api.getMyCommands !== undefined) {
        const registered = await this.api.getMyCommands(scope);
        if (!sameCommandMenu(commands, registered)) {
          throw new Error(`Telegram command menu verification failed for ${scope.type}.`);
        }
      }
    }
  }

  public async stop(): Promise<void> {
    this.running = false;
    this.pollController?.abort();
    await this.pollingPromise;
    await this.persistState().catch((error: unknown) => {
      console.error(`Atlas Telegram state flush failed: ${safeErrorMessage(error)}`);
    });
  }

  public isConnected(): boolean {
    return this.connected;
  }

  public async handleUpdate(update: TelegramUpdate): Promise<void> {
    await this.stateLoaded;
    if (update.callback_query !== undefined) {
      await this.handleCallbackQuery(update.callback_query);
      return;
    }
    const message = update.message;
    if (message === undefined || message.from?.is_bot === true) {
      return;
    }
    const messageKey = telegramMessageKey(message);
    const storedTurn = this.turns.get(messageKey);
    if (storedTurn?.status === 'terminal') {
      this.rememberProcessedMessage(messageKey);
      return;
    }
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
    if (
      command === undefined &&
      (await this.respondToPendingInput(message, conversationId, text))
    ) {
      return;
    }
    if (command !== undefined) {
      await this.handleCommand(message, conversationId, command);
      return;
    }

    await this.enqueueConversation(conversationId, async () => {
      const existing = this.turns.get(telegramMessageKey(message));
      const materialized: MaterializedAttachments =
        existing?.replyMessageId === undefined
          ? await this.materializeAttachments(message)
          : existing.attachment_refs !== undefined
            ? await this.materializeAttachmentReferences(existing.attachment_refs)
            : { attachments: existing.attachments ?? [] };
      const attachments =
        materialized.attachments.length > 0
          ? materialized.attachments
          : (existing?.attachments ?? []);
      const input =
        existing?.input ??
        (stripBotMention(text, botUsername) ||
          (attachments.length > 0 ? 'Analise os anexos.' : ''));
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
    command: TelegramCommandName,
  ): Promise<void> {
    if (command === 'help') {
      await this.sendText(message.chat.id, formatHelp(), replyOptions(message));
      return;
    }
    const event = await this.options.runtime.command(conversationId, command);
    if (event.type !== 'command.completed') {
      await this.sendText(message.chat.id, eventMessage(event), replyOptions(message));
      return;
    }
    const data = event.data as unknown as RuntimeCommandCompletedData;
    await this.sendText(message.chat.id, formatCommandResult(data), replyOptions(message));
  }

  private async handleTurn(
    message: TelegramMessage,
    conversationId: string,
    input: string,
    attachments: RuntimeAttachment[],
  ): Promise<void> {
    const messageKey = telegramMessageKey(message);
    let turn = this.turns.get(messageKey);
    if (turn === undefined) {
      turn = {
        messageKey,
        requestId: randomUUID(),
        conversationId,
        chatId: message.chat.id,
        ...(message.message_thread_id === undefined ? {} : { threadId: message.message_thread_id }),
        input,
        ...(attachments.length === 0 ? {} : { attachment_refs: attachmentReferences(attachments) }),
        status: 'pending',
      };
      this.turns.set(messageKey, turn);
      await this.persistState();
    }
    const reply =
      turn.replyMessageId === undefined
        ? await this.sendMessage(message.chat.id, '⏳', {
            message_thread_id: message.message_thread_id,
            reply_to_message_id: message.message_id,
            disable_notification: true,
          })
        : { message_id: turn.replyMessageId, chat: message.chat };
    turn.replyMessageId = reply.message_id;
    turn.delivery ??= { confirmedChunks: 0 };
    await this.persistState();
    const stream = new TelegramStreamConsumer(
      this.api,
      this.chatBudgets,
      message.chat.id,
      reply.message_id,
      {
        sendOptions: {
          ...(message.message_thread_id === undefined
            ? {}
            : { message_thread_id: message.message_thread_id }),
          reply_to_message_id: message.message_id,
        },
        sendMessage: (text, options) => this.sendMessageRaw(message.chat.id, text, options),
        editMessage: (messageId, text) => this.editMessageRaw(message.chat.id, messageId, text),
        delivery: turn.delivery,
        onDelivery: async (delivery) => {
          turn.delivery = delivery;
          await this.persistState();
        },
      },
    );
    const active: ActiveTurn = {
      chatId: message.chat.id,
      ...(message.message_thread_id === undefined ? {} : { threadId: message.message_thread_id }),
      requestId: turn.requestId,
      messageKey,
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
      } else if (event.type === 'input.requested') {
        void this.sendInputRequest(message, conversationId, event).catch((error: unknown) => {
          console.error(`Atlas Telegram input notification failed: ${safeErrorMessage(error)}`);
        });
      } else if (event.type === 'tool.started' || event.type === 'execution.started') {
        stream.pushStatus('⚙️ Executando...');
      } else if (event.type === 'reasoning-start') {
        stream.pushStatus('💭 Pensando...');
      }
    };

    try {
      void this.sendChatAction(message.chat.id).catch(() => undefined);
      const terminal = await this.options.runtime.runTurn(
        {
          request_id: turn.requestId,
          conversation_id: conversationId,
          input,
          ...(attachments.length === 0 ? {} : { attachments }),
        },
        (event) => void onEvent(event),
      );
      await this.finishTurn(active, terminal);
      turn.status = 'terminal';
      await this.persistState();
    } finally {
      await this.clearPendingApprovals(conversationId);
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
      await this.deliverAttachments(active.chatId, data.attachments, active.threadId);
    }
  }

  private enqueueConversation(conversationId: string, task: () => Promise<void>): Promise<void> {
    const previous = this.conversationQueues.get(conversationId) ?? Promise.resolve();
    const current = previous.catch(() => undefined).then(task);
    this.conversationQueues.set(conversationId, current);
    void current.then(
      () => {
        if (this.conversationQueues.get(conversationId) === current) {
          this.conversationQueues.delete(conversationId);
        }
      },
      () => {
        if (this.conversationQueues.get(conversationId) === current) {
          this.conversationQueues.delete(conversationId);
        }
      },
    );
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
      if (isPermanentUpdateError(error)) {
        this.pendingUpdates.delete(update.update_id);
        if (update.message !== undefined && this.authorization.allows(update.message)) {
          await this.sendText(
            update.message.chat.id,
            `Não foi possível processar esta mensagem: ${safeErrorMessage(error)}`,
          ).catch(() => undefined);
        }
        this.rememberProcessedMessage(
          update.message === undefined
            ? `update:${update.update_id}`
            : telegramMessageKey(update.message),
        );
        await this.persistState();
      } else {
        console.error(`Atlas Telegram update failed: ${safeErrorMessage(error)}`);
      }
    } finally {
      this.processingUpdates.delete(update.update_id);
    }
  }

  private async loadState(): Promise<void> {
    if (this.statePath === undefined) {
      return;
    }
    try {
      let stateChanged = false;
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
      if (Array.isArray(parsed.turns)) {
        for (const turn of parsed.turns) {
          if (
            turn !== null &&
            typeof turn === 'object' &&
            typeof turn.messageKey === 'string' &&
            typeof turn.requestId === 'string' &&
            typeof turn.conversationId === 'string' &&
            (turn.status === 'pending' || turn.status === 'terminal')
          ) {
            const legacyAttachments = turn.attachments;
            if (turn.attachment_refs === undefined && legacyAttachments !== undefined) {
              const references = attachmentReferences(legacyAttachments);
              if (references.length === legacyAttachments.length) {
                turn.attachment_refs = references;
              }
              delete turn.attachments;
              stateChanged = true;
            }
            this.turns.set(turn.messageKey, turn);
          }
        }
      }
      if (Array.isArray(parsed.pending_approvals)) {
        for (const approval of parsed.pending_approvals) {
          if (
            approval !== null &&
            typeof approval === 'object' &&
            typeof approval.token === 'string' &&
            typeof approval.conversationId === 'string' &&
            typeof approval.approvalId === 'string' &&
            approval.chat !== undefined
          ) {
            this.pendingApprovals.set(approval.token, approval);
          }
        }
      }
      if (Array.isArray(parsed.pending_inputs)) {
        for (const input of parsed.pending_inputs) {
          if (
            input !== null &&
            typeof input === 'object' &&
            typeof input.token === 'string' &&
            typeof input.conversationId === 'string' &&
            typeof input.inputId === 'string' &&
            input.chat !== undefined
          ) {
            this.pendingInputs.set(input.token, input);
          }
        }
      }
      const terminalRequests = new Set(
        [...this.turns.values()]
          .filter((turn) => turn.status === 'terminal')
          .map((turn) => turn.requestId),
      );
      for (const [token, pending] of this.pendingApprovals) {
        if (
          (pending.requestId !== undefined && terminalRequests.has(pending.requestId)) ||
          (pending.requestId === undefined &&
            [...this.turns.values()].some(
              (turn) =>
                turn.status === 'terminal' && turn.conversationId === pending.conversationId,
            ))
        ) {
          this.pendingApprovals.delete(token);
          stateChanged = true;
        }
      }
      for (const [token, pending] of this.pendingInputs) {
        if (
          (pending.requestId !== undefined && terminalRequests.has(pending.requestId)) ||
          (pending.requestId === undefined &&
            [...this.turns.values()].some(
              (turn) =>
                turn.status === 'terminal' && turn.conversationId === pending.conversationId,
            ))
        ) {
          this.pendingInputs.delete(token);
          stateChanged = true;
        }
      }
      if (stateChanged) {
        await this.persistState();
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
    const write = this.stateWrite
      .catch(() => undefined)
      .then(async () => {
        await mkdir(dirname(this.statePath as string), { recursive: true, mode: 0o700 });
        const temporaryPath = `${this.statePath}.tmp`;
        const state: TelegramState = {
          ...(this.offset === undefined ? {} : { offset: this.offset }),
          pending: [...this.pendingUpdates.values()],
          processed_message_ids: [...this.processedMessageIds],
          turns: [...this.turns.values()],
          pending_approvals: [...this.pendingApprovals.values()],
          pending_inputs: [...this.pendingInputs.values()],
        };
        await writeFile(temporaryPath, JSON.stringify(state), { mode: 0o600 });
        await rename(temporaryPath, this.statePath as string);
      });
    this.stateWrite = write.catch(() => undefined);
    return write;
  }

  private async handleCallbackQuery(query: TelegramCallbackQuery): Promise<void> {
    const input = parseInputCallback(query.data);
    if (input !== undefined) {
      await this.handleInputCallback(query, input);
      return;
    }
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
      await this.persistState();
      if (message !== undefined && this.api.editMessageReplyMarkup !== undefined) {
        const editMarkup = this.api.editMessageReplyMarkup.bind(this.api);
        await this.chatBudgets.enqueue(message.chat.id, () =>
          editMarkup(message.chat.id, message.message_id, { inline_keyboard: [] }),
        );
      }
      await this.answerCallback(query.id, parsed.approved ? 'Aprovado.' : 'Rejeitado.');
      return;
    }
    await this.answerCallback(query.id, eventMessage(event));
  }

  private async respondToPendingInput(
    message: TelegramMessage,
    conversationId: string,
    value: string,
  ): Promise<boolean> {
    if (!value.trim() || this.options.runtime.respondInput === undefined) {
      return false;
    }
    const pending = [...this.pendingInputs.values()].find(
      (item) =>
        item.conversationId === conversationId && String(item.chat.id) === String(message.chat.id),
    );
    if (
      pending === undefined ||
      (pending.userId !== undefined && pending.userId !== message.from?.id)
    ) {
      return false;
    }
    const event = await this.options.runtime.respondInput(
      conversationId,
      pending.inputId,
      value.trim(),
    );
    if (event.type === 'input.resolved' || event.type === 'turn.completed') {
      this.pendingInputs.delete(pending.token);
      await this.persistState();
      const response = stringField(event.data.message);
      if (response) {
        await this.sendText(message.chat.id, response, replyOptions(message));
      }
    }
    return true;
  }

  private async handleInputCallback(
    query: TelegramCallbackQuery,
    input: { token: string; value: string },
  ): Promise<void> {
    const pending = this.pendingInputs.get(input.token);
    if (
      pending === undefined ||
      (pending.userId !== undefined && pending.userId !== query.from.id) ||
      String(query.message?.chat.id ?? pending.chat.id) !== String(pending.chat.id) ||
      this.options.runtime.respondInput === undefined
    ) {
      await this.answerCallback(query.id, 'Esta pergunta já expirou.');
      return;
    }
    const event = await this.options.runtime.respondInput(
      pending.conversationId,
      pending.inputId,
      input.value,
    );
    if (event.type === 'input.resolved' || event.type === 'turn.completed') {
      this.pendingInputs.delete(input.token);
      await this.persistState();
      if (query.message !== undefined && this.api.editMessageReplyMarkup !== undefined) {
        const editMarkup = this.api.editMessageReplyMarkup.bind(this.api);
        await this.chatBudgets.enqueue(query.message.chat.id, () =>
          editMarkup(query.message!.chat.id, query.message!.message_id, { inline_keyboard: [] }),
        );
      }
      await this.answerCallback(query.id, 'Resposta registrada.');
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
    const existing = [...this.pendingApprovals.values()].find(
      (pending) => pending.conversationId === conversationId && pending.approvalId === approvalId,
    );
    if (existing !== undefined) {
      return;
    }
    const token = randomUUID();
    const pending: PendingTelegramApproval = {
      token,
      ...(event.request_id === undefined ? {} : { requestId: event.request_id }),
      conversationId,
      approvalId,
      chat: message.chat,
      userId: message.from?.id,
    };
    this.pendingApprovals.set(token, pending);
    try {
      const prompt = await this.sendMessage(message.chat.id, formatApproval(event), {
        ...replyOptions(message),
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
      await this.persistState();
    } catch (error) {
      this.pendingApprovals.delete(token);
      await this.persistState();
      throw error;
    }
  }

  private async sendInputRequest(
    message: TelegramMessage,
    conversationId: string,
    event: RuntimeEvent,
  ): Promise<void> {
    const inputId = stringField(event.data.input_id);
    const prompt = stringField(event.data.prompt);
    if (!inputId || !prompt) {
      return;
    }
    const existing = [...this.pendingInputs.values()].find(
      (pending) => pending.conversationId === conversationId && pending.inputId === inputId,
    );
    if (existing !== undefined) {
      return;
    }
    const choices = Array.isArray(event.data.choices)
      ? event.data.choices.filter((choice): choice is string => typeof choice === 'string')
      : [];
    const token = randomUUID();
    this.pendingInputs.set(token, {
      token,
      ...(event.request_id === undefined ? {} : { requestId: event.request_id }),
      conversationId,
      inputId,
      chat: message.chat,
      userId: message.from?.id,
    });
    try {
      await this.sendMessage(message.chat.id, prompt, {
        ...replyOptions(message),
        ...(choices.length === 0
          ? {}
          : {
              reply_markup: {
                inline_keyboard: choices.map((choice) => [
                  {
                    text: choice,
                    callback_data: `${INPUT_CALLBACK_PREFIX}${token}:${encodeURIComponent(choice)}`,
                  },
                ]),
              },
            }),
      });
      await this.persistState();
    } catch (error) {
      this.pendingInputs.delete(token);
      await this.persistState();
      throw error;
    }
  }

  private async answerCallback(callbackQueryId: string, text: string): Promise<void> {
    if (this.api.answerCallbackQuery !== undefined) {
      await this.api.answerCallbackQuery(callbackQueryId, text.slice(0, 200));
    }
  }

  private async clearPendingApprovals(conversationId: string): Promise<void> {
    let removed = false;
    for (const [token, pending] of this.pendingApprovals) {
      if (pending.conversationId === conversationId) {
        this.pendingApprovals.delete(token);
        removed = true;
      }
    }
    for (const [token, pending] of this.pendingInputs) {
      if (pending.conversationId === conversationId) {
        this.pendingInputs.delete(token);
        removed = true;
      }
    }
    if (removed) {
      await this.persistState();
    }
  }

  private async sendText(
    chatId: number | string,
    text: string,
    options: Parameters<TelegramApi['sendMessage']>[2] = {},
  ): Promise<void> {
    const rendered = renderTelegramMarkdown(text || ' ');
    for (const chunk of splitTelegramText(rendered)) {
      await this.sendMessageRaw(chatId, chunk, options);
    }
  }

  private sendMessage(
    chatId: number | string,
    text: string,
    options: Parameters<TelegramApi['sendMessage']>[2] = {},
  ): ReturnType<TelegramApi['sendMessage']> {
    return this.sendMessageRaw(chatId, renderTelegramMarkdown(text), options);
  }

  private async sendMessageRaw(
    chatId: number | string,
    text: string,
    options: Parameters<TelegramApi['sendMessage']>[2] = {},
  ): ReturnType<TelegramApi['sendMessage']> {
    const formattedOptions = {
      ...options,
      parse_mode: options.parse_mode ?? ('MarkdownV2' as const),
    };
    try {
      return await this.chatBudgets.enqueue(chatId, () =>
        this.api.sendMessage(chatId, text, formattedOptions),
      );
    } catch (error) {
      if (isMarkdownError(error)) {
        const plainOptions = { ...formattedOptions, parse_mode: undefined };
        return this.chatBudgets.enqueue(chatId, () =>
          this.api.sendMessage(chatId, text, plainOptions),
        );
      }
      if (!hasInvalidThreadError(error) || options.message_thread_id === undefined) {
        throw error;
      }
      const fallback = { ...formattedOptions };
      delete fallback.message_thread_id;
      try {
        return await this.chatBudgets.enqueue(chatId, () =>
          this.api.sendMessage(chatId, text, fallback),
        );
      } catch (fallbackError) {
        if (!isMarkdownError(fallbackError)) {
          throw fallbackError;
        }
        const plainFallback = { ...fallback, parse_mode: undefined };
        return this.chatBudgets.enqueue(chatId, () =>
          this.api.sendMessage(chatId, text, plainFallback),
        );
      }
    }
  }

  private editMessage(chatId: number | string, messageId: number, text: string): Promise<void> {
    return this.editMessageRaw(chatId, messageId, renderTelegramMarkdown(text));
  }

  private editMessageRaw(chatId: number | string, messageId: number, text: string): Promise<void> {
    return this.chatBudgets
      .enqueue(chatId, () =>
        this.api.editMessageText(chatId, messageId, text, { parse_mode: 'MarkdownV2' }),
      )
      .catch((error: unknown) => {
        if (!isMarkdownError(error)) {
          throw error;
        }
        return this.chatBudgets.enqueue(chatId, () =>
          this.api.editMessageText(chatId, messageId, text),
        );
      });
  }

  private async sendChatAction(chatId: number | string): Promise<void> {
    if (this.api.sendChatAction === undefined) {
      return;
    }
    const sendChatAction = this.api.sendChatAction.bind(this.api);
    await this.chatBudgets.enqueue(chatId, () => sendChatAction(chatId, 'typing'));
  }

  private async removeMaterializedDirectory(directory: string | undefined): Promise<void> {
    if (directory !== undefined) {
      await rm(directory, { recursive: true, force: true });
    }
  }

  private async deliverAttachments(
    chatId: number | string,
    attachments: RuntimeAttachment[] | undefined,
    threadId?: number,
  ): Promise<void> {
    const items = attachments ?? [];
    let index = 0;
    while (index < items.length) {
      const album: TelegramMediaGroupItem[] = [];
      let albumEnd = index;
      while (album.length < 10 && albumEnd < items.length) {
        const item = items[albumEnd];
        if (item === undefined || !isAlbumAttachment(item) || !item.uri.startsWith('file://')) {
          break;
        }
        album.push({
          type: item.type === 'image' ? 'photo' : item.type,
          filePath: fileURLToPath(item.uri),
          ...(item.file_name === undefined
            ? {}
            : { caption: truncateTelegramText(item.file_name, TELEGRAM_CAPTION_LIMIT) }),
        });
        albumEnd += 1;
      }
      if (album.length >= 2 && this.api.sendMediaGroup !== undefined) {
        const sendMediaGroup = this.api.sendMediaGroup.bind(this.api);
        await this.sendMediaWithThreadFallback(chatId, threadId, (options) =>
          this.chatBudgets.enqueue(chatId, () => sendMediaGroup(chatId, album, options)),
        );
        index = albumEnd;
        continue;
      }

      const attachment = items[index];
      index += 1;
      if (attachment === undefined || !attachment.uri.startsWith('file://')) {
        continue;
      }
      const filePath = fileURLToPath(attachment.uri);
      const caption =
        attachment.file_name === undefined
          ? undefined
          : truncateTelegramText(attachment.file_name, TELEGRAM_CAPTION_LIMIT);
      if (attachment.type === 'image' && this.api.sendPhoto !== undefined) {
        const sendPhoto = this.api.sendPhoto.bind(this.api);
        await this.sendMediaWithThreadFallback(chatId, threadId, (options) =>
          this.chatBudgets.enqueue(chatId, () => sendPhoto(chatId, filePath, caption, options)),
        );
      } else if (attachment.type === 'video' && this.api.sendVideo !== undefined) {
        const sendVideo = this.api.sendVideo.bind(this.api);
        await this.sendMediaWithThreadFallback(chatId, threadId, (options) =>
          this.chatBudgets.enqueue(chatId, () => sendVideo(chatId, filePath, caption, options)),
        );
      } else if (attachment.type === 'animation' && this.api.sendAnimation !== undefined) {
        const sendAnimation = this.api.sendAnimation.bind(this.api);
        await this.sendMediaWithThreadFallback(chatId, threadId, (options) =>
          this.chatBudgets.enqueue(chatId, () => sendAnimation(chatId, filePath, caption, options)),
        );
      } else if (attachment.type === 'voice' && this.api.sendVoice !== undefined) {
        const sendVoice = this.api.sendVoice.bind(this.api);
        await this.sendMediaWithThreadFallback(chatId, threadId, (options) =>
          this.chatBudgets.enqueue(chatId, () => sendVoice(chatId, filePath, caption, options)),
        );
      } else if (attachment.type === 'audio' && this.api.sendAudio !== undefined) {
        const sendAudio = this.api.sendAudio.bind(this.api);
        await this.sendMediaWithThreadFallback(chatId, threadId, (options) =>
          this.chatBudgets.enqueue(chatId, () => sendAudio(chatId, filePath, caption, options)),
        );
      } else if (attachment.type === 'document' && this.api.sendDocument !== undefined) {
        const sendDocument = this.api.sendDocument.bind(this.api);
        await this.sendMediaWithThreadFallback(chatId, threadId, (options) =>
          this.chatBudgets.enqueue(chatId, () => sendDocument(chatId, filePath, caption, options)),
        );
      }
    }
  }

  private async sendMediaWithThreadFallback(
    chatId: number | string,
    threadId: number | undefined,
    send: (options: { message_thread_id?: number }) => Promise<void>,
  ): Promise<void> {
    const options = threadId === undefined ? {} : { message_thread_id: threadId };
    try {
      await send(options);
    } catch (error) {
      if (!hasInvalidThreadError(error) || threadId === undefined) {
        throw error;
      }
      await send({});
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
    if (message.video !== undefined) {
      refs.push({
        type: 'video',
        ref: message.video,
        mediaType: message.video.mime_type ?? 'video/mp4',
      });
    }
    if (message.animation !== undefined) {
      refs.push({
        type: 'animation',
        ref: message.animation,
        mediaType: message.animation.mime_type ?? 'video/mp4',
      });
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

  private async materializeAttachmentReferences(
    references: TelegramAttachmentReference[],
  ): Promise<MaterializedAttachments> {
    if (references.length === 0) {
      return { attachments: [] };
    }
    const directory = await mkdtemp(join(this.downloadDirectory, 'turn-'));
    const attachments: RuntimeAttachment[] = [];
    try {
      for (const [index, reference] of references.entries()) {
        const file = await this.api.getFile(reference.fileId);
        const size = file.file_size ?? reference.sizeBytes;
        if (size !== undefined && size > MAX_TELEGRAM_ATTACHMENT_BYTES) {
          throw new Error(
            `Telegram attachment exceeds the ${MAX_TELEGRAM_ATTACHMENT_BYTES}-byte limit.`,
          );
        }
        const name =
          reference.fileName?.trim() || basename(file.file_path) || `${reference.fileId}.bin`;
        const safeName = `${index}-${name.replace(/[^a-zA-Z0-9._-]/gu, '_')}`;
        const destination = join(directory, safeName);
        await this.api.downloadFile(file.file_path, destination, MAX_TELEGRAM_ATTACHMENT_BYTES);
        attachments.push({
          type: reference.type,
          uri: pathToFileURL(destination).href,
          media_type: reference.mediaType,
          file_name: name,
          ...(size === undefined ? {} : { size_bytes: size }),
          source: { platform: 'telegram', file_id: reference.fileId },
        });
      }
    } catch (error) {
      await this.removeMaterializedDirectory(directory);
      throw error;
    }
    return { attachments, directory };
  }
}

function attachmentReferences(attachments: RuntimeAttachment[]): TelegramAttachmentReference[] {
  return attachments.flatMap((attachment) => {
    const fileId =
      attachment.source?.platform === 'telegram' ? attachment.source.file_id : undefined;
    return fileId === undefined
      ? []
      : [
          {
            type: attachment.type,
            fileId,
            mediaType: attachment.media_type,
            ...(attachment.file_name === undefined ? {} : { fileName: attachment.file_name }),
            ...(attachment.size_bytes === undefined ? {} : { sizeBytes: attachment.size_bytes }),
          },
        ];
  });
}

function stringField(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function telegramMessageKey(message: TelegramMessage): string {
  return `${String(message.chat.id)}:${message.message_id}`;
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

function parseInputCallback(
  data: string | undefined,
): { token: string; value: string } | undefined {
  if (data === undefined || !data.startsWith(INPUT_CALLBACK_PREFIX)) {
    return undefined;
  }
  const value = data.slice(INPUT_CALLBACK_PREFIX.length);
  const separator = value.indexOf(':');
  if (separator <= 0) {
    return undefined;
  }
  const token = value.slice(0, separator);
  try {
    return { token, value: decodeURIComponent(value.slice(separator + 1)) };
  } catch {
    return undefined;
  }
}

function eventMessage(event: RuntimeEvent): string {
  return stringField(event.data.message) || 'O Runtime não concluiu a operação.';
}

function isAlbumAttachment(
  attachment: RuntimeAttachment,
): attachment is RuntimeAttachment & { type: 'image' | 'video' | 'document' } {
  return (
    attachment.type === 'image' || attachment.type === 'video' || attachment.type === 'document'
  );
}

function sameCommandMenu(
  expected: Array<{ command: string; description: string }>,
  actual: Array<{ command: string; description: string }>,
): boolean {
  return (
    expected.length === actual.length &&
    expected.every(
      (command, index) =>
        command.command === actual[index]?.command &&
        command.description === actual[index]?.description,
    )
  );
}

function formatHelp(): string {
  return [
    '*Comandos disponíveis*',
    ...TELEGRAM_COMMANDS.map((command) => `/${command.name} — ${command.description}`),
  ].join('\n');
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

function replyOptions(message: TelegramMessage): {
  message_thread_id?: number;
  reply_to_message_id?: number;
} {
  return {
    ...(message.message_thread_id === undefined
      ? {}
      : { message_thread_id: message.message_thread_id }),
    reply_to_message_id: message.message_id,
  };
}

function hasInvalidThreadError(error: unknown): boolean {
  return /message thread|thread not found|not a forum|topic/iu.test(
    error instanceof Error ? error.message : String(error),
  );
}

function isMarkdownError(error: unknown): boolean {
  return /parse entities|can't parse|markdown/iu.test(safeErrorMessage(error));
}

function isValidTelegramUpdate(value: unknown): value is TelegramUpdate {
  if (value === null || typeof value !== 'object') {
    return false;
  }
  const update = value as Record<string, unknown>;
  if (!Number.isSafeInteger(update.update_id)) {
    return false;
  }
  if (update.message !== undefined && isValidTelegramMessage(update.message)) {
    return true;
  }
  return isValidTelegramCallback(update.callback_query);
}

function isValidTelegramMessage(value: unknown): value is TelegramMessage {
  if (value === null || typeof value !== 'object') {
    return false;
  }
  const message = value as Partial<TelegramMessage>;
  return (
    Number.isSafeInteger(message.message_id) &&
    message.chat !== undefined &&
    typeof message.chat === 'object' &&
    message.chat !== null &&
    (typeof message.chat.id === 'string' || typeof message.chat.id === 'number') &&
    typeof message.chat.type === 'string'
  );
}

function isValidTelegramCallback(value: unknown): value is TelegramCallbackQuery {
  if (value === null || typeof value !== 'object') {
    return false;
  }
  const query = value as Partial<TelegramCallbackQuery>;
  return (
    typeof query.id === 'string' && query.from !== undefined && Number.isSafeInteger(query.from.id)
  );
}

function updateIdOf(value: unknown): number {
  if (value !== null && typeof value === 'object') {
    const updateId = (value as { update_id?: unknown }).update_id;
    return Number.isSafeInteger(updateId) ? (updateId as number) : -1;
  }
  return -1;
}

function isPermanentUpdateError(error: unknown): boolean {
  return /attachment exceeds|invalid update|unsupported attachment|malformed|not a valid/iu.test(
    safeErrorMessage(error),
  );
}

function isConflictError(error: unknown): boolean {
  return /409|conflict|terminated by other getupdates/iu.test(safeErrorMessage(error));
}

function isAbortError(error: unknown): boolean {
  return error instanceof Error && (error.name === 'AbortError' || error.name === 'TimeoutError');
}

function safeErrorMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  return message.replace(/[\r\n\t]+/gu, ' ').slice(0, 300);
}

async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message: string,
  signal?: AbortSignal,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let onAbort: (() => void) | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<T>((_, reject) => {
        timer = setTimeout(() => reject(new Error(message)), timeoutMs);
      }),
      ...(signal === undefined
        ? []
        : [
            new Promise<T>((_, reject) => {
              onAbort = () => reject(signal.reason ?? new Error('Polling stopped.'));
              signal.addEventListener('abort', onAbort, { once: true });
            }),
          ]),
    ]);
  } finally {
    if (timer !== undefined) {
      clearTimeout(timer);
    }
    if (signal !== undefined && onAbort !== undefined) {
      signal.removeEventListener('abort', onAbort);
    }
  }
}

function sleepWithAbort(milliseconds: number, signal: AbortSignal): Promise<void> {
  if (signal.aborted) {
    return Promise.reject(signal.reason ?? new Error('Polling stopped.'));
  }
  return new Promise((resolve, reject) => {
    const timer = setTimeout(resolve, milliseconds);
    signal.addEventListener(
      'abort',
      () => {
        clearTimeout(timer);
        reject(signal.reason ?? new Error('Polling stopped.'));
      },
      { once: true },
    );
  });
}
