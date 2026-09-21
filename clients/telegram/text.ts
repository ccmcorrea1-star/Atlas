export const TELEGRAM_MESSAGE_LIMIT = 4096;
export const TELEGRAM_CAPTION_LIMIT = 1024;

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

function isLowSurrogate(value: number): boolean {
  return value >= 0xdc00 && value <= 0xdfff;
}
