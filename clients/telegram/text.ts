export const TELEGRAM_MESSAGE_LIMIT = 4096;
export const TELEGRAM_CAPTION_LIMIT = 1024;

const MARKDOWN_V2_SPECIALS = new RegExp('([_*\\[\\]~`>#+=|{}.!\\\\-])', 'gu');
const CODE_PLACEHOLDER = /\uE000(\d+)\uE001/gu;
const BLOCKQUOTE_MARKER = '\uE010';

export function telegramUtf16Length(text: string): number {
  return text.length;
}

export function truncateTelegramText(text: string, limit = TELEGRAM_MESSAGE_LIMIT): string {
  if (text.length <= limit) {
    return text;
  }
  let end = limit;
  if (end > 0 && isLowSurrogate(text.charCodeAt(end))) {
    end -= 1;
  }
  return text.slice(0, end);
}

export function splitTelegramText(text: string, limit = TELEGRAM_MESSAGE_LIMIT): string[] {
  if (text.length === 0) {
    return [' '];
  }
  if (limit < 1) {
    throw new RangeError('Telegram text limit must be positive.');
  }

  const chunks: string[] = [];
  let start = 0;
  let units = 0;
  for (let index = 0; index < text.length;) {
    const codePoint = text.codePointAt(index);
    const width = codePoint !== undefined && codePoint > 0xffff ? 2 : 1;
    if (units + width > limit) {
      chunks.push(text.slice(start, index));
      start = index;
      units = 0;
    }
    index += width;
    units += width;
  }
  if (start < text.length) {
    chunks.push(text.slice(start));
  }
  return chunks;
}

export function telegramTextFits(text: string, limit = TELEGRAM_MESSAGE_LIMIT): boolean {
  return text.length <= limit;
}

/** Converte Markdown comum para o subconjunto seguro aceito pelo Telegram. */
export function renderTelegramMarkdown(text: string): string {
  const placeholders: string[] = [];
  const protect = (value: string): string => {
    const token = `\uE000${placeholders.length}\uE001`;
    placeholders.push(value);
    return token;
  };

  let source = normalizeMarkdownLayout(text);
  source = source.replace(
    /```([^\n]*)\n([\s\S]*?)```/gu,
    (_match, language: string, body: string) => {
      const header = language.trim() ? `${language.trim()}\n` : '';
      return protect(`\`\`\`${header}${escapeCode(body)}\`\`\``);
    },
  );
  source = source.replace(/`([^`\n]+)`/gu, (_match, code: string) =>
    protect(`\`${escapeCode(code)}\``),
  );
  source = source.replace(/\[([^\]]+)\]\(([^)]+)\)/gu, (_match, label: string, url: string) =>
    protect(`[${escapeInline(label)}](${escapeUrl(url)})`),
  );
  source = source.replace(/\*\*([^*]+)\*\*/gu, (_match, value: string) =>
    protect(`*${escapeInline(value)}*`),
  );
  source = source.replace(/__([^_]+)__/gu, (_match, value: string) =>
    protect(`*${escapeInline(value)}*`),
  );
  source = source.replace(/~~([^~]+)~~/gu, (_match, value: string) =>
    protect(`~${escapeInline(value)}~`),
  );
  source = source.replace(
    /(^|\s)\*([^*\n]+)\*(?=\s|$)/gmu,
    (_match, prefix: string, value: string) => `${prefix}${protect(`_${escapeInline(value)}_`)}`,
  );

  let rendered = escapeInline(source);
  rendered = rendered.replace(
    CODE_PLACEHOLDER,
    (_match, index: string) => placeholders[Number(index)] ?? '',
  );
  return rendered.replaceAll(BLOCKQUOTE_MARKER, '>');
}

function normalizeMarkdownLayout(text: string): string {
  const lines = text.replace(/\r\n?/gu, '\n').split('\n');
  const result: string[] = [];
  let inTable = false;
  for (const line of lines) {
    if (/^\s*\|?.+\|.+\|?\s*$/u.test(line) && /\|/u.test(line)) {
      const cells = line
        .split('|')
        .map((cell) => cell.trim())
        .filter(Boolean);
      if (cells.length > 0 && cells.every((cell) => /^:?-{3,}:?$/u.test(cell))) {
        inTable = true;
        continue;
      }
      if (inTable) {
        result.push(`• ${cells.join(' — ')}`);
        continue;
      }
    }
    inTable = false;
    const heading = /^\s*#{1,6}\s+(.*)$/u.exec(line);
    if (heading !== null) {
      result.push(`**${heading[1]}**`);
      continue;
    }
    const bullet = /^\s*[-+*]\s+(.*)$/u.exec(line);
    if (bullet !== null) {
      result.push(`• ${bullet[1]}`);
      continue;
    }
    const quote = /^\s*>\s?(.*)$/u.exec(line);
    if (quote !== null) {
      result.push(`${BLOCKQUOTE_MARKER} ${quote[1]}`);
      continue;
    }
    result.push(line);
  }
  return result.join('\n');
}

function escapeInline(value: string): string {
  return value.replace(MARKDOWN_V2_SPECIALS, '\\$1');
}

function escapeCode(value: string): string {
  return value.replace(/[\\`]/gu, '\\$&');
}

function escapeUrl(value: string): string {
  return value.replace(/[\\)]/gu, '\\$&');
}

function isLowSurrogate(value: number): boolean {
  return value >= 0xdc00 && value <= 0xdfff;
}
