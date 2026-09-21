import { createWriteStream } from 'node:fs';
import { readFile, rm } from 'node:fs/promises';
import { pipeline } from 'node:stream/promises';
import { Readable, Transform } from 'node:stream';

import type {
  TelegramApi,
  TelegramBot,
  TelegramBotCommand,
  TelegramBotCommandScope,
  TelegramInlineKeyboardMarkup,
  TelegramMediaGroupItem,
  TelegramMediaOptions,
  TelegramSendOptions,
  TelegramSentMessage,
  TelegramUpdate,
} from './types.js';
import { TELEGRAM_CAPTION_LIMIT, truncateTelegramText } from './text.js';

type TelegramResponse<T> = {
  ok: boolean;
  result?: T;
  description?: string;
  parameters?: { retry_after?: number };
};

const TELEGRAM_REQUEST_DEADLINE_MS = 40_000;

export class TelegramApiError extends Error {
  public readonly retry_after?: number;
  public readonly statusCode?: number;
  public readonly kind: 'transient' | 'permanent' | 'ambiguous';

  public constructor(
    message: string,
    retryAfter?: number,
    statusCode?: number,
    kind: TelegramApiError['kind'] = 'permanent',
  ) {
    super(message);
    this.name = 'TelegramApiError';
    if (retryAfter !== undefined) {
      this.retry_after = retryAfter;
    }
    this.statusCode = statusCode;
    this.kind = kind;
  }
}

export class FetchTelegramApi implements TelegramApi {
  private readonly apiBase: string;
  private readonly fileBase: string;

  public constructor(
    private readonly token: string,
    apiBase = 'https://api.telegram.org',
    fileBase = 'https://api.telegram.org/file',
  ) {
    this.apiBase = `${apiBase.replace(/\/$/, '')}/bot${token}`;
    this.fileBase = `${fileBase.replace(/\/$/, '')}/bot${token}`;
  }

  public getMe(): Promise<TelegramBot> {
    return this.call<TelegramBot>('getMe');
  }

  public getUpdates(
    offset: number | undefined,
    timeoutSeconds: number,
    signal?: AbortSignal,
  ): Promise<TelegramUpdate[]> {
    const params = new URLSearchParams({
      timeout: String(timeoutSeconds),
      allowed_updates: JSON.stringify([
        'message',
        'edited_message',
        'channel_post',
        'edited_channel_post',
        'callback_query',
        'message_reaction',
        'message_reaction_count',
      ]),
    });
    if (offset !== undefined) {
      params.set('offset', String(offset));
    }
    return this.callGet<TelegramUpdate[]>(
      'getUpdates',
      params,
      signal,
      (timeoutSeconds + 10) * 1_000,
    );
  }

  public sendMessage(
    chatId: number | string,
    text: string,
    options: TelegramSendOptions = {},
  ): Promise<TelegramSentMessage> {
    return this.call<TelegramSentMessage>('sendMessage', {
      chat_id: chatId,
      text,
      ...(options.reply_markup === undefined ? {} : { reply_markup: options.reply_markup }),
      ...(options.message_thread_id === undefined
        ? {}
        : { message_thread_id: options.message_thread_id }),
      ...(options.reply_to_message_id === undefined
        ? {}
        : { reply_to_message_id: options.reply_to_message_id }),
      ...(options.disable_notification === undefined
        ? {}
        : { disable_notification: options.disable_notification }),
      ...(options.parse_mode === undefined ? {} : { parse_mode: options.parse_mode }),
    });
  }

  public async editMessageText(
    chatId: number | string,
    messageId: number,
    text: string,
    options: Pick<TelegramSendOptions, 'parse_mode'> = {},
  ): Promise<void> {
    try {
      await this.call<boolean>('editMessageText', {
        chat_id: chatId,
        message_id: messageId,
        text,
        ...(options.parse_mode === undefined ? {} : { parse_mode: options.parse_mode }),
      });
    } catch (error) {
      if (error instanceof Error && error.message.includes('message is not modified')) {
        return;
      }
      throw error;
    }
  }

