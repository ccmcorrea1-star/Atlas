import type { RuntimeEvent } from '../../src/runtime/protocol.js';
import { TelegramChatBudget } from './budget.js';
import type { TelegramApi } from './types.js';

const MAX_TELEGRAM_TEXT_LENGTH = 4096;
const INITIAL_EDIT_INTERVAL_MS = 1_000;
const MAX_EDIT_INTERVAL_MS = 30_000;

type StreamState = {
  content: string;
  previewMessageId?: number;
  lastSentText: string;
  lastEditAt: number;
  editIntervalMs: number;
  floodUntil: number;
  floodStrikes: number;
  completed: boolean;
  pendingEdit: boolean;
  editTimer?: ReturnType<typeof setTimeout>;
  closed: boolean;
};

type StreamItem =
  | { type: 'delta'; messageId: string; delta: string }
  | { type: 'completed'; messageId: string; content: string }
  | { type: 'intermediate'; state: StreamState }
  | { type: 'final'; terminal: RuntimeEvent };

export class TelegramStreamConsumer {
  private readonly states = new Map<string, StreamState>();
  private readonly queue: StreamItem[] = [];
  private processing: Promise<void> | undefined;

  public constructor(
    private readonly api: TelegramApi,
    private readonly budget: TelegramChatBudget,
    private readonly chatId: number | string,
    replyMessageId: number,
  ) {
    this.states.set('turn', this.newState(replyMessageId, '⏳'));
  }

  public pushDelta(messageId: string, delta: string): void {
    if (delta) {
      this.queue.push({ type: 'delta', messageId: messageId || 'turn', delta });
      this.ensureProcessing();
    }
  }

  public pushCompleted(messageId: string, content: string): void {
    this.queue.push({ type: 'completed', messageId: messageId || 'turn', content });
    this.ensureProcessing();
  }

  public async finish(terminal: RuntimeEvent): Promise<void> {
    this.queue.push({ type: 'final', terminal });
    while (this.queue.length > 0 || this.processing !== undefined) {
      await this.ensureProcessing();
    }
  }

  private ensureProcessing(): Promise<void> {
    if (this.processing === undefined) {
      this.processing = this.processQueue().finally(() => {
        this.processing = undefined;
        if (this.queue.length > 0) {
          this.ensureProcessing();
        }
      });
    }
    return this.processing;
  }

  private async processQueue(): Promise<void> {
    while (this.queue.length > 0) {
      const item = this.queue.shift();
      if (item === undefined) {
        continue;
      }
      if (item.type === 'delta') {
        this.applyDelta(item.messageId, item.delta);
      } else if (item.type === 'completed') {
        this.applyCompleted(item.messageId, item.content);
      } else if (item.type === 'intermediate') {
        await this.processIntermediate(item.state);
      } else {
        await this.finalize(item.terminal);
        return;
      }
    }
  }

  private applyDelta(messageId: string, delta: string): void {
    const state = this.state(messageId);
    if (state.closed || state.completed) {
      return;
    }
    state.content += delta;
    state.pendingEdit = true;
    this.scheduleEdit(state);
  }

  private applyCompleted(messageId: string, content: string): void {
    const state = this.state(messageId);
    if (state.closed) {
      return;
    }
    state.content = mergeContent(state.content, content);
    state.completed = true;
    state.pendingEdit = true;
    this.scheduleEdit(state);
  }

  private state(messageId: string): StreamState {
    let state = this.states.get(messageId);
    if (state === undefined) {
      const initial = this.states.get('turn');
      if (initial !== undefined && this.states.size === 1 && messageId !== 'turn') {
        this.states.delete('turn');
        this.states.set(messageId, initial);
        return initial;
      }
      state = this.newState();
      this.states.set(messageId, state);
    }
    return state;
  }

  private newState(previewMessageId?: number, lastSentText = ''): StreamState {
    return {
      content: '',
      ...(previewMessageId === undefined ? {} : { previewMessageId }),
      lastSentText,
      lastEditAt: previewMessageId === undefined ? 0 : Date.now(),
      editIntervalMs: INITIAL_EDIT_INTERVAL_MS,
      floodUntil: 0,
      floodStrikes: 0,
      completed: false,
      pendingEdit: false,
      closed: false,
    };
  }

  private scheduleEdit(state: StreamState, minimumDelayMs = 0): void {
    if (state.closed || state.editTimer !== undefined) {
      return;
    }
    const now = Date.now();
    const delay = Math.max(
      minimumDelayMs,
      state.lastEditAt + state.editIntervalMs - now,
      state.floodUntil - now,
    );
    state.editTimer = setTimeout(
      () => {
        state.editTimer = undefined;
        this.queue.push({ type: 'intermediate', state });
        this.ensureProcessing();
      },
      Math.max(0, delay),
    );
  }

