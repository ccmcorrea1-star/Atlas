import { readFile, rm, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

import {
  atlasApiKey,
  atlasRuntimeSessionData,
  DEFAULT_ATLAS_CONFIG,
  type AtlasConfig,
  loadAtlasConfig,
} from '../config/index.js';
import { runAtlas, type AtlasRunEvent, type AtlasRunOptions } from '../index.js';
import {
  getOpenCodeGoContextWindow,
  OPENCODE_GO_MODELS,
  OPENCODE_GO_PROVIDER,
  type OpenCodeGoModelDefinition,
} from '../opencode-go.js';
import {
  parseRuntimeMessage,
  runtimeErrorEvent,
  runtimeEvent,
  serializeRuntimeMessage,
  type RuntimeEvent,
  type RuntimeRequest,
  type RuntimeSessionUpdatedData,
  type RuntimeTurnCompletedData,
  type RuntimeTurnRequest,
} from './protocol.js';
import { UnixSocketServer } from './transport/unix/server.js';

export const DEFAULT_RUNTIME_SOCKET_PATH = '/tmp/atlas-runtime.sock';

function defaultRuntimeSocketPath(): string {
  const runtimeDirectory = process.env.XDG_RUNTIME_DIR?.trim();
  return runtimeDirectory === undefined || runtimeDirectory.length === 0
    ? DEFAULT_RUNTIME_SOCKET_PATH
    : resolve(runtimeDirectory, 'atlas-runtime.sock');
}

function configuredRuntimeSocketPath(): string {
  return process.env.ATLAS_RUNTIME_SOCKET ?? defaultRuntimeSocketPath();
}

function runtimePidPath(socketPath: string): string {
  return `${socketPath}.pid`;
}

async function writeRuntimePid(socketPath: string): Promise<void> {
  await writeFile(runtimePidPath(socketPath), `${process.pid}\n`, {
    encoding: 'utf8',
    mode: 0o600,
  });
}

async function removeRuntimePid(socketPath: string): Promise<void> {
  const pidPath = runtimePidPath(socketPath);
  try {
    const pid = (await readFile(pidPath, 'utf8')).trim();
    if (pid === String(process.pid)) {
      await rm(pidPath, { force: true });
    }
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw error;
    }
  }
}

export type RuntimeServerOptions = {
  socketPath?: string;
  runOptions?: Omit<AtlasRunOptions, 'conversationId' | 'onEvent'>;
  // Configuração global já resolvida pelo loader canônico.
  atlasConfig?: AtlasConfig;
};

export class AtlasRuntimeServer {
  public readonly socketPath: string;

  private readonly runOptions: Omit<AtlasRunOptions, 'conversationId' | 'onEvent'>;
  private readonly contextWindow: number | undefined;
  private readonly sessionData: RuntimeSessionUpdatedData;
  private readonly atlasModel: string;
  private readonly transport: UnixSocketServer;
  private readonly conversationQueues = new Map<string, Promise<void>>();
  private readonly activeTurns = new Map<string, Map<string, AbortController>>();

  public constructor(options: RuntimeServerOptions = {}) {
    this.socketPath = options.socketPath ?? configuredRuntimeSocketPath();
    this.runOptions = options.runOptions ?? {};
    // Provider/model da sessão são definidos pela configuração global do Atlas.
    this.sessionData = atlasRuntimeSessionData(options.atlasConfig ?? DEFAULT_ATLAS_CONFIG);
    this.atlasModel = `${OPENCODE_GO_PROVIDER}/${this.sessionData.model}`;
    // Modelos fora do registro inicial usam o endpoint de responses por padrão.
    const registry = OPENCODE_GO_MODELS as Readonly<Record<string, OpenCodeGoModelDefinition>>;
    this.runOptions = Object.hasOwn(registry, this.sessionData.model)
      ? this.runOptions
      : {
          ...this.runOptions,
          models: {
            ...this.runOptions.models,
            [this.sessionData.model]: { endpoint: 'responses' },
          },
        };
    this.contextWindow = getOpenCodeGoContextWindow(this.runOptions, this.atlasModel);
    this.transport = new UnixSocketServer({
      socketPath: this.socketPath,
      onLine: (line, send) => this.handleLine(line, send),
    });
  }

