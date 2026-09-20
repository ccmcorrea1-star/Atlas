import { randomUUID } from 'node:crypto';
import { mkdir, mkdtemp } from 'node:fs/promises';
import { basename, join } from 'node:path';
import { tmpdir } from 'node:os';
import { pathToFileURL, fileURLToPath } from 'node:url';

import type {
  RuntimeAttachment,
  RuntimeCommandCompletedData,
  RuntimeEvent,
  RuntimeTurnCompletedData,
} from '../../src/runtime/protocol.js';
import { FetchTelegramApi } from './api.js';
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
  TelegramFileRef,
  TelegramMessage,
  TelegramRuntime,
  TelegramUpdate,
} from './types.js';

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
};

type ActiveTurn = {
  requestId: string;
  conversationId: string;
  chatId: number | string;
  replyMessageId: number;
  content: string;
};

export class TelegramAdapter {
  private readonly api: TelegramApi;
  private readonly authorization: TelegramAuthorization;
  private readonly homeChatId: number | string | undefined;
  private readonly downloadDirectory: string;
  private readonly activeTurns = new Map<string, ActiveTurn>();
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
  }

  public async start(): Promise<void> {
    this.bot = await this.api.getMe();
    await mkdir(this.downloadDirectory, { recursive: true, mode: 0o700 });
    this.running = true;
    while (this.running) {
      const updates = await this.api.getUpdates(this.offset, 30);
      for (const update of updates) {
        this.offset = update.update_id + 1;
        void this.handleUpdate(update).catch((error: unknown) => {
          console.error(
            `Atlas Telegram update failed: ${error instanceof Error ? error.message : String(error)}`,
          );
        });
      }
    }
  }

  public stop(): void {
    this.running = false;
  }

  public async handleUpdate(update: TelegramUpdate): Promise<void> {
    const message = update.message ?? update.edited_message;
    if (message === undefined || message.from?.is_bot === true) {
      return;
    }
    if (!this.authorization.allows(message)) {
      return;
    }
    if (this.bot === undefined) {
      this.bot = await this.api.getMe();
    }
    if (!groupIsTriggered(message, this.bot)) {
      return;
    }

    const text = message.text ?? message.caption ?? '';
    const command = runtimeCommandFromText(text);
    const conversationId = conversationIdForTelegram(message.chat.id, message.message_thread_id);
    if (command !== undefined) {
      await this.handleCommand(message, conversationId, command);
      return;
    }

    const attachments = await this.materializeAttachments(message);
    const input =
      stripBotMention(text, this.bot.username) ||
      (attachments.length > 0 ? 'Analise os anexos.' : '');
    if (!input) {
      return;
    }
    await this.handleTurn(message, conversationId, input, attachments);
  }

  public async notifyHome(text: string): Promise<void> {
    if (this.homeChatId === undefined) {
      return;
    }
    await this.api.sendMessage(this.homeChatId, text);
  }

  private async handleCommand(
    message: TelegramMessage,
    conversationId: string,
    command: 'new' | 'status' | 'stop',
  ): Promise<void> {
    const event = await this.options.runtime.command(conversationId, command);
    if (event.type !== 'command.completed') {
      await this.api.sendMessage(message.chat.id, eventMessage(event));
      return;
    }
    const data = event.data as unknown as RuntimeCommandCompletedData;
    await this.api.sendMessage(message.chat.id, formatCommandResult(data));
  }

  private async handleTurn(
    message: TelegramMessage,
    conversationId: string,
    input: string,
    attachments: RuntimeAttachment[],
  ): Promise<void> {
    const requestId = randomUUID();
    const reply = await this.api.sendMessage(message.chat.id, '⏳');
    const active: ActiveTurn = {
      requestId,
      conversationId,
      chatId: message.chat.id,
      replyMessageId: reply.message_id,
      content: '',
    };
    this.activeTurns.set(conversationId, active);

    const onEvent = async (event: RuntimeEvent) => {
      if (event.type === 'message.delta') {
        active.content += stringField(event.data.delta);
        await this.editStream(active);
      } else if (event.type === 'message.completed') {
        active.content = stringField(event.data.content);
        await this.editStream(active);
      } else if (event.type === 'approval.requested') {
        await this.api.sendMessage(message.chat.id, formatApproval(event));
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
      if (this.activeTurns.get(conversationId) === active) {
        this.activeTurns.delete(conversationId);
      }
    }
  }

  private async editStream(active: ActiveTurn): Promise<void> {
    const content = active.content || '⏳';
    await this.api.editMessageText(active.chatId, active.replyMessageId, content);
  }

  private async finishTurn(active: ActiveTurn, terminal: RuntimeEvent): Promise<void> {
    if (terminal.type === 'turn.completed' || terminal.type === 'turn.cancelled') {
      const data = terminal.data as unknown as RuntimeTurnCompletedData;
      active.content = data.content || active.content;
      await this.api.editMessageText(active.chatId, active.replyMessageId, active.content || '');
      await this.deliverAttachments(active.chatId, data.attachments);
      return;
    }
    await this.api.editMessageText(active.chatId, active.replyMessageId, eventMessage(terminal));
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

  private async materializeAttachments(message: TelegramMessage): Promise<RuntimeAttachment[]> {
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

    const directory = await mkdtemp(join(this.downloadDirectory, 'turn-'));
    const attachments: RuntimeAttachment[] = [];
    for (const item of refs) {
      const file = await this.api.getFile(item.ref.file_id);
      const name = item.ref.file_name ?? (basename(file.file_path) || `${item.ref.file_id}.bin`);
      const destination = join(directory, name.replace(/[^a-zA-Z0-9._-]/gu, '_'));
      await this.api.downloadFile(file.file_path, destination);
      attachments.push({
        type: item.type,
        uri: pathToFileURL(destination).href,
        media_type: item.mediaType,
        ...(name === undefined ? {} : { file_name: name }),
        ...(file.file_size === undefined ? {} : { size_bytes: file.file_size }),
        source: { platform: 'telegram', file_id: item.ref.file_id },
      });
    }
    return attachments;
  }
}

function stringField(value: unknown): string {
  return typeof value === 'string' ? value : '';
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
