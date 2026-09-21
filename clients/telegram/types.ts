import type {
  RuntimeAttachment,
  RuntimeEvent,
  RuntimeTurnContext,
} from '../../src/runtime/protocol.js';

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
  video?: TelegramFileRef;
  animation?: TelegramFileRef;
  photo?: TelegramPhotoSize[];
  document?: TelegramFileRef;
  sticker?: TelegramFileRef;
  media_group_id?: string;
  media_group_messages?: TelegramMessage[];
};

export type TelegramInlineKeyboardButton = {
  text: string;
  callback_data?: string;
};

export type TelegramInlineKeyboardMarkup = {
  inline_keyboard: TelegramInlineKeyboardButton[][];
};

export type TelegramCallbackQuery = {
  id: string;
  from: TelegramUser;
  data?: string;
  message?: TelegramMessage;
};

export type TelegramUpdate = {
  update_id: number;
  message?: TelegramMessage;
  edited_message?: TelegramMessage;
  channel_post?: TelegramMessage;
  edited_channel_post?: TelegramMessage;
  callback_query?: TelegramCallbackQuery;
};

export type TelegramBot = {
  id: number;
  username?: string;
};

export type TelegramSentMessage = {
  message_id: number;
  chat: TelegramChat;
};

export type TelegramBotCommand = {
  command: string;
  description: string;
};

export type TelegramBotCommandScope =
  | { type: 'default' }
  | { type: 'all_private_chats' }
  | { type: 'all_group_chats' }
  | { type: 'all_chat_administrators' };

export type TelegramMediaInput = {
  type: 'photo' | 'video' | 'animation' | 'document';
  media: string;
  caption?: string;
  parse_mode?: 'MarkdownV2';
};

export type TelegramMediaGroupItem = {
  type: 'photo' | 'video' | 'document';
  filePath: string;
  caption?: string;
};

export type TelegramSendOptions = {
  reply_markup?: TelegramInlineKeyboardMarkup;
  message_thread_id?: number;
  reply_to_message_id?: number;
  disable_notification?: boolean;
  parse_mode?: 'MarkdownV2';
};

export type TelegramMediaOptions = {
  message_thread_id?: number;
  reply_to_message_id?: number;
  disable_notification?: boolean;
  parse_mode?: 'MarkdownV2';
};

export type TelegramApi = {
  getMe(): Promise<TelegramBot>;
  getUpdates(
    offset: number | undefined,
    timeoutSeconds: number,
    signal?: AbortSignal,
  ): Promise<TelegramUpdate[]>;
  sendMessage(
    chatId: number | string,
    text: string,
    options?: TelegramSendOptions,
  ): Promise<TelegramSentMessage>;
  editMessageText(
    chatId: number | string,
    messageId: number,
    text: string,
    options?: Pick<TelegramSendOptions, 'parse_mode'>,
  ): Promise<void>;
  editMessageReplyMarkup?(
    chatId: number | string,
    messageId: number,
    replyMarkup: TelegramInlineKeyboardMarkup,
  ): Promise<void>;
  answerCallbackQuery?(callbackQueryId: string, text?: string): Promise<void>;
  getFile(fileId: string): Promise<{ file_path: string; file_size?: number }>;
  downloadFile(filePath: string, destination: string, maxBytes?: number): Promise<void>;
  sendPhoto?(
    chatId: number | string,
    filePath: string,
    caption?: string,
    options?: TelegramMediaOptions,
  ): Promise<void>;
  sendVideo?(
    chatId: number | string,
    filePath: string,
    caption?: string,
    options?: TelegramMediaOptions,
  ): Promise<void>;
  sendAnimation?(
    chatId: number | string,
    filePath: string,
    caption?: string,
    options?: TelegramMediaOptions,
  ): Promise<void>;
  sendMediaGroup?(
    chatId: number | string,
    items: TelegramMediaGroupItem[],
    options?: TelegramMediaOptions,
  ): Promise<void>;
  sendAudio?(
    chatId: number | string,
    filePath: string,
    caption?: string,
    options?: TelegramMediaOptions,
  ): Promise<void>;
  sendVoice?(
    chatId: number | string,
    filePath: string,
    caption?: string,
    options?: TelegramMediaOptions,
  ): Promise<void>;
  sendDocument?(
    chatId: number | string,
    filePath: string,
    caption?: string,
    options?: TelegramMediaOptions,
  ): Promise<void>;
  sendChatAction?(chatId: number | string, action: 'typing' | 'upload_document'): Promise<void>;
  setMyCommands?(commands: TelegramBotCommand[], scope?: TelegramBotCommandScope): Promise<void>;
  getMyCommands?(scope?: TelegramBotCommandScope): Promise<TelegramBotCommand[]>;
};

export type TelegramRuntime = {
  runTurn(
    request: {
      request_id: string;
      conversation_id: string;
      input: string;
      attachments?: RuntimeAttachment[];
      context?: RuntimeTurnContext;
    },
    onEvent: (event: RuntimeEvent) => void,
  ): Promise<RuntimeEvent>;
  command(conversationId: string, command: 'new' | 'status' | 'stop'): Promise<RuntimeEvent>;
  respondApproval(
    conversationId: string,
    approvalId: string,
    approved: boolean,
    comment?: string,
  ): Promise<RuntimeEvent>;
  respondInput?(conversationId: string, inputId: string, value: string): Promise<RuntimeEvent>;
};
