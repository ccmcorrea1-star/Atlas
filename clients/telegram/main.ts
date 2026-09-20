import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

import { TelegramAdapter } from './adapter.js';
import { UnixTelegramRuntime } from './runtime.js';

function listEnv(name: string): string[] {
  return (process.env[name] ?? '')
    .split(',')
    .map((value) => value.trim())
    .filter(Boolean);
}

function runtimeSocketPath(): string {
  if (process.env.ATLAS_RUNTIME_SOCKET?.trim()) {
    return process.env.ATLAS_RUNTIME_SOCKET;
  }
  const runtimeDirectory = process.env.XDG_RUNTIME_DIR?.trim();
  return runtimeDirectory === undefined || runtimeDirectory.length === 0
    ? '/tmp/atlas-runtime.sock'
    : join(runtimeDirectory, 'atlas-runtime.sock');
}

function isMainModule(): boolean {
  const entrypoint = process.argv[1];
  return entrypoint !== undefined && import.meta.url === pathToFileURL(resolve(entrypoint)).href;
}

async function main(): Promise<void> {
  const token = process.env.TELEGRAM_BOT_TOKEN?.trim();
  if (!token) {
    throw new Error('TELEGRAM_BOT_TOKEN is required.');
  }

  const adapter = new TelegramAdapter({
    token,
    runtime: new UnixTelegramRuntime(runtimeSocketPath()),
    allowedUsers: listEnv('TELEGRAM_ALLOWED_USERS'),
    allowedChats: listEnv('TELEGRAM_ALLOWED_CHATS'),
    allowAll: process.env.TELEGRAM_ALLOW_ALL_USERS === 'true',
    homeChatId: process.env.TELEGRAM_HOME_CHAT,
  });
  const stop = () => adapter.stop();
  process.once('SIGINT', stop);
  process.once('SIGTERM', stop);
  await adapter.start();
}

if (isMainModule()) {
  main().catch((error: unknown) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
