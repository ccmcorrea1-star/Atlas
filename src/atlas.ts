import { randomUUID } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  Agent,
  MemorySession,
  Runner,
  type FunctionTool,
  type Model,
  type RunStreamEvent,
} from '@openai/agents';

import {
  OpenCodeGoProvider,
  type OpenCodeGoProviderOptions,
  OPENCODE_GO_MODEL,
  withOpenCodeGoAbortSignal,
  withOpenCodeGoSession,
} from './opencode-go.js';
import { HookableCapabilityRuntime, RetryGuard, stableSerialize } from './capability-hooks.js';
import {
  createCapabilityRuntime,
  type CapabilityDefinition,
  type CapabilityDiscoveryRequest,
  type CapabilityDiscoveryResult,
  type CapabilityExecutionOptions,
  type CapabilityRuntime,
} from './capability-runtime.js';

let instructionsCache: string | undefined;

// Os .txt ficam em src/prompts; no build, o modulo esta em dist e o fonte continua ao lado.
function promptsDirectory(): string {
  const moduleDirectory = dirname(fileURLToPath(import.meta.url));
  const candidates = [
    join(moduleDirectory, 'prompts'),
    join(moduleDirectory, '..', 'src', 'prompts'),
  ];
  const directory = candidates.find((candidate) => existsSync(candidate));
  if (directory === undefined) {
    throw new Error('Atlas prompt directory was not found.');
  }
  return directory;
}

// Carrega o prompt padrao do Agent com cache; o conteudo e imutavel durante o processo.
export function loadInstructions(): string {
  if (instructionsCache !== undefined) {
    return instructionsCache;
  }

  instructionsCache = readFileSync(join(promptsDirectory(), 'default.txt'), 'utf8').trim();
  return instructionsCache;
}

// Capabilities com uso frequente que o Agent expoe como Function Tool direta.
// O nome, a descricao e o schema vem sempre do Registry em tempo de execucao.
export const MATERIALIZED_CAPABILITY_IDS = [
  'filesystem.read',
  'filesystem.list',
  'filesystem.search',
  'filesystem.write',
  'filesystem.edit',
  'filesystem.glob',
  'filesystem.patch',
  'shell.exec',
  'system.info',
  'lsp.diagnostics',
  'git.status',
  'git.diff',
] as const;

// O provedor aceita apenas [a-zA-Z0-9_-] no nome da Function Tool.
export function materializedToolName(capabilityId: string): string {
  return capabilityId.replaceAll('.', '_');
}

// O Agent base define a identidade; cada turno recebe as tools base do Atlas.
export const Atlas = new Agent({
  name: 'Atlas',
  instructions: loadInstructions(),
  model: OPENCODE_GO_MODEL,
});

export type AtlasRunOptions = OpenCodeGoProviderOptions & {
  // O signal permite ao Runtime interromper o request de modelo ativo.
  abortSignal?: AbortSignal;
  // O modelo da forma "provider/modelo" definido pela configuração global.
  model?: string;
  // O ID explicito permite continuar a mesma conversa entre chamadas.
  conversationId?: string;
  capabilityRuntime?: CapabilityRuntime;
  // Desligar mantem apenas o caminho generico (list_tools/discover/describe/execute).
  coreTools?: boolean;
  onEvent?: (event: AtlasRunEvent) => void | Promise<void>;
};

// Eventos publicos permitem observar a execucao sem expor tipos do Agent SDK.
export type AtlasRunEvent =
  | {
      type: 'message.delta';
      messageId: string;
      delta: string;
    }
  | {
      type: 'message.completed';
      messageId: string;
      content: string;
    }
  | {
      type: 'tool.started';
      toolId: string;
      toolName: string;
      target?: string;
    }
  | {
      type: 'tool.completed';
      toolId: string;
      toolName: string;
      output?: string;
    }
  | {
      type: 'execution.started';
      executionId: string;
      capability: 'shell.exec';
      program: string;
      args: string[];
      cwd?: string;
      target?: string;
    }
  | {
      type: 'execution.output.delta';
      executionId: string;
      capability: 'shell.exec';
      channel: 'stdout' | 'stderr';
      delta: string;
    }
  | {
      type: 'execution.completed';
      executionId: string;
      capability: 'shell.exec';
      stdout: string;
      stderr: string;
      exitCode: number;
      durationMs: number;
      status: string;
    };