  public editMessageReplyMarkup(
    chatId: number | string,
    messageId: number,
    replyMarkup: TelegramInlineKeyboardMarkup,
  ): Promise<void> {
    return this.call<boolean>('editMessageReplyMarkup', {
      chat_id: chatId,
      message_id: messageId,
      reply_markup: replyMarkup,
    }).then(() => undefined);
  }

  public answerCallbackQuery(callbackQueryId: string, text?: string): Promise<void> {
    return this.call<boolean>('answerCallbackQuery', {
      callback_query_id: callbackQueryId,
      ...(text === undefined ? {} : { text }),
    }).then(() => undefined);
  }

  public sendChatAction(
    chatId: number | string,
    action: 'typing' | 'upload_document',
  ): Promise<void> {
    return this.call<boolean>('sendChatAction', { chat_id: chatId, action }).then(() => undefined);
  }

  public setMyCommands(
    commands: TelegramBotCommand[],
    scope?: TelegramBotCommandScope,
  ): Promise<void> {
    return this.call<boolean>('setMyCommands', {
      commands,
      ...(scope === undefined ? {} : { scope }),
    }).then(() => undefined);
  }

  public getMyCommands(scope?: TelegramBotCommandScope): Promise<TelegramBotCommand[]> {
    return this.call<TelegramBotCommand[]>('getMyCommands', scope === undefined ? {} : { scope });
  }

  public getFile(fileId: string): Promise<{ file_path: string; file_size?: number }> {
    return this.call<{ file_path: string; file_size?: number }>('getFile', { file_id: fileId });
  }

  public async downloadFile(
    filePath: string,
    destination: string,
    maxBytes = 20 * 1024 * 1024,
  ): Promise<void> {
    const response = await fetchTelegram(`${this.fileBase}/${filePath}`, {
      signal: AbortSignal.timeout(TELEGRAM_REQUEST_DEADLINE_MS),
    });
    if (!response.ok) {
      throw new TelegramApiError(
        `Telegram file download failed with HTTP ${response.status}.`,
        undefined,
        response.status,
        response.status >= 500 ? 'ambiguous' : 'permanent',
      );
    }
    const contentLength = response.headers.get('content-length');
    if (contentLength !== null && Number(contentLength) > maxBytes) {
      throw new Error(`Telegram attachment exceeds the ${maxBytes}-byte limit.`);
    }
    if (response.body === null) {
      throw new Error('Telegram file download returned an empty body.');
    }

    let totalBytes = 0;
    const limiter = new Transform({
      transform(chunk: Buffer, _encoding, callback) {
        totalBytes += chunk.byteLength;
        if (totalBytes > maxBytes) {
          callback(new Error(`Telegram attachment exceeds the ${maxBytes}-byte limit.`));
          return;
        }
        callback(null, chunk);
      },
    });
    try {
      await pipeline(
        Readable.fromWeb(response.body as globalThis.ReadableStream<Uint8Array>),
        limiter,
        createWriteStream(destination, { mode: 0o600 }),
      );
    } catch (error) {
      await rm(destination, { force: true });
      throw error;
    }
  }

  public sendPhoto(
    chatId: number | string,
    filePath: string,
    caption?: string,
    options?: TelegramMediaOptions,
  ): Promise<void> {
    return this.sendMedia('sendPhoto', 'photo', chatId, filePath, caption, options);
  }

  public sendVideo(
    chatId: number | string,
    filePath: string,
    caption?: string,
    options?: TelegramMediaOptions,
  ): Promise<void> {
    return this.sendMedia('sendVideo', 'video', chatId, filePath, caption, options);
  }

  public sendAnimation(
    chatId: number | string,
    filePath: string,
    caption?: string,
    options?: TelegramMediaOptions,
  ): Promise<void> {
    return this.sendMedia('sendAnimation', 'animation', chatId, filePath, caption, options);
  }

