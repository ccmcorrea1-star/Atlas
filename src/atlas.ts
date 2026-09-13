import { randomUUID } from 'node:crypto';

import { Agent, MemorySession, Runner } from '@openai/agents';

import {
  OpenCodeGoProvider,
  type OpenCodeGoProviderOptions,
  OPENCODE_GO_MODEL,
  withOpenCodeGoSession,
} from './opencode-go.js';

// O Agent define a identidade e as instrucoes; a execucao concreta fica no Runner configurado abaixo.
export const Atlas = new Agent({
  name: 'Atlas',
  instructions:
    'You are Atlas, a pragmatic coding agent. Give clear, concise answers and do not claim work you did not perform.',
  model: OPENCODE_GO_MODEL,
});

export type AtlasRunOptions = OpenCodeGoProviderOptions & {
  // O ID explicito permite continuar a mesma conversa entre chamadas.
  conversationId?: string;
};

// Cada runtime agrupa um Runner reutilizavel e as sessoes das suas conversas.
type AtlasRuntime = {
  runner: Runner;
  sessions: Map<string, MemorySession>;
};

// A chave e a configuracao do provider, nao o ID da conversa.
const atlasRuntimes = new Map<string, AtlasRuntime>();

// Cria o Runner com tracing desabilitado para manter a execucao local e previsivel.
export function createAtlasRunner(options: OpenCodeGoProviderOptions = {}): Runner {
  return new Runner({
    modelProvider: new OpenCodeGoProvider(options),
    tracingDisabled: true,
  });
}

function stableSerialize(value: unknown): string {
  // Serializacao ordenada impede que a ordem das chaves gere runtimes duplicados.
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
  // A sessao pertence a conversa; o Runner deve ser compartilhado entre elas.
  const { session: _session, sessionId: _sessionId, ...providerOptions } = options;
  return stableSerialize(providerOptions);
}

// Reutiliza o Runner por configuração e mantém as sessões separadas por conversa.
function getAtlasRuntime(options: OpenCodeGoProviderOptions): AtlasRuntime {
  const key = getRuntimeKey(options);
  const existingRuntime = atlasRuntimes.get(key);
  if (existingRuntime) {
    // Reaproveita provider, cache de modelos e conexoes para a mesma configuracao.
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
  // Expor o mesmo Runner permite chamadas diretas e chamadas por runAtlas.
  return getAtlasRuntime(options).runner;
}

// Gera uma conversa nova quando o chamador não informa um identificador.
function getConversationId(options: AtlasRunOptions): string {
  // Mantem compatibilidade com as formas de sessao aceitas pelo provider.
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
  // O ID de conversa nao participa da chave do runtime, apenas da sessao de historico.
  const { conversationId: _conversationId, ...providerOptions } = options;
  const runtime = getAtlasRuntime(providerOptions);
  const sessionId = getConversationId(options);
  // A Session guarda o historico; o contexto assincrono aplica seu ID ao request.
  const session = runtime.sessions.get(sessionId) ?? new MemorySession({ sessionId });

  runtime.sessions.set(sessionId, session);

  return withOpenCodeGoSession(sessionId, () => runtime.runner.run(Atlas, input, { session }));
}