// Cada runtime agrupa um Runner reutilizavel e as sessoes das suas conversas.
type AtlasRuntime = {
  runner: Runner;
  sessions: Map<string, MemorySession>;
  capabilityRuntime: CapabilityRuntime;
};

// A chave e a configuracao do provider, nao o ID da conversa.
const atlasRuntimes = new Map<string, AtlasRuntime>();

// Cria o Runner com tracing desabilitado para manter a execucao local e previsivel.
export function createAtlasRunner(options: OpenCodeGoProviderOptions = {}): Runner {
  return createRunner(options);
}

function createRunner(options: OpenCodeGoProviderOptions): Runner {
  const provider = new OpenCodeGoProvider(options);
  return new Runner({
    modelProvider: provider,
    tracingDisabled: true,
  });
}

// As definicoes nao mudam durante o processo: o cache evita uma ida ao Registry
// por turno, e cada turno ainda monta as tools sobre o runtime com hooks.
const materializedDefinitionCache = new WeakMap<
  CapabilityRuntime,
  Promise<CapabilityDefinition[]>
>();

function loadMaterializedDefinitions(
  capabilityRuntime: CapabilityRuntime,
): Promise<CapabilityDefinition[]> {
  const cached = materializedDefinitionCache.get(capabilityRuntime);
  if (cached !== undefined) {
    return cached;
  }

  const loading = Promise.all(
    MATERIALIZED_CAPABILITY_IDS.map(async (id) => {
      try {
        const definition = await capabilityRuntime.getDefinition(id);
        // Uma definicao de outro id nao materializa tool nenhuma.
        return definition?.id === id ? definition : undefined;
      } catch {
        // Falha de Registry nao pode derrubar o caminho generico de execucao.
        return undefined;
      }
    }),
  ).then((definitions) =>
    definitions.filter((item): item is CapabilityDefinition => item !== undefined),
  );
  materializedDefinitionCache.set(capabilityRuntime, loading);
  return loading;
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
    runner: createRunner(options),
    sessions: new Map(),
    capabilityRuntime: createCapabilityRuntime(),
  };
  atlasRuntimes.set(key, runtime);
  return runtime;
}

function parseObjectInput(input: string, toolName: string): Record<string, unknown> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(input) as unknown;
  } catch (error) {
    throw new Error(
      `${toolName} received invalid JSON arguments: ${error instanceof Error ? error.message : String(error)}`,
    );
  }

  if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error(`${toolName} arguments must be a JSON object.`);
  }

  return parsed as Record<string, unknown>;
}

function executionTool(
  capabilityRuntime: CapabilityRuntime,
  onOutput?: (
    capabilityId: string,
    executionId: string,
    channel: 'stdout' | 'stderr',
    delta: string,
  ) => void | Promise<void>,
): FunctionTool {
  return {
    type: 'function',
    name: 'execute',
    description: 'executa qualquer capability registrada pelo id e pelos argumentos fornecidos',
    parameters: {
      type: 'object',
      properties: {
        id: { type: 'string', description: 'identificador exato da capability' },
        arguments: {
          type: 'object',
          description: 'argumentos definidos pelo schema da capability',
          additionalProperties: true,
        },
      },
      required: ['id', 'arguments'],
      additionalProperties: false,
    } as FunctionTool['parameters'],
    strict: false,
    needsApproval: async () => false,
    isEnabled: async () => true,
    invoke: async (_runContext, input, details) => {
      const request = parseObjectInput(input, 'execute');
      const id = request.id;
      if (typeof id !== 'string' || !id) {
        throw new Error('execute id must be a non-empty string.');
      }
      const arguments_ = recordValue(decodedJsonValue(request.arguments));
      if (arguments_ === undefined) {
        throw new Error('execute arguments must be a JSON object.');
      }
      const executionId = details?.toolCall?.callId;
      const executionOptions: CapabilityExecutionOptions = {
        signal: details?.signal,
        ...(executionId === undefined || onOutput === undefined
          ? {}
          : {
              onOutput: (channel, delta) => onOutput(id, executionId, channel, delta),
            }),
      };
      const result = await capabilityRuntime.execute(id, 'local', arguments_, executionOptions);
      return JSON.stringify(result);
    },
  };
}

