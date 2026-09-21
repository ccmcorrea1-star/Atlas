type QueuedOperation<T> = {
  operation: () => Promise<T>;
  resolve: (value: T) => void;
  reject: (error: unknown) => void;
};

type ChatBudgetState = {
  nextSlotAt: number;
  blockedUntil: number;
  running: boolean;
  queue: Array<QueuedOperation<unknown>>;
  draining: boolean;
};

const TELEGRAM_CHAT_SLOT_MS = 1_000;

export class TelegramChatBudget {
  private readonly chats = new Map<string, ChatBudgetState>();

  public enqueue<T>(chatId: number | string, operation: () => Promise<T>): Promise<T> {
    const state = this.state(chatId);
    return new Promise<T>((resolve, reject) => {
      state.queue.push({
        operation: async () => operation(),
        resolve: (value) => resolve(value as T),
        reject,
      });
      this.drain(chatId, state);
    });
  }

  public tryEnqueue<T>(
    chatId: number | string,
    operation: () => Promise<T>,
  ): Promise<T> | undefined {
    const state = this.state(chatId);
    if (state.running || state.queue.length > 0 || this.waitUntil(state) > Date.now()) {
      return undefined;
    }
    return this.enqueue(chatId, operation);
  }

  public block(chatId: number | string, milliseconds: number): void {
    if (milliseconds <= 0) {
      return;
    }
    const state = this.state(chatId);
    state.blockedUntil = Math.max(state.blockedUntil, Date.now() + milliseconds);
  }

  private state(chatId: number | string): ChatBudgetState {
    const key = String(chatId);
    let state = this.chats.get(key);
    if (state === undefined) {
      state = {
        nextSlotAt: 0,
        blockedUntil: 0,
        running: false,
        queue: [],
        draining: false,
      };
      this.chats.set(key, state);
    }
    return state;
  }

  private waitUntil(state: ChatBudgetState): number {
    return Math.max(state.nextSlotAt, state.blockedUntil);
  }

  private drain(chatId: number | string, state: ChatBudgetState): void {
    if (state.draining) {
      return;
    }
    state.draining = true;
    void (async () => {
      while (state.queue.length > 0) {
        const wait = this.waitUntil(state) - Date.now();
        if (wait > 0) {
          await new Promise((resolve) => setTimeout(resolve, wait));
        }
        const item = state.queue.shift();
        if (item === undefined) {
          continue;
        }
        state.running = true;
        try {
          item.resolve(await item.operation());
        } catch (error) {
          item.reject(error);
        } finally {
          state.running = false;
          state.nextSlotAt = Date.now() + TELEGRAM_CHAT_SLOT_MS;
        }
      }
      state.draining = false;
      if (state.queue.length > 0) {
        this.drain(chatId, state);
      }
    })().catch((error: unknown) => {
      state.draining = false;
      console.error(
        `Atlas Telegram budget failed: ${error instanceof Error ? error.message : String(error)}`,
      );
      if (state.queue.length > 0) {
        this.drain(chatId, state);
      }
    });
  }
}
