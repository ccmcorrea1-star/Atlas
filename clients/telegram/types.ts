import type { RuntimeAttachment, RuntimeEvent } from '../../src/runtime/protocol.js';

export type TelegramChatType = 'private' | 'group' | 'supergroup' | 'channel';

export type TelegramUser = {
  id: number;
  is_bot?: boolean;
  username?: string;
  first_name?: string;
};

export type TelegramChat = {
  id: number | string;
  type: TelegramChatType;
  title?: string;
};

export type TelegramEntity = {
  type: string;
  offset: number;
  length: number;
  user?: TelegramUser;
};

export type TelegramFileRef = {
  file_id: string;
  file_unique_id?: string;
  file_name?: string;
  mime_type?: string;
  file_size?: number;
};

export type TelegramPhotoSize = TelegramFileRef & { width: number; height: number };

export type TelegramMessage = {
  message_id: number;
  chat: TelegramChat;
  from?: TelegramUser;
  text?: string;
  caption?: string;
  entities?: TelegramEntity[];
  caption_entities?: TelegramEntity[];
  message_thread_id?: number;
  reply_to_message?: TelegramMessage;
  voice?: TelegramFileRef;
  audio?: TelegramFileRef;
  photo?: TelegramPhotoSize[];
  document?: TelegramFileRef;
};

export type TelegramUpdate = {
  update_id: number;
  message?: TelegramMessage;
  edited_message?: TelegramMessage;
};

export type TelegramBot = {
  id: number;
  username?: string;
};

export type TelegramSentMessage = {
  message_id: number;
  chat: TelegramChat;
};

export type TelegramApi = {
  getMe(): Promise<TelegramBot>;
  getUpdates(offset: number | undefined, timeoutSeconds: number): Promise<TelegramUpdate[]>;
  sendMessage(chatId: number | string, text: string): Promise<TelegramSentMessage>;
  editMessageText(chatId: number | string, messageId: number, text: string): Promise<void>;
  getFile(fileId: string): Promise<{ file_path: string; file_size?: number }>;
  downloadFile(filePath: string, destination: string): Promise<void>;
  sendPhoto?(chatId: number | string, filePath: string, caption?: string): Promise<void>;
  sendAudio?(chatId: number | string, filePath: string, caption?: string): Promise<void>;
  sendVoice?(chatId: number | string, filePath: string, caption?: string): Promise<void>;
  sendDocument?(chatId: number | string, filePath: string, caption?: string): Promise<void>;
};

export type TelegramRuntime = {
  runTurn(
    request: {
      request_id: string;
      conversation_id: string;
      input: string;
      attachments?: RuntimeAttachment[];
    },
    onEvent: (event: RuntimeEvent) => void,
  ): Promise<RuntimeEvent>;
  command(conversationId: string, command: 'new' | 'status' | 'stop'): Promise<RuntimeEvent>;
};
