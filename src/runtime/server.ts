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
import {
  runAtlas,
  resetAtlasConversation,
  type AtlasApprovalDecision,
  type AtlasRunEvent,
  type AtlasRunOptions,
} from '../index.js';
import {
  getOpenCodeGoContextWindow,
  OPENCODE_GO_MODELS,
  OPENCODE_GO_PROVIDER,
  type OpenCodeGoModelDefinition,
} from '../providers/opencode-go.js';
import {
  parseRuntimeMessage,
  runtimeErrorEvent,
  runtimeEvent,
  serializeRuntimeMessage,
  type RuntimeCommandCompletedData,
  type RuntimeCommandRequest,
  type RuntimeEvent,
  type RuntimeRequest,
  type RuntimeSession,
  type RuntimeSessionUpdatedData,
  type RuntimeTurnCompletedData,
  type RuntimeTurnRequest,
} from './protocol.js';
import { OperationStore, type HostOperation } from '../host/operations.js';
import { UnixSocketServer } from './transport/unix/server.js';

export const DEFAULT_RUNTIME_SOCKET_PATH = '/tmp/atlas-runtime.sock';

type AtlasRunResult = Awaited<ReturnType<typeof runAtlas>>;
type RequestUsageEntry = NonNullable<
  AtlasRunResult['runContext']['usage']['requestUsageEntries']
>[number];

type TurnUsage = {
  currentRequest: RequestUsageEntry | undefined;
  entries: RequestUsageEntry[];
  inputTokens: number;
  outputTokens: number;
  requests: number;
};

type PendingApprovalGroup = {
  request: RuntimeTurnRequest;
  approvalIds: Set<string>;
  decisions: Map<string, AtlasApprovalDecision>;
  resolve: ((decisions: AtlasApprovalDecision[]) => void) | undefined;
  reject: ((error: Error) => void) | undefined;
  abortCleanup: (() => void) | undefined;
};

function isTokenCount(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
}

function getTurnUsage(result: AtlasRunResult): TurnUsage {
  const usage = result.runContext.usage;
  const entries = (usage.requestUsageEntries ?? []).filter(
    (entry): entry is RequestUsageEntry =>
      isTokenCount(entry.inputTokens) && isTokenCount(entry.outputTokens),
  );
  return {
    currentRequest: entries.at(-1),
    entries,
    inputTokens: usage.inputTokens,
    outputTokens: usage.outputTokens,
    requests: usage.requests,
  };
}

function formatDebugTokens(tokens: number): string {
  if (tokens < 1_000) {
    return String(tokens);
  }
  if (tokens < 1_000_000) {
    const value = Number((tokens / 1_000).toFixed(1));
    return `${value}k`;
  }
  const value = Number((tokens / 1_000_000).toFixed(1));
  return `${value}m`;
}

function debugTurnUsage(usage: TurnUsage): void {
  if (process.env.ATLAS_DEBUG !== '1') {
    return;
  }

  const requestLines = usage.entries.map(
    (entry, index) =>
      `request ${index + 1} · in ${formatDebugTokens(entry.inputTokens)} · out ${formatDebugTokens(entry.outputTokens)}`,
  );
  const current = usage.currentRequest;
  const currentLine =
    current === undefined
      ? 'context current: unavailable (request usage unavailable)'
      : `context current: ${formatDebugTokens(current.inputTokens)}`;
  console.debug(
    [
      ...requestLines,
      currentLine,
      `turn input: ${formatDebugTokens(usage.inputTokens)}`,
      `turn output: ${formatDebugTokens(usage.outputTokens)}`,
      `requests: ${usage.requests}`,
    ].join('\n'),
  );
}

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
  operationStore?: OperationStore;
  operationStorePath?: string;
};

export class AtlasRuntimeServer {
  public readonly socketPath: string;

  private readonly runOptions: Omit<AtlasRunOptions, 'conversationId' | 'onEvent'>;
  private readonly contextWindow: number | undefined;
  private readonly sessionData: RuntimeSessionUpdatedData;
  private readonly atlasModel: string;
  private readonly atlasConfig: AtlasConfig;
  private readonly operationStore: OperationStore | undefined;
  private readonly transport: UnixSocketServer;
  private readonly conversationQueues = new Map<string, Promise<void>>();
  private readonly activeTurns = new Map<string, Map<string, AbortController>>();
  private readonly recoveryPromises = new Map<string, Promise<HostOperation>>();
  private readonly pendingApprovals = new Map<string, PendingApprovalGroup>();

