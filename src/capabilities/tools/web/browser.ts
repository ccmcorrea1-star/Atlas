#!/usr/bin/env node
// Tool web.browser: sessão persistente local sobre Camoufox + Playwright.
import { existsSync, mkdirSync, readFileSync, unlinkSync } from 'node:fs';
import { createConnection, createServer, type Socket } from 'node:net';
import { homedir, tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { spawn } from 'node:child_process';

export type BrowserOperation =
  | { operation: 'navigate'; url: string }
  | { operation: 'snapshot'; maxChars?: number }
  | { operation: 'click'; selector: string }
  | { operation: 'type'; selector: string; text: string }
  | { operation: 'scroll'; amount?: number }
  | { operation: 'screenshot' }
  | { operation: 'tabs' }
  | { operation: 'close' };

export type BrowserConfig = {
  provider?: 'camoufox';
  executablePath?: string;
  userDataDir?: string;
  socketPath?: string;
  headless?: boolean;
};

export type BrowserPageAdapter = {
  goto(url: string): Promise<unknown>;
  title(): Promise<string>;
  url(): string;
  locator(selector: string): {
    click(): Promise<void>;
    fill(text: string): Promise<void>;
    innerText(): Promise<string>;
  };
  evaluate(expression: string, amount: number): Promise<unknown>;
  screenshot(options: { type: 'png' }): Promise<Buffer>;
};

export type BrowserContextAdapter = {
  pages(): BrowserPageAdapter[];
  newPage(): Promise<BrowserPageAdapter>;
  close(): Promise<void>;
};

export type BrowserLauncher = (config: BrowserConfig) => Promise<BrowserContextAdapter>;

type PlaywrightLike = {
  firefox: {
    launchPersistentContext(
      userDataDir: string,
      options: { headless: boolean; executablePath?: string },
    ): Promise<BrowserContextAdapter>;
  };
};

function defaultSocketPath(): string {
  return join(tmpdir(), 'atlas-web-browser.sock');
}

function defaultUserDataDir(): string {
  return join(homedir(), '.local', 'share', 'atlas', 'browser');
}

export function browserConfigFromEnvironment(
  env: NodeJS.ProcessEnv = process.env,
): Required<Pick<BrowserConfig, 'headless'>> & BrowserConfig {
  let browser: BrowserConfig = {};
  try {
    const inline = env.ATLAS_WEB_CONFIG_JSON;
    if (inline) {
      browser = (JSON.parse(inline) as { browser?: BrowserConfig }).browser ?? {};
    } else if (env.ATLAS_CONFIG) {
      const config = JSON.parse(readFileSync(env.ATLAS_CONFIG, 'utf8')) as {
        web?: { browser?: BrowserConfig };
      };
      browser = config.web?.browser ?? {};
    }
  } catch {
    browser = {};
  }
  return {
    ...browser,
    headless: browser.headless ?? true,
    socketPath: browser.socketPath ?? defaultSocketPath(),
    userDataDir: browser.userDataDir ?? defaultUserDataDir(),
  };
}

export function parseRequest(text: string): BrowserOperation {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new Error('invalid JSON request');
  }
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('request must be a JSON object');
  }
  const record = value as Record<string, unknown>;
  const operation = record.operation;
  if (
    operation !== 'navigate' &&
    operation !== 'snapshot' &&
    operation !== 'click' &&
    operation !== 'type' &&
    operation !== 'scroll' &&
    operation !== 'screenshot' &&
    operation !== 'tabs' &&
    operation !== 'close'
  ) {
    throw new Error("field 'operation' must be a supported browser operation");
  }
  if (operation === 'navigate') {
    if (typeof record.url !== 'string' || !/^https?:\/\//i.test(record.url)) {
      throw new Error("field 'url' must be an HTTP or HTTPS URL");
    }
    return { operation, url: record.url };
  }
  if (operation === 'click' || operation === 'type') {
    if (typeof record.selector !== 'string' || record.selector.trim() === '') {
      throw new Error("field 'selector' must be a non-empty string");
    }
    if (operation === 'type' && typeof record.text !== 'string') {
      throw new Error("field 'text' must be a string");
    }
    return operation === 'click'
      ? { operation, selector: record.selector }
      : { operation, selector: record.selector, text: record.text as string };
  }
  if (operation === 'scroll') {
    if (
      record.amount !== undefined &&
      (typeof record.amount !== 'number' || !Number.isFinite(record.amount))
    ) {
      throw new Error("field 'amount' must be a finite number");
    }
    return {
      operation,
      ...(record.amount === undefined ? {} : { amount: record.amount as number }),
    };
  }
  if (operation === 'snapshot') {
    if (
      record.maxChars !== undefined &&
      (typeof record.maxChars !== 'number' ||
        !Number.isInteger(record.maxChars) ||
        record.maxChars < 1)
    ) {
      throw new Error("field 'maxChars' must be a positive integer");
    }
    return {
      operation,
      ...(record.maxChars === undefined ? {} : { maxChars: record.maxChars as number }),
    };
  }
  return { operation } as BrowserOperation;
}

