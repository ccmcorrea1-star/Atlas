import { createWriteStream } from 'node:fs';
import { readFile, rm } from 'node:fs/promises';
import { pipeline } from 'node:stream/promises';
import { Readable, Transform } from 'node:stream';

import type {
  TelegramApi,
  TelegramBot,
  TelegramInlineKeyboardMarkup,
  TelegramSentMessage,
  TelegramUpdate,
} from './types.js';

type TelegramResponse<T> = {
  ok: boolean;
  result?: T;
  description?: string;
};

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

  public getUpdates(offset: number | undefined, timeoutSeconds: number): Promise<TelegramUpdate[]> {
    const params = new URLSearchParams({
      timeout: String(timeoutSeconds),
      allowed_updates: JSON.stringify(['message', 'callback_query']),
    });
    if (offset !== undefined) {
      params.set('offset', String(offset));
    }
    return this.callGet<TelegramUpdate[]>('getUpdates', params);
  }

  public sendMessage(
    chatId: number | string,
    text: string,
    options: { reply_markup?: TelegramInlineKeyboardMarkup } = {},
  ): Promise<TelegramSentMessage> {
    return this.call<TelegramSentMessage>('sendMessage', {
      chat_id: chatId,
      text,
      ...(options.reply_markup === undefined ? {} : { reply_markup: options.reply_markup }),
    });
  }

  public async editMessageText(
    chatId: number | string,
    messageId: number,
    text: string,
  ): Promise<void> {
    try {
      await this.call<boolean>('editMessageText', {
        chat_id: chatId,
        message_id: messageId,
        text,
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

  public getFile(fileId: string): Promise<{ file_path: string; file_size?: number }> {
    return this.call<{ file_path: string; file_size?: number }>('getFile', { file_id: fileId });
  }

  public async downloadFile(
    filePath: string,
    destination: string,
    maxBytes = 20 * 1024 * 1024,
  ): Promise<void> {
    const response = await fetch(`${this.fileBase}/${filePath}`, {
      signal: AbortSignal.timeout(60_000),
    });
    if (!response.ok) {
      throw new Error(`Telegram file download failed with HTTP ${response.status}.`);
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

  public sendPhoto(chatId: number | string, filePath: string, caption?: string): Promise<void> {
    return this.sendMedia('sendPhoto', 'photo', chatId, filePath, caption);
  }

  public sendAudio(chatId: number | string, filePath: string, caption?: string): Promise<void> {
    return this.sendMedia('sendAudio', 'audio', chatId, filePath, caption);
  }

  public sendVoice(chatId: number | string, filePath: string, caption?: string): Promise<void> {
    return this.sendMedia('sendVoice', 'voice', chatId, filePath, caption);
  }

  public sendDocument(chatId: number | string, filePath: string, caption?: string): Promise<void> {
    return this.sendMedia('sendDocument', 'document', chatId, filePath, caption);
  }

  private async sendMedia(
    method: string,
    field: string,
    chatId: number | string,
    filePath: string,
    caption?: string,
  ): Promise<void> {
    const form = new FormData();
    form.set('chat_id', String(chatId));
    if (caption !== undefined) {
      form.set('caption', caption);
    }
    form.set(
      field,
      new Blob([await readFile(filePath)]),
      filePath.split('/').at(-1) ?? 'attachment',
    );
    const response = await fetch(`${this.apiBase}/${method}`, { method: 'POST', body: form });
    const payload = (await response.json()) as TelegramResponse<unknown>;
    if (!response.ok || !payload.ok) {
      throw new Error(payload.description ?? `Telegram API ${method} failed.`);
    }
  }

  private async callGet<T>(method: string, params: URLSearchParams): Promise<T> {
    const response = await fetch(`${this.apiBase}/${method}?${params.toString()}`, {
      signal: AbortSignal.timeout(40_000),
    });
    const payload = (await response.json()) as TelegramResponse<T>;
    if (!response.ok || !payload.ok || payload.result === undefined) {
      throw new Error(payload.description ?? `Telegram API ${method} failed.`);
    }
    return payload.result;
  }

  private async call<T>(method: string, body: Record<string, unknown> = {}): Promise<T> {
    const response = await fetch(`${this.apiBase}/${method}`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
    });
    const payload = (await response.json()) as TelegramResponse<T>;
    if (!response.ok || !payload.ok || payload.result === undefined) {
      throw new Error(payload.description ?? `Telegram API ${method} failed.`);
    }
    return payload.result;
  }
}

export async function readTelegramMedia(path: string): Promise<Uint8Array> {
  return new Uint8Array(await readFile(path));
}
