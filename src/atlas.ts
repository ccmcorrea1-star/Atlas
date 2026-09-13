import { randomUUID } from 'node:crypto';

import { Agent, MemorySession, Runner } from '@openai/agents';

import {
  OpenCodeGoProvider,
  type OpenCodeGoProviderOptions,
  OPENCODE_GO_MODEL,
  withOpenCodeGoSession,
} from './opencode-go.js';

export const Atlas = new Agent({
  name: 'Atlas',
  instructions:
    'You are Atlas, a pragmatic coding agent. Give clear, concise answers and do not claim work you did not perform.',
  model: OPENCODE_GO_MODEL,
});

export type AtlasRunOptions = OpenCodeGoProviderOptions & {
  conversationId?: string;
};

type AtlasRuntime = {
  runner: Runner;
  sessions: Map<string, MemorySession>;
};

const atlasRuntimes = new Map<string, AtlasRuntime>();

export function createAtlasRunner(options: OpenCodeGoProviderOptions = {}): Runner {
  return new Runner({
    modelProvider: new OpenCodeGoProvider(options),
    tracingDisabled: true,
  });
}

function stableSerialize(value: unknown): string {
  if (value === undefined) {
    return 'undefined';
  }

  if (value === null || typeof value !== 'object') {
    return JSON.stringify(value);
  }

  if (Array.isArray(value)) {
    return `[${value.map((item) => stableSerialize(item)).join(',')}]`;
  }

  const record = value as Record<string, unknown>;
  return `{${Object.keys(record)
    .sort()
    .map((key) => `${JSON.stringify(key)}:${stableSerialize(record[key])}`)
    .join(',')}}`;
}

function getRuntimeKey(options: OpenCodeGoProviderOptions): string {
  const { session: _session, sessionId: _sessionId, ...providerOptions } = options;
  return stableSerialize(providerOptions);
}

function getAtlasRuntime(options: OpenCodeGoProviderOptions): AtlasRuntime {
  const key = getRuntimeKey(options);
  const existingRuntime = atlasRuntimes.get(key);
  if (existingRuntime) {
    return existingRuntime;
  }

  const runtime: AtlasRuntime = {
    runner: createAtlasRunner(options),
    sessions: new Map(),
  };
  atlasRuntimes.set(key, runtime);
  return runtime;
}

export function getAtlasRunner(options: OpenCodeGoProviderOptions = {}): Runner {
  return getAtlasRuntime(options).runner;
}

function getConversationId(options: AtlasRunOptions): string {
  const requestedId = options.conversationId ?? options.sessionId ?? options.session?.id;
  if (requestedId !== undefined) {
    if (!requestedId.trim()) {
      throw new Error('Atlas conversation ID cannot be empty.');
    }

    return requestedId;
  }

  return randomUUID();
}

export async function runAtlas(input: string, options: AtlasRunOptions = {}) {
  const { conversationId: _conversationId, ...providerOptions } = options;
  const runtime = getAtlasRuntime(providerOptions);
  const sessionId = getConversationId(options);
  const session =
    runtime.sessions.get(sessionId) ?? new MemorySession({ sessionId });

  runtime.sessions.set(sessionId, session);

  return withOpenCodeGoSession(sessionId, () =>
    runtime.runner.run(Atlas, input, { session }),
  );
}