async function loadPlaywright(): Promise<PlaywrightLike> {
  for (const packageName of ['playwright', 'playwright-core']) {
    try {
      return (await import(packageName)) as unknown as PlaywrightLike;
    } catch {
      // Tenta o próximo adapter opcional.
    }
  }
  throw new Error('Playwright is not installed; install playwright to enable web.browser');
}

export class BrowserSession {
  private context: BrowserContextAdapter | undefined;
  private page: BrowserPageAdapter | undefined;

  public constructor(
    private readonly config: BrowserConfig,
    private readonly launcher?: BrowserLauncher,
  ) {}

  public get isOpen(): boolean {
    return this.context !== undefined;
  }

  public async execute(operation: BrowserOperation): Promise<Record<string, unknown>> {
    if (operation.operation === 'close') {
      await this.close();
      return { status: 'success', closed: true };
    }
    const page = await this.currentPage();
    switch (operation.operation) {
      case 'navigate':
        await page.goto(operation.url);
        return this.pageState(page);
      case 'snapshot': {
        const text = await page.locator('body').innerText();
        const maxChars = operation.maxChars ?? 20000;
        return { ...this.pageState(page), text: text.slice(0, maxChars) };
      }
      case 'click':
        await page.locator(operation.selector).click();
        return this.pageState(page);
      case 'type':
        await page.locator(operation.selector).fill(operation.text);
        return this.pageState(page);
      case 'scroll':
        await page.evaluate('window.scrollBy(0, amount)', operation.amount ?? 600);
        return this.pageState(page);
      case 'screenshot':
        return {
          ...this.pageState(page),
          mimeType: 'image/png',
          data: (await page.screenshot({ type: 'png' })).toString('base64'),
        };
      case 'tabs':
        return { tabs: await this.tabs() };
    }
  }

  public async close(): Promise<void> {
    if (this.context !== undefined) {
      await this.context.close();
      this.context = undefined;
      this.page = undefined;
    }
  }

  private async currentPage(): Promise<BrowserPageAdapter> {
    if (this.context === undefined) {
      if (this.launcher !== undefined) {
        this.context = await this.launcher(this.config);
      } else {
        const playwright = await loadPlaywright();
        if (this.config.executablePath === undefined || this.config.executablePath.trim() === '') {
          throw new Error('Camoufox executablePath is not configured');
        }
        mkdirSync(this.config.userDataDir ?? defaultUserDataDir(), { recursive: true });
        this.context = await playwright.firefox.launchPersistentContext(
          this.config.userDataDir ?? defaultUserDataDir(),
          { headless: this.config.headless ?? true, executablePath: this.config.executablePath },
        );
      }
    }
    this.page ??= this.context.pages()[0] ?? (await this.context.newPage());
    return this.page;
  }

  private pageState(page: BrowserPageAdapter): Record<string, unknown> {
    return { url: page.url() };
  }

  private async tabs(): Promise<Array<Record<string, unknown>>> {
    const pages = this.context?.pages() ?? [];
    return Promise.all(pages.map(async (page) => ({ url: page.url(), title: await page.title() })));
  }
}

function responseFor(target: string, result: Record<string, unknown>): string {
  return `${JSON.stringify({ target, ...result })}\n`;
}