// Function Tool direta para uma capability do Registry: mesmo Executor,
// hooks e validacao de schema do caminho generico, sem discover/describe.
function capabilityFunctionTool(
  definition: CapabilityDefinition,
  capabilityRuntime: CapabilityRuntime,
  onOutput?: (
    capabilityId: string,
    executionId: string,
    channel: 'stdout' | 'stderr',
    delta: string,
  ) => void | Promise<void>,
): FunctionTool {
  return {
    type: 'function',
    name: materializedToolName(definition.id),
    // Resumo e descricao publicos vem do Registry; o schema e o do manifesto.
    description: `${definition.summary} — ${definition.description}`,
    parameters: definition.schema as FunctionTool['parameters'],
    strict: false,
    needsApproval: async () => false,
    isEnabled: async () => true,
    invoke: async (_runContext, input, details) => {
      const arguments_ = parseObjectInput(input, definition.id);
      const executionId = details?.toolCall?.callId;
      const executionOptions: CapabilityExecutionOptions = {
        signal: details?.signal,
        ...(executionId === undefined || onOutput === undefined
          ? {}
          : {
              onOutput: (channel, delta) => onOutput(definition.id, executionId, channel, delta),
            }),
      };
      const result = await capabilityRuntime.execute(
        definition.id,
        'local',
        arguments_,
        executionOptions,
      );
      return JSON.stringify(result);
    },
  };
}

function discoveryTool(capabilityRuntime: CapabilityRuntime): FunctionTool {
  const parameters = {
    type: 'object',
    properties: {
      query: { type: 'string', description: 'consulta textual' },
      limit: { type: 'integer', minimum: 0, description: 'quantidade maxima de resultados' },
    },
    required: ['query'],
    additionalProperties: false,
  } as FunctionTool['parameters'];

  return {
    type: 'function',
    name: 'discover',
    description: 'encontra capabilities utilizáveis sem executar uma capability',
    parameters,
    strict: false,
    needsApproval: async () => false,
    isEnabled: async () => true,
    invoke: async (_runContext, input) => {
      const request = parseObjectInput(input, 'discover');
      if (Object.keys(request).some((key) => key !== 'query' && key !== 'limit')) {
        throw new Error('discover accepts only query and limit.');
      }
      const query = request.query;
      if (typeof query !== 'string' || !query) {
        throw new Error('discover query must be a string.');
      }
      const limit = request.limit;
      const summaryRequest: CapabilityDiscoveryRequest = {
        ...(query === undefined ? {} : { query }),
      };
      if (
        limit !== undefined &&
        (typeof limit !== 'number' || !Number.isSafeInteger(limit) || limit < 0)
      ) {
        throw new Error('discover limit must be a non-negative integer.');
      }
      if (typeof limit === 'number') {
        summaryRequest.limit = limit;
      }
      const results = await capabilityRuntime.discover(summaryRequest);
      const summaries = results
        .filter(({ type }) => type === 'tool' || type === 'skill')
        .map(({ id, type, summary }) => ({ id, type, summary }));
      return JSON.stringify(summaries satisfies CapabilityDiscoveryResult[]);
    },
  };
}