  public sendMediaGroup(
    chatId: number | string,
    items: TelegramMediaGroupItem[],
    options: TelegramMediaOptions = {},
  ): Promise<void> {
    return this.sendMediaGroupRequest(chatId, items, options);
  }

  private async sendMediaGroupRequest(
    chatId: number | string,
    items: TelegramMediaGroupItem[],
    options: TelegramMediaOptions,
  ): Promise<void> {
    if (items.length < 2 || items.length > 10) {
      throw new Error('Telegram media groups must contain between 2 and 10 items.');
    }
    for (let attempt = 0; attempt < 5; attempt += 1) {
      const form = new FormData();
      form.set('chat_id', String(chatId));
      if (options.message_thread_id !== undefined) {
        form.set('message_thread_id', String(options.message_thread_id));
      }
      if (options.reply_to_message_id !== undefined) {
        form.set('reply_to_message_id', String(options.reply_to_message_id));
      }
      if (options.disable_notification !== undefined) {
        form.set('disable_notification', String(options.disable_notification));
      }
      const media = [] as Array<Record<string, string>>;
      for (const [index, item] of items.entries()) {
        const field = `media${index}`;
        media.push({
          type: item.type,
          media: `attach://${field}`,
          ...(item.caption === undefined
            ? {}
            : { caption: truncateTelegramText(item.caption, TELEGRAM_CAPTION_LIMIT) }),
          ...(options.parse_mode === undefined ? {} : { parse_mode: options.parse_mode }),
        });
        form.set(
          field,
          new Blob([await readFile(item.filePath)]),
          item.filePath.split('/').at(-1) ?? field,
        );
      }
      form.set('media', JSON.stringify(media));
      const response = await fetchTelegram(`${this.apiBase}/sendMediaGroup`, {
        method: 'POST',
        body: form,
        signal: AbortSignal.timeout(TELEGRAM_REQUEST_DEADLINE_MS),
      });
      const payload = (await response.json()) as TelegramResponse<unknown>;
      if (response.ok && payload.ok) {
        return;
      }
      const error = telegramError('sendMediaGroup', payload, response.status);
      if (!shouldRetry(error) || attempt === 4) {
        throw error;
      }
      await sleep(retryAfterMilliseconds(error));
    }
  }

  public sendAudio(
    chatId: number | string,
    filePath: string,
    caption?: string,
    options?: TelegramMediaOptions,
  ): Promise<void> {
    return this.sendMedia('sendAudio', 'audio', chatId, filePath, caption, options);
  }

  public sendVoice(
    chatId: number | string,
    filePath: string,
    caption?: string,
    options?: TelegramMediaOptions,
  ): Promise<void> {
    return this.sendMedia('sendVoice', 'voice', chatId, filePath, caption, options);
  }

  public sendDocument(
    chatId: number | string,
    filePath: string,
    caption?: string,
    options?: TelegramMediaOptions,
  ): Promise<void> {
    return this.sendMedia('sendDocument', 'document', chatId, filePath, caption, options);
  }

  private async sendMedia(
    method: string,
    field: string,
    chatId: number | string,
    filePath: string,
    caption?: string,
    options: TelegramMediaOptions = {},
  ): Promise<void> {
    for (let attempt = 0; attempt < 5; attempt += 1) {
      const form = new FormData();
      form.set('chat_id', String(chatId));
      if (caption !== undefined) {
        form.set('caption', truncateTelegramText(caption, TELEGRAM_CAPTION_LIMIT));
      }
      if (options.message_thread_id !== undefined) {
        form.set('message_thread_id', String(options.message_thread_id));
      }
      if (options.reply_to_message_id !== undefined) {
        form.set('reply_to_message_id', String(options.reply_to_message_id));
      }
      if (options.disable_notification !== undefined) {
        form.set('disable_notification', String(options.disable_notification));
      }
      if (options.parse_mode !== undefined) {
        form.set('parse_mode', options.parse_mode);
      }
      form.set(
        field,
        new Blob([await readFile(filePath)]),
        filePath.split('/').at(-1) ?? 'attachment',
      );
      const response = await fetchTelegram(`${this.apiBase}/${method}`, {
        method: 'POST',
        body: form,
        signal: AbortSignal.timeout(TELEGRAM_REQUEST_DEADLINE_MS),
      });
      const payload = (await response.json()) as TelegramResponse<unknown>;
      if (response.ok && payload.ok) {
        return;
      }
      const error = telegramError(method, payload, response.status);
      if (!shouldRetry(error) || attempt === 4) {
        throw error;
      }
      await sleep(retryAfterMilliseconds(error));
    }
  }

