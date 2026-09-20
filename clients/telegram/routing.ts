import { RUNTIME_COMMANDS, type RuntimeCommandName } from '../../src/runtime/protocol.js';
import type { TelegramMessage } from './types.js';

export type TelegramAuthorizationConfig = {
  allowedUsers?: Iterable<number | string>;
  allowedChats?: Iterable<number | string>;
  allowAll?: boolean;
};

function idSet(values: Iterable<number | string> | undefined): Set<string> {
  return new Set([...(values ?? [])].map((value) => String(value)));
}

export class TelegramAuthorization {
  private readonly users: Set<string>;
  private readonly chats: Set<string>;

  public constructor(private readonly config: TelegramAuthorizationConfig = {}) {
    this.users = idSet(config.allowedUsers);
    this.chats = idSet(config.allowedChats);
  }

  public allows(message: TelegramMessage): boolean {
    if (this.config.allowAll === true) {
      return true;
    }
    const userAllowed = message.from !== undefined && this.users.has(String(message.from.id));
    const chatAllowed = this.chats.has(String(message.chat.id));
    return userAllowed || chatAllowed;
  }
}

export function conversationIdForTelegram(chatId: number | string, threadId?: number): string {
  const encodedChat = encodeURIComponent(String(chatId));
  const encodedThread = encodeURIComponent(threadId === undefined ? 'root' : String(threadId));
  return `telegram:${encodedChat}:thread:${encodedThread}`;
}

export function runtimeCommandFromText(text: string): RuntimeCommandName | undefined {
  const match = /^\/(\w+)(?:@[^\s]+)?(?:\s|$)/u.exec(text.trim());
  if (match === null) {
    return undefined;
  }
  const name = match[1];
  return RUNTIME_COMMANDS.some((command) => command.name === name)
    ? (name as RuntimeCommandName)
    : undefined;
}

export function isGroupMessage(message: TelegramMessage): boolean {
  return message.chat.type === 'group' || message.chat.type === 'supergroup';
}

export function groupIsTriggered(
  message: TelegramMessage,
  bot: { id: number; username?: string },
): boolean {
  if (!isGroupMessage(message)) {
    return true;
  }
  if (message.reply_to_message?.from?.id === bot.id) {
    return true;
  }
  const username = bot.username?.toLowerCase();
  return (
    username !== undefined &&
    (message.entities ?? message.caption_entities ?? []).some((entity) => {
      if (entity.type === 'text_mention') {
        return entity.user?.id === bot.id;
      }
      if (entity.type !== 'mention' || message.text === undefined) {
        return false;
      }
      const value = message.text.slice(entity.offset, entity.offset + entity.length);
      return value.toLowerCase() === `@${username}`;
    })
  );
}

export function stripBotMention(text: string, botUsername?: string): string {
  if (botUsername === undefined) {
    return text.trim();
  }
  return text.replace(new RegExp(`@${botUsername}\\b`, 'giu'), '').trim();
}