function listToolsTool(capabilityRuntime: CapabilityRuntime): FunctionTool {
  return {
    type: 'function',
    name: 'list_tools',
    description: 'lista grupos e tools registradas, opcionalmente filtradas por grupo',
    parameters: {
      type: 'object',
      properties: {
        group: { type: 'string', description: 'grupo organizacional opcional' },
      },
      required: [],
      additionalProperties: false,
    } as FunctionTool['parameters'],
    strict: false,
    needsApproval: async () => false,
    isEnabled: async () => true,
    invoke: async (_runContext, input) => {
      const request = parseObjectInput(input, 'list_tools');
      if (Object.keys(request).some((key) => key !== 'group')) {
        throw new Error('list_tools accepts only group.');
      }
      const group = request.group;
      if (group !== undefined && (typeof group !== 'string' || !group)) {
        throw new Error('list_tools group must be a non-empty string.');
      }
      const results = await capabilityRuntime.listTools(group === undefined ? {} : { group });
      return JSON.stringify(results);
    },
  };
}

function describeTool(capabilityRuntime: CapabilityRuntime): FunctionTool {
  return {
    type: 'function',
    name: 'describe',
    description: 'retorna a definição completa de uma capability conhecida pelo id',
    parameters: {
      type: 'object',
      properties: {
        id: { type: 'string', description: 'identificador exato da capability' },
      },
      required: ['id'],
      additionalProperties: false,
    } as FunctionTool['parameters'],
    strict: false,
    needsApproval: async () => false,
    isEnabled: async () => true,
    invoke: async (_runContext, input) => {
      const request = parseObjectInput(input, 'describe');
      const id = request.id;
      if (typeof id !== 'string' || !id) {
        throw new Error('describe id must be a non-empty string.');
      }
      const definition = await capabilityRuntime.getDefinition(id);
      if (definition === undefined || (definition.type !== 'tool' && definition.type !== 'skill')) {
        throw new Error(`Capability "${id}" was not found.`);
      }
      return JSON.stringify(definition);
    },
  };
}

function createAtlasAgent(
  capabilityRuntime: CapabilityRuntime,
  definitions: readonly CapabilityDefinition[],
  onOutput?: (
    capabilityId: string,
    executionId: string,
    channel: 'stdout' | 'stderr',
    delta: string,
  ) => void | Promise<void>,
  model: string | Model = Atlas.model,
): Agent {
  return new Agent({
    name: Atlas.name,
    instructions: loadInstructions(),
    model,
    tools: [
      // Core tools materializadas primeiro; o long tail continua nos base tools.
      ...definitions.map((definition) =>
        capabilityFunctionTool(definition, capabilityRuntime, onOutput),
      ),
      listToolsTool(capabilityRuntime),
      discoveryTool(capabilityRuntime),
      describeTool(capabilityRuntime),
      executionTool(capabilityRuntime, onOutput),
    ],
  });
}

function stringValue(value: unknown): string | undefined {
  return typeof value === 'string' && value ? value : undefined;
}

