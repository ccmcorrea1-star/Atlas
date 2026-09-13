import { AsyncLocalStorage } from 'node:async_hooks';
import { randomUUID } from 'node:crypto';

import { createAnthropic } from '@ai-sdk/anthropic';
import { createOpenAI } from '@ai-sdk/openai';
import { createOpenAICompatible } from '@ai-sdk/openai-compatible';
import { aisdk } from '@openai/agents-extensions/ai-sdk';
import type { Model, ModelProvider } from '@openai/agents';

// Constantes mantem identidade, endpoint e modelo padrao em um unico contrato.
export const OPENCODE_GO_PROVIDER = 'opencode-go';
export const OPENCODE_GO_MODEL_ID = 'gpt-5.6-luna';
export const OPENCODE_GO_MODEL = `${OPENCODE_GO_PROVIDER}/${OPENCODE_GO_MODEL_ID}`;
export const OPENCODE_GO_BASE_URL = 'https://opencode.ai/zen/go/v1';
export const OPENCODE_GO_RESPONSES_PATH = '/zen/go/v1/responses';
export const OPENCODE_GO_RESPONSES_URL = `https://opencode.ai${OPENCODE_GO_RESPONSES_PATH}`;
export const ATLAS_USER_AGENT = 'Atlas/1.0.0';

// Evita estado mutavel compartilhado entre requests concorrentes.
const activeOpenCodeGoSession = new AsyncLocalStorage<string>();

// Cada modelo declara qual formato de endpoint o adapter deve usar.
export type OpenCodeGoEndpoint = 'responses' | 'chat/completions' | 'messages';

export type OpenCodeGoModelDefinition = {
  endpoint: OpenCodeGoEndpoint;
};

// O registro inicial e pequeno; novos modelos podem ser fornecidos nas opcoes do provider.
export const OPENCODE_GO_MODELS = {
  [OPENCODE_GO_MODEL_ID]: { endpoint: 'responses' },
} as const satisfies Record<string, OpenCodeGoModelDefinition>;

export class OpenCodeGoSession {
  public readonly id: string;

  public constructor(id: string = randomUUID()) {
    // Uma sessao sem ID nao consegue manter continuidade entre requests.
    if (!id.trim()) {
      throw new Error('OpenCode Go session ID cannot be empty.');
    }

    this.id = id;
  }
}

export function withOpenCodeGoSession<T>(
  session: OpenCodeGoSession | string,
  callback: () => Promise<T>,
): Promise<T> {
  // O contexto local garante que requests concorrentes usem o ID correto.
  return activeOpenCodeGoSession.run(typeof session === 'string' ? session : session.id, callback);
}

// Opcoes publicas controlam credenciais, endpoint, headers, sessao e modelos adicionais.
export type OpenCodeGoProviderOptions = {
  apiKey?: string;
  baseURL?: string;
  headers?: Record<string, string>;
  session?: OpenCodeGoSession;
  sessionId?: string;
  userAgent?: string;
  models?: Readonly<Record<string, OpenCodeGoModelDefinition>>;
};

function normalizeBaseURL(baseURL: string): string {
  // Os adapters acrescentam seus proprios paths; a base nao deve terminar em '/'.
  return baseURL.replace(/\/+$/, '');
}

function createAtlasFetch(userAgent: string, defaultSessionId: string): typeof globalThis.fetch {
  return async (input, init) => {
    // Preserva headers do request e das opcoes, mas garante os metadados do Atlas.
    const headers = new Headers(input instanceof Request ? input.headers : undefined);
    const initHeaders = new Headers(init?.headers);

    initHeaders.forEach((value, name) => headers.set(name, value));
    headers.set('User-Agent', userAgent);
    // O header e resolvido por request para acompanhar a conversa ativa.
    headers.set('x-opencode-session', activeOpenCodeGoSession.getStore() ?? defaultSessionId);

    return globalThis.fetch(input, { ...init, headers });
  };
}

function getModelId(modelName: string): string {
  // Aceita tanto "modelo" quanto "opencode-go/modelo" na API publica.
  const prefix = `${OPENCODE_GO_PROVIDER}/`;

  if (modelName.startsWith(prefix)) {
    return modelName.slice(prefix.length);
  }

  if (modelName.includes('/')) {
    throw new Error(`Model "${modelName}" does not belong to provider "${OPENCODE_GO_PROVIDER}".`);
  }

  return modelName;
}

export class OpenCodeGoProvider implements ModelProvider {
  public readonly session: OpenCodeGoSession;
  public readonly sessionId: string;

  private readonly models: Readonly<Record<string, OpenCodeGoModelDefinition>>;
  private readonly responsesProvider: ReturnType<typeof createOpenAI>;
  private readonly chatProvider: ReturnType<typeof createOpenAICompatible>;
  private readonly messagesProvider: ReturnType<typeof createAnthropic>;
  private readonly modelCache = new Map<string, Model>();

  public constructor(options: OpenCodeGoProviderOptions = {}) {
    // A chave explicita tem prioridade; o ambiente serve como configuracao local padrao.
    const apiKey = options.apiKey ?? process.env.OPENCODE_GO_API_KEY;

    if (!apiKey) {
      throw new Error('OPENCODE_GO_API_KEY is required to use OpenCode Go.');
    }

    this.session = options.session ?? new OpenCodeGoSession(options.sessionId);
    this.sessionId = this.session.id;
    // Opcoes adicionais sobrescrevem apenas modelos com o mesmo ID.
    this.models = { ...OPENCODE_GO_MODELS, ...(options.models ?? {}) };

    const headers = {
      ...(options.headers ?? {}),
      'User-Agent': options.userAgent ?? ATLAS_USER_AGENT,
    };
    const fetch = createAtlasFetch(headers['User-Agent'], this.session.id);
    const baseURL = normalizeBaseURL(options.baseURL ?? OPENCODE_GO_BASE_URL);

    // Os tres adapters compartilham fetch, headers e base URL, mas falam formatos diferentes.
    this.responsesProvider = createOpenAI({
      apiKey,
      baseURL,
      fetch,
      headers,
      name: OPENCODE_GO_PROVIDER,
    });
    this.chatProvider = createOpenAICompatible({
      apiKey,
      baseURL,
      fetch,
      headers,
      name: OPENCODE_GO_PROVIDER,
    });
    this.messagesProvider = createAnthropic({
      apiKey,
      baseURL,
      fetch,
      headers,
      name: OPENCODE_GO_PROVIDER,
    });
  }

  public getModel(modelName = OPENCODE_GO_MODEL): Model {
    const modelId = getModelId(modelName);
    const definition = this.models[modelId];

    if (!definition) {
      throw new Error(
        `Unknown OpenCode Go model "${modelId}". Add it to the model registry with a supported endpoint.`,
      );
    }

    const cachedModel = this.modelCache.get(modelId);
    if (cachedModel) {
      // O cache evita recriar o adapter a cada chamada do Runner.
      return cachedModel;
    }

    const model = this.createModel(modelId, definition.endpoint);
    this.modelCache.set(modelId, model);
    return model;
  }

  private createModel(modelId: string, endpoint: OpenCodeGoEndpoint): Model {
    // Converte o endpoint declarado no registro para o adapter compatível do AI SDK.
    switch (endpoint) {
      case 'responses':
        return aisdk(this.responsesProvider.responses(modelId));
      case 'chat/completions':
        return aisdk(this.chatProvider.chatModel(modelId));
      case 'messages':
        return aisdk(this.messagesProvider.messages(modelId));
    }
  }
}
