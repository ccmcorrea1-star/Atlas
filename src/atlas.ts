import { createHash, randomUUID } from 'node:crypto';

import {
  Agent,
  MemorySession,
  Runner,
  type FunctionTool,
  type RunStreamEvent,
} from '@openai/agents';

import {
  OpenCodeGoProvider,
  type OpenCodeGoProviderOptions,
  OPENCODE_GO_MODEL,
  withOpenCodeGoAbortSignal,
  withOpenCodeGoSession,
} from './opencode-go.js';
import {
  createCapabilityRuntime,
  type CapabilityDefinition,
  type CapabilityDiscoveryRequest,
  type CapabilityDiscoveryResult,
  type CapabilityExecutionOptions,
  type CapabilityRuntime,
} from './capability-runtime.js';

const ATLAS_INSTRUCTIONS =
  'You are Atlas, a pragmatic coding agent. Give clear, concise answers and do not claim work you did not perform.';

// O Agent base define a identidade; cada turno recebe um clone com capabilities isoladas.
export const Atlas = new Agent({
  name: 'Atlas',
  instructions: ATLAS_INSTRUCTIONS,
  model: OPENCODE_GO_MODEL,
});

export type AtlasRunOptions = OpenCodeGoProviderOptions & {
  // O signal permite ao Runtime interromper o request de modelo ativo.
  abortSignal?: AbortSignal;
  // O ID explicito permite continuar a mesma conversa entre chamadas.
  conversationId?: string;
  capabilityRuntime?: CapabilityRuntime;
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
      capability: 'process.exec';
      program: string;
      args: string[];
      cwd?: string;
      target?: string;
    }
  | {
      type: 'execution.output.delta';
      executionId: string;
      capability: 'process.exec';
      channel: 'stdout' | 'stderr';
      delta: string;
    }
  | {
      type: 'execution.completed';
      executionId: string;
      capability: 'process.exec';
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
  materializedCapabilitiesByConversation: Map<string, Map<string, CapabilityDefinition>>;
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
    runner: createRunner(options),
    sessions: new Map(),
    capabilityRuntime: createCapabilityRuntime(),
    materializedCapabilitiesByConversation: new Map(),
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

function schemaForTool(definition: CapabilityDefinition): FunctionTool['parameters'] {
  return definition.schema as FunctionTool['parameters'];
}

function providerToolName(capabilityId: string): string {
  // O digest diferencia IDs que produzem a mesma forma sanitizada.
  const readableName = capabilityId.replace(/[^a-zA-Z0-9_-]/g, '_') || 'capability';
  const suffix = createHash('sha256').update(capabilityId).digest('hex').slice(0, 16);
  return `${readableName}_${suffix}`;
}

function materializeTool(
  definition: CapabilityDefinition,
  capabilityRuntime: CapabilityRuntime,
  toolName: string,
  capabilityIdsByToolName: ReadonlyMap<string, string>,
  onOutput?: (
    capabilityId: string,
    executionId: string,
    channel: 'stdout' | 'stderr',
    delta: string,
  ) => void | Promise<void>,
): FunctionTool {
  return {
    type: 'function',
    name: toolName,
    description: definition.description,
    parameters: schemaForTool(definition),
    strict: false,
    needsApproval: async () => false,
    isEnabled: async () => true,
    invoke: async (_runContext, input, details) => {
      if (capabilityIdsByToolName.get(toolName) !== definition.id) {
        throw new Error(`Unknown provider tool mapping for capability "${definition.id}".`);
      }
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

function discoveryTool(
  capabilityRuntime: CapabilityRuntime,
  addTool: (definition: CapabilityDefinition) => void,
): FunctionTool {
  const parameters = {
    type: 'object',
    properties: {
      path: { type: 'string', description: 'caminho hierarquico opcional' },
      query: { type: 'string', description: 'consulta textual opcional' },
    },
    required: [],
    additionalProperties: false,
  } as FunctionTool['parameters'];

  return {
    type: 'function',
    name: 'discover',
    description: 'encontra grupos e capabilities disponíveis sem executar uma capability',
    parameters,
    strict: false,
    needsApproval: async () => false,
    isEnabled: async () => true,
    invoke: async (_runContext, input) => {
      const request = parseObjectInput(input, 'discover') as CapabilityDiscoveryRequest;
      const results = await capabilityRuntime.discover(request);
      for (const result of results) {
        if (result.type !== 'tool') {
          continue;
        }
        const definition = await capabilityRuntime.getDefinition(result.id);
        if (definition !== undefined) {
          addTool(definition);
        }
      }
      return JSON.stringify(results satisfies CapabilityDiscoveryResult[]);
    },
  };
}

type AtlasAgent = {
  agent: Agent;
  capabilityIdsByToolName: ReadonlyMap<string, string>;
};

function createAtlasAgent(
  capabilityRuntime: CapabilityRuntime,
  rootGroups: readonly CapabilityDiscoveryResult[],
  materializedDefinitions: Map<string, CapabilityDefinition>,
  onOutput?: (
    capabilityId: string,
    executionId: string,
    channel: 'stdout' | 'stderr',
    delta: string,
  ) => void | Promise<void>,
): AtlasAgent {
  const catalog = rootGroups.map(({ id, summary }) => `${id} - ${summary}`).join('\n');
  const agent = Atlas.clone({
    instructions: `${ATLAS_INSTRUCTIONS}\n\nAvailable capability groups:\n${catalog}`,
    tools: [],
  });
  const materialized = new Set<string>();
  const capabilityIdsByToolName = new Map<string, string>();
  const addTool = (definition: CapabilityDefinition) => {
    if (materialized.has(definition.id)) {
      return;
    }
    materialized.add(definition.id);
    const toolName = providerToolName(definition.id);
    const mappedCapabilityId = capabilityIdsByToolName.get(toolName);
    if (mappedCapabilityId !== undefined && mappedCapabilityId !== definition.id) {
      throw new Error(
        `Provider tool name collision between capabilities "${mappedCapabilityId}" and "${definition.id}".`,
      );
    }
    capabilityIdsByToolName.set(toolName, definition.id);
    materializedDefinitions.set(definition.id, definition);
    agent.tools.push(
      materializeTool(definition, capabilityRuntime, toolName, capabilityIdsByToolName, onOutput),
    );
  };
  for (const definition of materializedDefinitions.values()) {
    addTool(definition);
  }
  const discover = discoveryTool(capabilityRuntime, addTool);
  agent.tools.push(discover);
  return { agent, capabilityIdsByToolName };
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
  const decoded = decodedJsonValue(value);
  if (Array.isArray(decoded)) {
    const text = decoded
      .map(recordValue)
      .map((record) => record?.text)
      .find((text): text is string => typeof text === 'string');
    return text === undefined ? decoded : decodedJsonValue(text);
  }

  const record = recordValue(decoded);
  if (typeof record?.text === 'string') {
    return decodedJsonValue(record.text);
  }
  return decoded;
}

function numberValue(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

function processExecArguments(item: Record<string, unknown>):
  | {
      program: string;
      args: string[];
      cwd?: string;
    }
  | undefined {
  const arguments_ = recordValue(decodedJsonValue(item.arguments));
  const program = stringValue(arguments_?.program);
  if (program === undefined) {
    return undefined;
  }

  const args = Array.isArray(arguments_?.args)
    ? arguments_.args.filter((argument): argument is string => typeof argument === 'string')
    : [];
  const cwd = stringValue(arguments_?.cwd);
  return {
    program,
    args,
    ...(cwd === undefined ? {} : { cwd }),
  };
}

function processExecResult(value: unknown): {
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
  capabilityIdsByToolName: ReadonlyMap<string, string>,
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
  const providerToolName = stringValue(item.name);
  const toolName =
    providerToolName === undefined
      ? undefined
      : (capabilityIdsByToolName.get(providerToolName) ?? providerToolName);
  const toolId = stringValue(item.callId) ?? stringValue(item.call_id);

  if (event.name === 'tool_called') {
    // Discovery is an Agent implementation detail, not a client-facing tool.
    if (toolName && providerToolName !== 'discover' && toolId) {
      if (toolName === 'process.exec') {
        const arguments_ = processExecArguments(item);
        if (arguments_ !== undefined) {
          await onEvent({
            type: 'execution.started',
            executionId: toolId,
            capability: 'process.exec',
            ...arguments_,
            target: 'local',
          });
        }
      } else {
        await onEvent({ type: 'tool.started', toolId, toolName });
      }
    }
    return;
  }

  if (event.name === 'tool_output') {
    if (toolName && providerToolName !== 'discover' && toolId) {
      const output = publicText(item.output);
      if (toolName === 'process.exec') {
        await onEvent({
          type: 'execution.completed',
          executionId: toolId,
          capability: 'process.exec',
          ...processExecResult(item.output),
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
    ...providerOptions
  } = options;
  const runtime = getAtlasRuntime(providerOptions);
  const sessionId = getConversationId(options);
  const capabilityRuntime = requestedCapabilityRuntime ?? runtime.capabilityRuntime;
  const rootGroups = await capabilityRuntime.discover();
  // As tools descobertas sobrevivem à recriação do Agent entre turnos da conversa.
  const materializedDefinitions =
    runtime.materializedCapabilitiesByConversation.get(sessionId) ?? new Map();
  runtime.materializedCapabilitiesByConversation.set(sessionId, materializedDefinitions);
  const { agent, capabilityIdsByToolName } = createAtlasAgent(
    capabilityRuntime,
    rootGroups,
    materializedDefinitions,
    onEvent === undefined
      ? undefined
      : async (capabilityId, executionId, channel, delta) => {
          if (capabilityId === 'process.exec') {
            await onEvent({
              type: 'execution.output.delta',
              executionId,
              capability: 'process.exec',
              channel,
              delta,
            });
          }
        },
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
      for await (const event of streamedResult) {
        await publishRunEvent(event, onEvent, fallbackMessageId, capabilityIdsByToolName);
      }

      // O iterador pode terminar antes da finalizacao interna do Runner.
      await streamedResult.completed;
      return streamedResult;
    }),
  );
}