  public listen(): Promise<void> {
    return this.transport.listen();
  }

  public close(): Promise<void> {
    return this.transport.close();
  }

  private handleLine(line: string, send: (payload: string) => void): Promise<void> {
    let message: RuntimeRequest;
    try {
      message = parseRuntimeMessage(line);
    } catch (error) {
      send(
        serializeRuntimeMessage(
          runtimeErrorEvent(error instanceof Error ? error.message : String(error)),
        ),
      );
      return Promise.resolve();
    }

    if (message.type === 'turn.cancel') {
      return this.handleCancel(message, send);
    }

    const request = message;
    const controller = new AbortController();
    const conversationTurns = this.activeTurns.get(request.conversation_id) ?? new Map();
    conversationTurns.set(request.request_id, controller);
    this.activeTurns.set(request.conversation_id, conversationTurns);
    const previous = this.conversationQueues.get(request.conversation_id) ?? Promise.resolve();
    const current = previous
      .catch(() => undefined)
      .then(() => this.handleTurn(request, send, controller.signal))
      .catch((error) => {
        // A fila nao pode deixar uma falha inesperada sem um evento terminal.
        send(
          serializeRuntimeMessage(
            runtimeErrorEvent(error instanceof Error ? error.message : String(error), request),
          ),
        );
      });
    this.conversationQueues.set(request.conversation_id, current);
    const clearQueue = () => {
      if (this.conversationQueues.get(request.conversation_id) === current) {
        this.conversationQueues.delete(request.conversation_id);
      }
      const turns = this.activeTurns.get(request.conversation_id);
      if (turns?.get(request.request_id) === controller) {
        turns.delete(request.request_id);
        if (turns.size === 0) {
          this.activeTurns.delete(request.conversation_id);
        }
      }
    };
    void current.then(clearQueue, clearQueue);
    return current;
  }

  private handleCancel(
    request: Extract<RuntimeRequest, { type: 'turn.cancel' }>,
    send: (payload: string) => void,
  ): Promise<void> {
    const controller = this.activeTurns.get(request.conversation_id)?.get(request.request_id);
    if (controller === undefined) {
      send(
        serializeRuntimeMessage(
          runtimeErrorEvent(`No active turn exists for request "${request.request_id}".`, request),
        ),
      );
      return Promise.resolve();
    }
    controller.abort(new Error('turn cancelled by client'));
    return Promise.resolve();
  }

  private async handleTurn(
    request: RuntimeTurnRequest,
    send: (payload: string) => void,
    abortSignal: AbortSignal,
  ): Promise<void> {
    const publish = (message: RuntimeEvent) => {
      send(serializeRuntimeMessage(message));
    };
    const sessionData: RuntimeSessionUpdatedData = this.sessionData;
    publish(runtimeEvent(request, 'session.updated', sessionData));
    publish(runtimeEvent(request, 'turn.started'));

    let messageId: string | undefined;
    try {
      if (abortSignal.aborted) {
        throw new Error('turn cancelled by client');
      }
      const result = await runAtlas(request.input, {
        ...this.runOptions,
        abortSignal,
        // O provider é fixo nesta fase; o modelo vem da configuração global.
        model: this.atlasModel,
        conversationId: request.conversation_id,
        onEvent: (event) => {
          messageId = eventMessageId(event) ?? messageId;
          publish(atlasEvent(request, event));
        },
      });
      const content =
        typeof result.finalOutput === 'string'
          ? result.finalOutput
          : JSON.stringify(result.finalOutput);
      const context =
        this.contextWindow === undefined
          ? undefined
          : {
              used_tokens: result.runContext.usage.inputTokens,
              context_window: this.contextWindow,
            };
      if (context !== undefined) {
        publish(runtimeEvent(request, 'context.updated', context));
      }
      const completedData: RuntimeTurnCompletedData = {
        ...(messageId === undefined ? {} : { message_id: messageId }),
        content: content ?? '',
        // A UI recebe o uso agregado sem precisar conhecer o Agent SDK.
        ...(context === undefined ? {} : { context }),
      };
      publish(runtimeEvent(request, 'turn.completed', completedData));
    } catch (error) {
      publish(runtimeErrorEvent(error instanceof Error ? error.message : String(error), request));
    }
  }
}