  private async callGet<T>(
    method: string,
    params: URLSearchParams,
    signal?: AbortSignal,
    timeoutMs = 40_000,
  ): Promise<T> {
    for (let attempt = 0; attempt < 5; attempt += 1) {
      const response = await fetchTelegram(`${this.apiBase}/${method}?${params.toString()}`, {
        signal:
          signal === undefined
            ? AbortSignal.timeout(timeoutMs)
            : AbortSignal.any([signal, AbortSignal.timeout(timeoutMs)]),
      });
      const payload = (await response.json()) as TelegramResponse<T>;
      if (response.ok && payload.ok && payload.result !== undefined) {
        return payload.result;
      }
      const error = telegramError(method, payload, response.status);
      if (!shouldRetry(error) || attempt === 4) {
        throw error;
      }
      await sleep(retryAfterMilliseconds(error));
    }
    throw new Error(`Telegram API ${method} retry loop exhausted.`);
  }

  private async call<T>(method: string, body: Record<string, unknown> = {}): Promise<T> {
    for (let attempt = 0; attempt < 5; attempt += 1) {
      const response = await fetchTelegram(`${this.apiBase}/${method}`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(TELEGRAM_REQUEST_DEADLINE_MS),
      });
      const payload = (await response.json()) as TelegramResponse<T>;
      if (response.ok && payload.ok && payload.result !== undefined) {
        return payload.result;
      }
      const error = telegramError(method, payload, response.status);
      if (!shouldRetry(error) || attempt === 4) {
        throw error;
      }
      await sleep(retryAfterMilliseconds(error));
    }
    throw new Error(`Telegram API ${method} retry loop exhausted.`);
  }
}

function telegramError<T>(
  method: string,
  payload: TelegramResponse<T>,
  statusCode?: number,
): TelegramApiError {
  const description = payload.description ?? `Telegram API ${method} failed.`;
  const retryAfter = payload.parameters?.retry_after;
  const status = statusCode ?? (description.includes('Conflict') ? 409 : undefined);
  const kind =
    retryAfter !== undefined || status === 429 || status === 409
      ? 'transient'
      : status !== undefined && status >= 500
        ? 'ambiguous'
        : 'permanent';
  return new TelegramApiError(
    `Telegram API ${method} failed: ${sanitizeTelegramDescription(description)}`,
    retryAfter,
    status,
    kind,
  );
}

function sanitizeTelegramDescription(description: string): string {
  return description.replace(/[\r\n\t]+/gu, ' ').slice(0, 300);
}

function shouldRetry(error: TelegramApiError): boolean {
  return error.retry_after !== undefined || error.statusCode === 429;
}

function retryAfterMilliseconds(error: TelegramApiError): number {
  return Math.max(50, Math.ceil((error.retry_after ?? 1) * 1_000));
}

function sleep(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function fetchTelegram(input: string, init: RequestInit): Promise<Response> {
  try {
    return await fetch(input, init);
  } catch (error) {
    if (error instanceof Error && error.name === 'AbortError') {
      throw error;
    }
    throw new TelegramApiError(
      `Telegram network request failed: ${sanitizeTelegramDescription(
        error instanceof Error ? error.message : String(error),
      )}`,
      undefined,
      undefined,
      'ambiguous',
    );
  }
}

export async function readTelegramMedia(path: string): Promise<Uint8Array> {
  return new Uint8Array(await readFile(path));
}