  private async processIntermediate(state: StreamState): Promise<void> {
    if (state.closed || !state.pendingEdit) {
      return;
    }
    state.pendingEdit = false;
    const visible = previewText(state.content);
    if (!visible || visible === state.lastSentText) {
      return;
    }
    const operation =
      state.previewMessageId === undefined
        ? this.budget.tryEnqueue(this.chatId, async () => {
            const sent = await this.api.sendMessage(this.chatId, visible);
            state.previewMessageId = sent.message_id;
          })
        : this.budget.tryEnqueue(this.chatId, () =>
            this.api.editMessageText(this.chatId, state.previewMessageId as number, visible),
          );
    if (operation === undefined) {
      state.pendingEdit = true;
      this.scheduleEdit(state, Math.max(INITIAL_EDIT_INTERVAL_MS, state.editIntervalMs) + 25);
      return;
    }
    try {
      await operation;
      state.lastSentText = visible;
      state.lastEditAt = Date.now();
      state.floodStrikes = 0;
      state.editIntervalMs = Math.max(
        INITIAL_EDIT_INTERVAL_MS,
        Math.floor(state.editIntervalMs * 0.8),
      );
    } catch (error) {
      this.recordFailure(state, error);
      state.pendingEdit = true;
      this.scheduleEdit(state);
    }
  }

  private async finalize(terminal: RuntimeEvent): Promise<void> {
    const data = terminal.data;
    const messageId = stringField(data.message_id);
    const state = messageId
      ? this.state(messageId)
      : this.states.size === 1
        ? (this.states.values().next().value as StreamState)
        : this.state('turn');
    const terminalContent = stringField(data.content) || eventMessage(data);
    if (terminal.type === 'turn.completed' || terminal.type === 'turn.cancelled') {
      state.content = mergeContent(state.content, terminalContent);
    } else if (terminalContent) {
      state.content = terminalContent;
    }

    for (const current of this.states.values()) {
      if (current.editTimer !== undefined) {
        clearTimeout(current.editTimer);
        current.editTimer = undefined;
      }
      current.closed = true;
    }
    for (const current of this.states.values()) {
      await this.finalizeState(current);
    }
  }

  private async finalizeState(state: StreamState): Promise<void> {
    if (!state.content && state.lastSentText === '⏳') {
      return;
    }
    const chunks = splitTelegramText(state.content || ' ');
    const first = chunks[0] ?? ' ';
    if (state.previewMessageId !== undefined) {
      if (state.lastSentText !== first) {
        await this.finalOperation(
          () => this.api.editMessageText(this.chatId, state.previewMessageId as number, first),
          state,
        );
        state.lastSentText = first;
      }
      for (const chunk of chunks.slice(1)) {
        await this.finalOperation(async () => {
          await this.api.sendMessage(this.chatId, chunk);
        }, state);
      }
      return;
    }
    for (const chunk of chunks) {
      await this.finalOperation(async () => {
        const sent = await this.api.sendMessage(this.chatId, chunk);
        state.previewMessageId = sent.message_id;
      }, state);
      state.lastSentText = chunk;
    }
  }

  private async finalOperation<T>(operation: () => Promise<T>, state: StreamState): Promise<T> {
    for (let attempt = 0; attempt < 8; attempt += 1) {
      try {
        return await this.budget.enqueue(this.chatId, operation);
      } catch (error) {
        const retryAfterMs = retryAfterMilliseconds(error);
        if (!isFloodError(error) && retryAfterMs === 0) {
          throw error;
        }
        this.recordFailure(state, error);
        const wait = Math.max(retryAfterMs, state.editIntervalMs);
        await sleep(wait);
      }
    }
    throw new Error('Telegram final delivery exhausted its retries.');
  }

  private recordFailure(state: StreamState, error: unknown): void {
    if (!isFloodError(error)) {
      return;
    }
    state.floodStrikes += 1;
    const retryAfterMs = retryAfterMilliseconds(error);
    state.editIntervalMs =
      retryAfterMs > 0
        ? Math.min(MAX_EDIT_INTERVAL_MS, retryAfterMs)
        : Math.min(MAX_EDIT_INTERVAL_MS, state.editIntervalMs * 2);
    state.floodUntil = Math.max(state.floodUntil, Date.now() + state.editIntervalMs);
    this.budget.block(this.chatId, state.editIntervalMs);
  }
}

function previewText(content: string): string {
  return Array.from(content).slice(0, MAX_TELEGRAM_TEXT_LENGTH).join('');
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

function stringField(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function mergeContent(current: string, snapshot: string): string {
  if (!current || snapshot.startsWith(current)) {
    return snapshot;
  }
  return current;
}

function eventMessage(data: Record<string, unknown>): string {
  return stringField(data.message) || 'O Runtime não concluiu a operação.';
}

function retryAfterMilliseconds(error: unknown): number {
  if (typeof error !== 'object' || error === null) {
    return 0;
  }
  const typedError = error as {
    retry_after?: unknown;
    parameters?: { retry_after?: unknown };
  };
  const value = typedError.retry_after ?? typedError.parameters?.retry_after;
  const numericValue = typeof value === 'number' ? value : Number(value);
  if (Number.isFinite(numericValue) && numericValue > 0) {
    return numericValue * 1_000;
  }
  const match = String((error as Error).message ?? '').match(
    /retry(?:_after| after)\s*[:=]?\s*(\d+(?:\.\d+)?)/iu,
  );
  return match === null ? 0 : Number(match[1]) * 1_000;
}

function isFloodError(error: unknown): boolean {
  if (retryAfterMilliseconds(error) > 0) {
    return true;
  }
  return /flood|too many requests|429/iu.test(String((error as Error)?.message ?? error));
}

function sleep(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