function eventMessageId(event: AtlasRunEvent): string | undefined {
  return 'messageId' in event ? event.messageId : undefined;
}

function atlasEvent(request: RuntimeTurnRequest, event: AtlasRunEvent): RuntimeEvent {
  switch (event.type) {
    case 'message.delta':
      return runtimeEvent(request, 'message.delta', {
        message_id: event.messageId,
        delta: event.delta,
      });
    case 'message.completed':
      return runtimeEvent(request, 'message.completed', {
        message_id: event.messageId,
        content: event.content,
      });
    case 'tool.started':
      return runtimeEvent(request, 'tool.started', {
        tool_id: event.toolId,
        name: event.toolName,
      });
    case 'tool.completed':
      return runtimeEvent(request, 'tool.completed', {
        tool_id: event.toolId,
        name: event.toolName,
        ...(event.output === undefined ? {} : { output: event.output }),
      });
    case 'execution.started':
      return runtimeEvent(request, 'execution.started', {
        execution_id: event.executionId,
        capability: event.capability,
        program: event.program,
        args: event.args,
        ...(event.cwd === undefined ? {} : { cwd: event.cwd }),
        ...(event.target === undefined ? {} : { target: event.target }),
      });
    case 'execution.output.delta':
      return runtimeEvent(request, 'execution.output.delta', {
        execution_id: event.executionId,
        capability: event.capability,
        channel: event.channel,
        delta: event.delta,
      });
    case 'execution.completed':
      return runtimeEvent(request, 'execution.completed', {
        execution_id: event.executionId,
        capability: event.capability,
        stdout: event.stdout,
        stderr: event.stderr,
        exit_code: event.exitCode,
        duration_ms: event.durationMs,
        status: event.status,
      });
  }
}

function isMainModule(): boolean {
  const entrypoint = process.argv[1];
  return entrypoint !== undefined && import.meta.url === pathToFileURL(resolve(entrypoint)).href;
}

async function runServer(): Promise<void> {
  // O Runtime só inicia com uma configuração global válida e com a API key resolvida.
  const { config, path, source } = await loadAtlasConfig();
  // O ambiente é apenas override; a key pode vir exclusivamente do config.json.
  const { apiKey } = atlasApiKey(config, process.env);
  const server = new AtlasRuntimeServer({
    atlasConfig: config,
    runOptions: {
      ...{ apiKey },
      // ATLAS_CORE_TOOLS=0 mantem apenas o caminho generico, para A/B local.
      coreTools: process.env.ATLAS_CORE_TOOLS !== '0',
    },
  });
  await server.listen();
  try {
    await writeRuntimePid(server.socketPath);
  } catch (error) {
    await server.close();
    throw error;
  }

  const shutdown = async () => {
    try {
      await server.close();
      await removeRuntimePid(server.socketPath);
      process.exit(0);
    } catch (error) {
      console.error(error);
      process.exit(1);
    }
  };
  process.once('SIGINT', shutdown);
  process.once('SIGTERM', shutdown);
  console.log(`Atlas Runtime listening on ${server.socketPath}`);
  // Sem arquivo, os defaults vigentes são usados e o caminho é reportado.
  console.log(`Atlas config: ${path} (${source})`);
}

if (isMainModule()) {
  try {
    await runServer();
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'EADDRINUSE') {
      console.error(
        `Atlas Runtime already uses ${configuredRuntimeSocketPath()}. Use "atlas server restart" to restart it.`,
      );
    } else {
      console.error(error);
    }
    process.exitCode = 1;
  }
}