function recordValue(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function decodedJsonValue(value: unknown): unknown {
  if (typeof value !== 'string') {
    return value;
  }

  try {
    return JSON.parse(value) as unknown;
  } catch {
    return value;
  }
}

function decodedToolOutput(value: unknown): unknown {
  let current = value;
  for (let depth = 0; depth < 4; depth += 1) {
    const decoded = decodedJsonValue(current);
    if (Array.isArray(decoded)) {
      const textParts = decoded
        .map(recordValue)
        .filter(
          (record): record is Record<string, unknown> =>
            record !== undefined &&
            (record.type === undefined ||
              record.type === 'text' ||
              record.type === 'output_text') &&
            typeof record.text === 'string',
        )
        .map((record) => record.text as string);
      if (textParts.length === 0) {
        return decoded;
      }
      current = textParts.join('');
      continue;
    }

    const record = recordValue(decoded);
    if (
      record !== undefined &&
      (record.type === undefined || record.type === 'text' || record.type === 'output_text') &&
      typeof record.text === 'string'
    ) {
      current = record.text;
      continue;
    }
    return decoded;
  }
  return current;
}

function numberValue(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

function executeCall(item: Record<string, unknown>):
  | {
      id: string;
      arguments_: Record<string, unknown>;
    }
  | undefined {
  const request = recordValue(decodedJsonValue(item.arguments));
  const id = stringValue(request?.id);
  const arguments_ = recordValue(decodedJsonValue(request?.arguments));
  if (id === undefined || arguments_ === undefined) {
    return undefined;
  }
  return { id, arguments_ };
}

function shellExecArguments(item: Record<string, unknown>):
  | {
      program: string;
      args: string[];
      cwd?: string;
    }
  | undefined {
  // Aceita o caminho generico (id + arguments) e a tool direta (argumentos crus).
  const direct = recordValue(decodedJsonValue(item.arguments));
  const arguments_ = executeCall(item)?.arguments_ ?? direct;
  const command = stringValue(arguments_?.command);
  if (command === undefined) {
    return undefined;
  }

  const cwd = stringValue(arguments_?.cwd);
  // shell.exec interpreta o comando pelo shell do sistema; o lifecycle expoe a
  // invocacao equivalente para renderizacao e correlacao pelos clientes.
  return {
    program: 'sh',
    args: ['-c', command],
    ...(cwd === undefined ? {} : { cwd }),
  };
}

function toolTarget(toolName: string, item: Record<string, unknown>): string | undefined {
  const arguments_ = executeCall(item)?.arguments_ ?? recordValue(decodedJsonValue(item.arguments));
  if (arguments_ === undefined) {
    return undefined;
  }

  const target =
    toolName === 'filesystem.glob'
      ? arguments_.pattern
      : toolName === 'filesystem.search'
        ? (arguments_.query ?? arguments_.path)
        : (arguments_.path ?? arguments_.target);
  return stringValue(target);
}

function shellExecResult(value: unknown): {
  stdout: string;
  stderr: string;
  exitCode: number;
  durationMs: number;
  status: string;
} {
  const result = recordValue(decodedToolOutput(value));
  const output = recordValue(decodedToolOutput(result?.output)) ?? result;
  const stderr = stringValue(output?.stderr) ?? stringValue(result?.error) ?? '';
  return {
    stdout: stringValue(output?.stdout) ?? '',
    stderr,
    exitCode: numberValue(output?.exit_code) ?? numberValue(result?.exit_code) ?? -1,
    durationMs: numberValue(output?.duration_ms) ?? numberValue(result?.duration_ms) ?? 0,
    status: stringValue(result?.status) ?? stringValue(output?.status) ?? 'failed',
  };
}

function publicText(value: unknown): string | undefined {
  if (typeof value === 'string') {
    return value;
  }

  if (value === undefined) {
    return undefined;
  }

  return JSON.stringify(value);
}

function messageContent(item: Record<string, unknown>): string {
  if (!Array.isArray(item.content)) {
    return '';
  }

  return item.content
    .map((content) => {
      if (content === null || typeof content !== 'object') {
        return '';
      }

      const contentRecord = content as Record<string, unknown>;
      return stringValue(contentRecord.text) ?? stringValue(contentRecord.refusal) ?? '';
    })
    .join('');
}

async function publishRunEvent(
  event: RunStreamEvent,
  onEvent: (event: AtlasRunEvent) => void | Promise<void>,
  fallbackMessageId: string,
  capabilityIdsByCallId: Map<string, string>,
  capabilityIdByToolName: Map<string, string>,
): Promise<void> {
  if (event.type === 'raw_model_stream_event') {
    if (event.data.type !== 'output_text_delta') {
      return;
    }

    await onEvent({
      type: 'message.delta',
      messageId: event.data.itemId ?? fallbackMessageId,
      delta: event.data.delta,
    });
    return;
  }

  if (event.type !== 'run_item_stream_event') {
    return;
  }

  const item = event.item.rawItem as unknown as Record<string, unknown>;
  const agentToolName = stringValue(item.name);
  const toolId = stringValue(item.callId) ?? stringValue(item.call_id);
  // Uma tool materializada carrega o id da capability no proprio nome.
  const directCapabilityId =
    agentToolName === undefined ? undefined : capabilityIdByToolName.get(agentToolName);
  const toolName =
    (agentToolName === 'execute' ? executeCall(item)?.id : directCapabilityId) ??
    (toolId === undefined ? undefined : capabilityIdsByCallId.get(toolId));

  if (agentToolName === 'execute' && toolId !== undefined && toolName !== undefined) {
    capabilityIdsByCallId.set(toolId, toolName);
  }

  if (event.name === 'tool_called') {
    // Discovery e describe são detalhes do Agent, não eventos públicos.
    if (toolName && toolId) {
      if (toolName === 'shell.exec') {
        const arguments_ = shellExecArguments(item);
        if (arguments_ !== undefined) {
          await onEvent({
            type: 'execution.started',
            executionId: toolId,
            capability: 'shell.exec',
            ...arguments_,
            target: 'local',
          });
        }
      } else {
        const target = toolTarget(toolName, item);
        await onEvent({
          type: 'tool.started',
          toolId,
          toolName,
          ...(target === undefined ? {} : { target }),
        });
      }
    }
    return;
  }

  if (event.name === 'tool_output') {
    if (toolName && toolId) {
      // O output público reutiliza o decoding ja aplicado ao lifecycle do shell.
      const output = publicText(decodedToolOutput(item.output));
      if (toolName === 'shell.exec') {
        await onEvent({
          type: 'execution.completed',
          executionId: toolId,
          capability: 'shell.exec',
          ...shellExecResult(item.output),
        });
      } else {
        await onEvent({
          type: 'tool.completed',
          toolId,
          toolName,
          ...(output === undefined ? {} : { output }),
        });
      }
    }
    return;
  }

  if (event.name === 'message_output_created') {
    const content = messageContent(item);
    if (!content) {
      return;
    }

    await onEvent({
      type: 'message.completed',
      messageId: stringValue(item.id) ?? fallbackMessageId,
      content,
    });
  }
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
  const {
    conversationId: _conversationId,
    capabilityRuntime: requestedCapabilityRuntime,
    abortSignal,
    onEvent,
    model: requestedModel,
    coreTools = true,
    ...providerOptions
  } = options;
  const runtime = getAtlasRuntime(providerOptions);
  const sessionId = getConversationId(options);
  const capabilityRuntime = requestedCapabilityRuntime ?? runtime.capabilityRuntime;
  // Cada turno recebe um RetryGuard novo para permitir a mesma chamada apos falha.
  const turnRuntime = new HookableCapabilityRuntime(capabilityRuntime, new RetryGuard());
  // As tools diretas sao montadas sobre o runtime do turno, com hooks e guard.
  const definitions = coreTools ? await loadMaterializedDefinitions(capabilityRuntime) : [];
  const capabilityIdByToolName = new Map(
    definitions.map((definition) => [materializedToolName(definition.id), definition.id]),
  );
  const agent = createAtlasAgent(
    turnRuntime,
    definitions,
    onEvent === undefined
      ? undefined
      : async (capabilityId, executionId, channel, delta) => {
          if (capabilityId === 'shell.exec') {
            await onEvent({
              type: 'execution.output.delta',
              executionId,
              capability: 'shell.exec',
              channel,
              delta,
            });
          }
        },
    requestedModel, // Sem modelo explicito, o Agent base continua sendo usado.
  );
  // A Session guarda o historico; o contexto assincrono aplica seu ID ao request.
  const session = runtime.sessions.get(sessionId) ?? new MemorySession({ sessionId });

  runtime.sessions.set(sessionId, session);

  return withOpenCodeGoAbortSignal(abortSignal, () =>
    withOpenCodeGoSession(sessionId, async () => {
      if (!onEvent) {
        return runtime.runner.run(agent, input, { session });
      }

      const streamedResult = await runtime.runner.run(agent, input, {
        session,
        stream: true,
      });
      const fallbackMessageId = randomUUID();
      const capabilityIdsByCallId = new Map<string, string>();
      for await (const event of streamedResult) {
        await publishRunEvent(
          event,
          onEvent,
          fallbackMessageId,
          capabilityIdsByCallId,
          capabilityIdByToolName,
        );
      }

      // O iterador pode terminar antes da finalizacao interna do Runner.
      await streamedResult.completed;
      return streamedResult;
    }),
  );
}