function connect(socketPath: string): Promise<Socket> {
  return new Promise((resolveSocket, reject) => {
    const socket = createConnection(socketPath);
    socket.once('connect', () => resolveSocket(socket));
    socket.once('error', reject);
  });
}

async function sendToDaemon(
  operation: BrowserOperation,
  config: BrowserConfig,
): Promise<Record<string, unknown>> {
  const socketPath = config.socketPath ?? defaultSocketPath();
  const runtimePath = process.argv[1];
  if (runtimePath === undefined) {
    throw new Error('browser runtime path is unavailable');
  }
  let socket: Socket | undefined;
  for (let attempt = 0; attempt < 30; attempt += 1) {
    try {
      socket = await connect(socketPath);
      break;
    } catch {
      if (attempt === 0) {
        const child = spawn(process.execPath, [runtimePath, '--daemon'], {
          detached: true,
          stdio: 'ignore',
          env: process.env,
        });
        child.unref();
      }
      await new Promise((resolveDelay) => setTimeout(resolveDelay, 50));
    }
  }
  if (socket === undefined) {
    throw new Error('browser session daemon did not become ready');
  }
  return new Promise((resolveResponse, rejectResponse) => {
    let buffer = '';
    socket?.on('data', (chunk) => {
      buffer += chunk.toString();
      const newline = buffer.indexOf('\n');
      if (newline === -1) {
        return;
      }
      const line = buffer.slice(0, newline);
      socket?.end();
      try {
        resolveResponse(JSON.parse(line) as Record<string, unknown>);
      } catch {
        rejectResponse(new Error('browser daemon returned invalid JSON'));
      }
    });
    socket?.once('error', rejectResponse);
    socket?.write(`${JSON.stringify(operation)}\n`);
  });
}

async function runDaemon(): Promise<void> {
  const config = browserConfigFromEnvironment();
  const socketPath = config.socketPath ?? defaultSocketPath();
  mkdirSync(dirname(socketPath), { recursive: true });
  if (existsSync(socketPath)) {
    unlinkSync(socketPath);
  }
  const session = new BrowserSession(config);
  let operationQueue = Promise.resolve();
  const server = createServer((socket) => {
    let buffer = '';
    socket.on('data', (chunk) => {
      buffer += chunk.toString();
      let newline = buffer.indexOf('\n');
      while (newline !== -1) {
        const line = buffer.slice(0, newline);
        buffer = buffer.slice(newline + 1);
        newline = buffer.indexOf('\n');
        operationQueue = operationQueue
          .then(() => parseRequest(line))
          .then((operation) => session.execute(operation))
          .then((result) => {
            socket.write(responseFor('local', result));
          })
          .catch((error: unknown) => {
            const message = error instanceof Error ? error.message : String(error);
            const status = /Playwright|Camoufox|executablePath|daemon/.test(message)
              ? 'unavailable'
              : 'failed';
            socket.write(responseFor('local', { status, error: message }));
          });
      }
    });
  });
  await new Promise<void>((resolveServer, rejectServer) => {
    server.once('error', rejectServer);
    server.listen(socketPath, resolveServer);
  });
  const shutdown = async () => {
    await session.close();
    server.close(() => {
      try {
        unlinkSync(socketPath);
      } catch {
        // O socket pode ter sido removido por limpeza externa.
      }
      process.exit(0);
    });
  };
  process.once('SIGTERM', shutdown);
  process.once('SIGINT', shutdown);
}

async function main(): Promise<void> {
  if (process.argv[2] === '--daemon') {
    await runDaemon();
    return;
  }
  let input = '';
  process.stdin.setEncoding('utf8');
  for await (const chunk of process.stdin) {
    input += chunk;
  }
  try {
    const operation = parseRequest(input);
    const result = await sendToDaemon(operation, browserConfigFromEnvironment());
    process.stdout.write(responseFor('local', result));
  } catch (error) {
    process.stdout.write(responseFor('local', { status: 'failed', error: String(error) }));
  }
}

if (process.argv[1]?.endsWith('/browser.js') || process.argv[1]?.endsWith('/browser/runtime')) {
  await main();
}