  public constructor(options: RuntimeServerOptions = {}) {
    this.socketPath = options.socketPath ?? configuredRuntimeSocketPath();
    this.runOptions = options.runOptions ?? {};
    // Provider/model da sessão são definidos pela configuração global do Atlas.
    this.atlasConfig = options.atlasConfig ?? DEFAULT_ATLAS_CONFIG;
    this.operationStore =
      options.operationStore ??
      (options.operationStorePath === undefined
        ? undefined
        : new OperationStore(options.operationStorePath));
    this.sessionData = atlasRuntimeSessionData(this.atlasConfig);
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

  public async listen(): Promise<void> {
    await this.transport.listen();
    await this.schedulePendingRecovery();
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
    if (message.type === 'command.request') {
      return this.handleCommand(message, send);
    }
    if (message.type === 'approval.respond') {
      return this.handleApprovalResponse(message, send);
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

  private async handleCommand(
    request: RuntimeCommandRequest,
    send: (payload: string) => void,
  ): Promise<void> {
    const active = this.activeTurns.get(request.conversation_id);
    const abortActive = () => {
      for (const controller of active?.values() ?? []) {
        controller.abort(new Error(`runtime command ${request.command}`));
      }
    };

    let message: string;
    let statusOverride: RuntimeSession['status'] | undefined;
    if (request.command === 'new') {
      abortActive();
      this.conversationQueues.delete(request.conversation_id);
      resetAtlasConversation(request.conversation_id);
      message = 'Nova sessão iniciada.';
    } else if (request.command === 'stop') {
      abortActive();
      message =
        active === undefined || active.size === 0 ? 'Nenhum turno ativo.' : 'Turno cancelado.';
      statusOverride = active === undefined || active.size === 0 ? 'idle' : 'cancelled';
    } else {
      message = 'Sessão ativa.';
    }

    const session = this.sessionSnapshot(request.conversation_id, statusOverride);
    const data: RuntimeCommandCompletedData = {
      command: request.command,
      message,
      session,
    };
    send(serializeRuntimeMessage(runtimeEvent(request, 'command.completed', data)));
  }

  private handleApprovalResponse(
    request: Extract<RuntimeRequest, { type: 'approval.respond' }>,
    send: (payload: string) => void,
  ): Promise<void> {
    const group = this.pendingApprovals.get(request.approval_id);
    if (group === undefined || group.request.conversation_id !== request.conversation_id) {
      send(
        serializeRuntimeMessage(
          runtimeErrorEvent(
            `No approval is currently waiting for "${request.approval_id}".`,
            request,
          ),
        ),
      );
      return Promise.resolve();
    }
    if (group.decisions.has(request.approval_id)) {
      send(
        serializeRuntimeMessage(
          runtimeErrorEvent(`Approval "${request.approval_id}" was already resolved.`, request),
        ),
      );
      return Promise.resolve();
    }

    group.decisions.set(request.approval_id, {
      approvalId: request.approval_id,
      approved: request.approved,
      ...(request.comment === undefined ? {} : { comment: request.comment }),
    });
    send(
      serializeRuntimeMessage(
        runtimeEvent(request, 'approval.resolved', {
          approval_id: request.approval_id,
          approved: request.approved,
          ...(request.comment === undefined ? {} : { comment: request.comment }),
        }),
      ),
    );

    if (group.decisions.size === group.approvalIds.size) {
      this.finishPendingApproval(group);
      group.resolve?.([...group.decisions.values()]);
    }
    return Promise.resolve();
  }

  private waitForApprovals(
    request: RuntimeTurnRequest,
    approvalIds: Iterable<string>,
    abortSignal: AbortSignal,
  ): Promise<AtlasApprovalDecision[]> {
    const ids = new Set(approvalIds);
    if (ids.size === 0) {
      return Promise.resolve([]);
    }
    const group: PendingApprovalGroup = {
      request,
      approvalIds: ids,
      decisions: new Map(),
      resolve: undefined,
      reject: undefined,
      abortCleanup: undefined,
    };
    for (const approvalId of ids) {
      this.pendingApprovals.set(approvalId, group);
    }

    return new Promise<AtlasApprovalDecision[]>((resolve, reject) => {
      group.resolve = resolve;
      group.reject = reject;
      const onAbort = () => {
        this.finishPendingApproval(group);
        reject(new Error('Approval was cancelled with the active turn.'));
      };
      abortSignal.addEventListener('abort', onAbort, { once: true });
      group.abortCleanup = () => abortSignal.removeEventListener('abort', onAbort);
      if (abortSignal.aborted) {
        onAbort();
      }
    });
  }

  private finishPendingApproval(group: PendingApprovalGroup): void {
    for (const approvalId of group.approvalIds) {
      if (this.pendingApprovals.get(approvalId) === group) {
        this.pendingApprovals.delete(approvalId);
      }
    }
    group.abortCleanup?.();
    group.abortCleanup = undefined;
  }

  private sessionSnapshot(
    conversationId: string,
    statusOverride?: RuntimeSession['status'],
  ): RuntimeSession {
    const active = this.activeTurns.get(conversationId);
    const activeRequestId = active?.keys().next().value as string | undefined;
    return {
      id: conversationId,
      model: this.sessionData.model,
      provider: this.sessionData.provider,
      status: statusOverride ?? (activeRequestId === undefined ? 'idle' : 'running'),
      ...(activeRequestId === undefined ? {} : { active_request_id: activeRequestId }),
    };
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
    const completedOperation = await this.completedOperation(request);
    if (completedOperation !== undefined) {
      await this.publishRecoveredOperation(request, completedOperation, publish);
      return;
    }
    const operation = await this.pendingOperation(request);
    if (operation !== undefined) {
      await this.operationStore?.update(operation.operation_id, { state: 'resuming' });
      publish(
        runtimeEvent(request, 'operation.resuming', {
          operation_id: operation.operation_id,
        }),
      );
      const recovery = this.recoveryPromises.get(operation.operation_id);
      if (recovery !== undefined) {
        try {
          const recovered = await recovery;
          await this.publishRecoveredOperation(request, recovered, publish, true);
        } catch (error) {
          publish(
            runtimeErrorEvent(error instanceof Error ? error.message : String(error), request),
          );
        }
        return;
      }
    }
    const input = operation === undefined ? request.input : resumeInput(operation);
    const sessionData: RuntimeSessionUpdatedData = this.sessionData;
    publish(runtimeEvent(request, 'session.updated', sessionData));
    publish(runtimeEvent(request, 'turn.started'));

    let messageId: string | undefined;
    const messageContents = new Map<string, string>();
    const publishCancelled = () => {
      const content = messageId === undefined ? '' : (messageContents.get(messageId) ?? '');
      publish(
        runtimeEvent(request, 'turn.cancelled', {
          content,
          ...(messageId === undefined ? {} : { message_id: messageId }),
        }),
      );
    };
    try {
      if (abortSignal.aborted) {
        throw new Error('turn cancelled by client');
      }
      let resumeState: string | undefined;
      let approvalDecisions: readonly AtlasApprovalDecision[] | undefined;
      let result: Awaited<ReturnType<typeof runAtlas>>;
      for (;;) {
        const requestedApprovals = new Set<string>();
        result = await runAtlas(input, {
          ...this.runOptions,
          abortSignal,
          // O provider é fixo nesta fase; o modelo vem da configuração global.
          model: this.atlasModel,
          atlasConfig: this.atlasConfig,
          conversationId: request.conversation_id,
          ...(resumeState === undefined && request.attachments !== undefined
            ? { attachments: request.attachments }
            : {}),
          ...(resumeState === undefined ? {} : { resumeState }),
          ...(approvalDecisions === undefined ? {} : { approvalDecisions }),
          onEvent: (event) => {
            messageId = eventMessageId(event) ?? messageId;
            if (event.type === 'message.delta') {
              messageContents.set(
                event.messageId,
                `${messageContents.get(event.messageId) ?? ''}${event.delta}`,
              );
            } else if (event.type === 'message.completed') {
              messageContents.set(event.messageId, event.content);
            } else if (event.type === 'approval.requested') {
              requestedApprovals.add(event.approvalId);
            }
            publish(atlasEvent(request, event));
          },
        });
        if (result.interruptions.length === 0) {
          break;
        }
        if (requestedApprovals.size !== result.interruptions.length) {
          throw new Error('Runtime received an approval without a stable identifier.');
        }
        approvalDecisions = await this.waitForApprovals(request, requestedApprovals, abortSignal);
        resumeState = result.state.toString();
      }
      if (abortSignal.aborted) {
        publishCancelled();
        return;
      }
      const content =
        typeof result.finalOutput === 'string'
          ? result.finalOutput
          : JSON.stringify(result.finalOutput);
      const usage = getTurnUsage(result);
      debugTurnUsage(usage);
      const context =
        this.contextWindow === undefined || usage.currentRequest === undefined
          ? undefined
          : {
              used_tokens: usage.currentRequest.inputTokens,
              context_window: this.contextWindow,
            };
      if (context !== undefined) {
        publish(runtimeEvent(request, 'context.updated', context));
      }
      const completedData: RuntimeTurnCompletedData = {
        ...(messageId === undefined ? {} : { message_id: messageId }),
        content: content ?? '',
        // A UI recebe somente o contexto da última chamada do modelo.
        ...(context === undefined ? {} : { context }),
      };
      if (operation !== undefined) {
        await this.operationStore?.update(operation.operation_id, {
          state: 'resumed',
          result: {
            content: completedData.content,
            ...(messageId === undefined ? {} : { message_id: messageId }),
          },
        });
        publish(
          runtimeEvent(request, 'operation.resumed', {
            operation_id: operation.operation_id,
          }),
        );
      }
      publish(runtimeEvent(request, 'turn.completed', completedData));
    } catch (error) {
      if (abortSignal.aborted) {
        publishCancelled();
        return;
      }
      if (operation !== undefined) {
        await this.operationStore?.update(operation.operation_id, {
          state: 'failed',
          error: error instanceof Error ? error.message : String(error),
        });
      }
      publish(runtimeErrorEvent(error instanceof Error ? error.message : String(error), request));
    }
  }

  private async schedulePendingRecovery(): Promise<void> {
    if (this.operationStore === undefined) {
      return;
    }
    for (const operation of await this.operationStore.pending()) {
      const recovery = this.recoverOperation(operation);
      this.recoveryPromises.set(operation.operation_id, recovery);
      void recovery.then(
        () => this.recoveryPromises.delete(operation.operation_id),
        () => this.recoveryPromises.delete(operation.operation_id),
      );
    }
  }

  private async recoverOperation(operation: HostOperation): Promise<HostOperation> {
    try {
      await this.operationStore?.update(operation.operation_id, { state: 'resuming' });
      const result = await runAtlas(resumeInput(operation), {
        ...this.runOptions,
        model: this.atlasModel,
        atlasConfig: this.atlasConfig,
        conversationId: operation.conversation_id,
      });
      const content =
        typeof result.finalOutput === 'string'
          ? result.finalOutput
          : JSON.stringify(result.finalOutput);
      await this.operationStore?.update(operation.operation_id, {
        state: 'resumed',
        result: { content: content ?? '' },
      });
      return (
        (await this.operationStore?.get(operation.operation_id)) ?? {
          ...operation,
          state: 'resumed',
          result: { content: content ?? '' },
        }
      );
    } catch (error) {
      await this.operationStore?.update(operation.operation_id, {
        state: 'failed',
        error: error instanceof Error ? error.message : String(error),
      });
      throw error;
    }
  }

  private async completedOperation(
    request: RuntimeTurnRequest,
  ): Promise<HostOperation | undefined> {
    const operation = await this.operationStore?.find(request.conversation_id, request.request_id);
    return operation?.state === 'resumed' && operation.result !== undefined ? operation : undefined;
  }

  private async publishRecoveredOperation(
    request: RuntimeTurnRequest,
    operation: HostOperation,
    publish: (message: RuntimeEvent) => void,
    alreadyResuming = false,
  ): Promise<void> {
    if (!alreadyResuming) {
      publish(
        runtimeEvent(request, 'operation.resuming', {
          operation_id: operation.operation_id,
        }),
      );
    }
    publish(runtimeEvent(request, 'session.updated', this.sessionData));
    publish(runtimeEvent(request, 'turn.started'));
    const result = operation.result ?? { content: '' };
    if (result.message_id !== undefined) {
      publish(
        runtimeEvent(request, 'message.completed', {
          message_id: result.message_id,
          content: result.content,
        }),
      );
    }
    publish(
      runtimeEvent(request, 'operation.resumed', {
        operation_id: operation.operation_id,
      }),
    );
    publish(
      runtimeEvent(request, 'turn.completed', {
        ...(result.message_id === undefined ? {} : { message_id: result.message_id }),
        content: result.content,
      }),
    );
  }

  private async pendingOperation(request: RuntimeTurnRequest): Promise<HostOperation | undefined> {
    const operation = await this.operationStore?.pending(
      request.conversation_id,
      request.request_id,
    );
    return operation?.[0];
  }
}

function resumeInput(operation: HostOperation): string {
  return [
    'Retome a operação persistida após a troca do Runtime.',
    `Objetivo: ${operation.objective}`,
    `Próximo passo: ${operation.next_step}`,
    operation.resume_context === undefined ? '' : `Contexto: ${operation.resume_context}`,
    'Verifique o estado atual do repositório antes de concluir.',
  ]
    .filter(Boolean)
    .join('\n');
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
    case 'reasoning-start':
      return runtimeEvent(request, 'reasoning-start', {
        reasoning_id: event.reasoningId,
      });
    case 'reasoning-delta':
      return runtimeEvent(request, 'reasoning-delta', {
        reasoning_id: event.reasoningId,
        delta: event.delta,
      });
    case 'reasoning-end':
      return runtimeEvent(request, 'reasoning-end', {
        reasoning_id: event.reasoningId,
      });
    case 'tool.started':
      return runtimeEvent(request, 'tool.started', {
        tool_id: event.toolId,
        name: event.toolName,
        ...(event.target === undefined ? {} : { target: event.target }),
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
    case 'approval.requested':
      return runtimeEvent(request, 'approval.requested', {
        approval_id: event.approvalId,
        tool_name: event.toolName,
        reason: event.reason,
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
    operationStorePath: process.env.ATLAS_HOST_OPERATIONS_FILE,
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
