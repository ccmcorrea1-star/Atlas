import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { resolve } from 'node:path';

export type CapabilityDiscoveryRequest = {
  query?: string;
  limit?: number;
};

export type CapabilityDiscoveryResult = {
  id: string;
  type: string;
  summary: string;
};

export type CapabilityToolListRequest = {
  group?: string;
};

export type CapabilityToolListResult = CapabilityDiscoveryResult & {
  group?: string;
};

export type CapabilityDefinition = CapabilityDiscoveryResult & {
  description: string;
  schema: Record<string, unknown>;
};

export type CapabilityExecutionResult = {
  target: string;
  status: string;
  error: string;
  [key: string]: unknown;
};

export type CapabilityExecutionOptions = {
  signal?: AbortSignal;
  onOutput?: (channel: 'stdout' | 'stderr', delta: string) => void | Promise<void>;
};

export interface CapabilityRuntime {
  discover(request?: CapabilityDiscoveryRequest): Promise<CapabilityDiscoveryResult[]>;
  listTools(request?: CapabilityToolListRequest): Promise<CapabilityToolListResult[]>;
  getDefinition(id: string): Promise<CapabilityDefinition | undefined>;
  execute(
    id: string,
    target: string,
    arguments_: Record<string, unknown>,
    options?: CapabilityExecutionOptions,
  ): Promise<CapabilityExecutionResult>;
  // Encerra recursos persistentes quando o runtime é descartado.
  close?(): Promise<void>;
}

export type NativeCapabilityRuntimeOptions = {
  executablePath?: string;
};

type PendingRequest = {
  resolve: (value: Record<string, unknown>) => void;
  reject: (error: Error) => void;
  onOutput?: (channel: 'stdout' | 'stderr', delta: string) => void | Promise<void>;
  streamQueue: Promise<void>;
  signal?: AbortSignal;
  abortListener?: () => void;
  settled: boolean;
};

type BridgeTerminator = () => void;

const activeBridges = new Set<BridgeTerminator>();
let exitHookInstalled = false;

function registerBridge(terminate: BridgeTerminator): void {
  activeBridges.add(terminate);
  if (exitHookInstalled) {
    return;
  }
  exitHookInstalled = true;
  process.once('exit', () => {
    for (const active of activeBridges) {
      active();
    }
  });
}

function unregisterBridge(terminate: BridgeTerminator): void {
  activeBridges.delete(terminate);
}

function toError(value: unknown): Error {
  return value instanceof Error ? value : new Error(String(value));
}

function asObject(value: unknown, context: string): Record<string, unknown> {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`${context} must be an object.`);
  }

  return value as Record<string, unknown>;
}

function requiredString(value: unknown, field: string): string {
  if (typeof value !== 'string' || !value) {
    throw new Error(`Capability runtime response field "${field}" must be a non-empty string.`);
  }

  return value;
}

function isUsableType(type: string): boolean {
  return type === 'tool' || type === 'skill';
}

function discoveryResult(value: unknown): CapabilityDiscoveryResult {
  const result = asObject(value, 'Discovery result');
  return {
    id: requiredString(result.id, 'id'),
    type: requiredString(result.type, 'type'),
    summary: requiredString(result.summary, 'summary'),
  };
}

function toolListResult(value: unknown): CapabilityToolListResult {
  const result = discoveryResult(value);
  const record = asObject(value, 'Tool list result');
  if (record.group !== undefined && (typeof record.group !== 'string' || !record.group)) {
    throw new Error('Tool list result field "group" must be a non-empty string when present.');
  }
  return {
    ...result,
    ...(record.group === undefined ? {} : { group: record.group }),
  };
}

function jsonSchema(value: unknown): Record<string, unknown> {
  return asObject(value, 'Capability schema');
}

// Cliente do bridge C++ persistente. O processo é iniciado sob demanda, reutilizado
// enquanto este runtime existir e reiniciado de forma limpa depois de crash ou EOF.
export class NativeCapabilityRuntime implements CapabilityRuntime {
  private readonly executablePath: string;
  private child: ChildProcessWithoutNullStreams | undefined;
  private terminate: BridgeTerminator | undefined;
  private stdoutBuffer = '';
  private stderrBuffer = '';
  private requestCounter = 0;
  private readonly pending = new Map<string, PendingRequest>();

  public constructor(options: NativeCapabilityRuntimeOptions = {}) {
    this.executablePath =
      options.executablePath ??
      process.env.ATLAS_CAPABILITY_RUNTIME ??
      resolve(process.cwd(), 'src/capabilities/runtime/bridge/runtime');
  }

  public async discover(
    request: CapabilityDiscoveryRequest = {},
  ): Promise<CapabilityDiscoveryResult[]> {
    const response = await this.request({
      operation: 'discover',
      ...(request.query === undefined ? {} : { query: request.query }),
      ...(request.limit === undefined ? {} : { limit: request.limit }),
    });
    const results = response.results;
    if (!Array.isArray(results)) {
      throw new Error('Capability runtime response field "results" must be an array.');
    }

    return results.map(discoveryResult).filter(({ type }) => isUsableType(type));
  }

  public async listTools(
    request: CapabilityToolListRequest = {},
  ): Promise<CapabilityToolListResult[]> {
    const response = await this.request({
      operation: 'list_tools',
      ...(request.group === undefined ? {} : { group: request.group }),
    });
    const tools = response.tools;
    if (!Array.isArray(tools)) {
      throw new Error('Capability runtime response field "tools" must be an array.');
    }

    return tools.map(toolListResult);
  }

  public async getDefinition(id: string): Promise<CapabilityDefinition | undefined> {
    const response = await this.request({ operation: 'get_definition', id });
    if (response.definition === null || response.definition === undefined) {
      return undefined;
    }

    const definition = asObject(response.definition, 'Capability definition');
    const parsedDefinition = {
      id: requiredString(definition.id, 'id'),
      type: requiredString(definition.type, 'type'),
      summary: requiredString(definition.summary, 'summary'),
      description: requiredString(definition.description, 'description'),
      schema: jsonSchema(definition.schema),
    };
    return isUsableType(parsedDefinition.type) ? parsedDefinition : undefined;
  }

  public async execute(
    id: string,
    target: string,
    arguments_: Record<string, unknown>,
    options: CapabilityExecutionOptions = {},
  ): Promise<CapabilityExecutionResult> {
    const response = await this.request(
      {
        operation: 'execute',
        id,
        target,
        arguments: arguments_,
        ...(options.onOutput === undefined ? {} : { stream: true }),
      },
      options,
    );
    // request_id é identidade do transporte e não faz parte do resultado público.
    const { request_id: _requestId, ...result } = asObject(response, 'Capability execution result');
    return {
      ...result,
      target: requiredString(result.target, 'target'),
      status: requiredString(result.status, 'status'),
      error: typeof result.error === 'string' ? result.error : '',
    };
  }

  // Encerra o bridge persistente e libera o processo.
  public close(): Promise<void> {
    const child = this.child;
    if (child === undefined) {
      return Promise.resolve();
    }

    return new Promise<void>((resolveClose) => {
      child.once('close', () => resolveClose());
      child.stdin.end();
    });
  }

  private request(
    request: Record<string, unknown>,
    options: Pick<CapabilityExecutionOptions, 'signal' | 'onOutput'> = {},
  ): Promise<Record<string, unknown>> {
    return new Promise((resolveRequest, rejectRequest) => {
      let child: ChildProcessWithoutNullStreams;
      try {
        child = this.ensureBridge();
      } catch (error) {
        rejectRequest(toError(error));
        return;
      }

      const requestId = `capability-${++this.requestCounter}`;
      const pending: PendingRequest = {
        resolve: resolveRequest,
        reject: rejectRequest,
        streamQueue: Promise.resolve(),
        settled: false,
        ...(options.onOutput === undefined ? {} : { onOutput: options.onOutput }),
        ...(options.signal === undefined ? {} : { signal: options.signal }),
      };
      this.pending.set(requestId, pending);

      const abort = () => {
        this.rejectRequest(requestId, new Error('Capability execution aborted.'));
        this.terminateBridge(child);
      };
      if (options.signal !== undefined) {
        if (options.signal.aborted) {
          abort();
          return;
        }
        pending.abortListener = abort;
        options.signal.addEventListener('abort', abort, { once: true });
      }

      child.stdin.write(`${JSON.stringify({ ...request, request_id: requestId })}\n`, (error) => {
        if (error) {
          this.rejectRequest(requestId, toError(error));
        }
      });
    });
  }

  private ensureBridge(): ChildProcessWithoutNullStreams {
    if (this.child !== undefined) {
      return this.child;
    }

    const child = spawn(this.executablePath, [], {
      stdio: ['pipe', 'pipe', 'pipe'],
      detached: process.platform !== 'win32',
    });
    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');

    const terminate = () => this.terminateBridge(child);
    registerBridge(terminate);
    this.terminate = terminate;

    child.stdout.on('data', (chunk: string) => this.handleStdout(chunk));
    child.stderr.on('data', (chunk: string) => {
      this.stderrBuffer += chunk;
    });
    child.stdin.on('error', (error: Error) => this.handleBridgeError(child, error));
    child.once('error', (error: Error) => this.handleBridgeError(child, error));
    child.once('close', (code, signal) => this.handleBridgeClose(child, code, signal));

    this.child = child;
    return child;
  }

  private handleStdout(chunk: string): void {
    this.stdoutBuffer += chunk;
    let newline = this.stdoutBuffer.indexOf('\n');
    while (newline !== -1) {
      const line = this.stdoutBuffer.slice(0, newline).replace(/\r$/, '').trim();
      this.stdoutBuffer = this.stdoutBuffer.slice(newline + 1);
      newline = this.stdoutBuffer.indexOf('\n');
      if (line) {
        this.handleMessage(line);
      }
    }
  }

  private handleMessage(line: string): void {
    let parsed: unknown;
    try {
      parsed = JSON.parse(line) as unknown;
    } catch {
      return;
    }
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
      return;
    }

    const record = parsed as Record<string, unknown>;
    const requestId = typeof record.request_id === 'string' ? record.request_id : undefined;

    if (record.event === 'execution.output.delta') {
      if (requestId === undefined) {
        return;
      }
      const pending = this.pending.get(requestId);
      if (pending === undefined || pending.onOutput === undefined) {
        return;
      }
      const channel = record.channel;
      const delta = record.delta;
      if ((channel !== 'stdout' && channel !== 'stderr') || typeof delta !== 'string') {
        return;
      }
      pending.streamQueue = pending.streamQueue.then(async () => {
        await pending.onOutput?.(channel, delta);
      });
      pending.streamQueue.catch((error) => this.rejectRequest(requestId, toError(error)));
      return;
    }

    if (requestId === undefined) {
      const message =
        typeof record.error === 'string' && record.error
          ? record.error
          : 'Capability bridge returned an uncorrelated response.';
      this.failAll(new Error(message));
      return;
    }

    if (this.pending.get(requestId) === undefined) {
      return;
    }

    // Erro de protocolo não possui status; resultados de execute sempre possuem.
    if (typeof record.error === 'string' && record.error && typeof record.status !== 'string') {
      this.rejectRequest(requestId, new Error(record.error));
      return;
    }

    this.resolveRequest(requestId, record);
  }

  private resolveRequest(requestId: string, value: Record<string, unknown>): void {
    const pending = this.pending.get(requestId);
    if (pending === undefined) {
      return;
    }
    pending.streamQueue.then(
      () => this.finishRequest(requestId, (current) => current.resolve(value)),
      (error) => this.finishRequest(requestId, (current) => current.reject(toError(error))),
    );
  }

  private rejectRequest(requestId: string, error: Error): void {
    const pending = this.pending.get(requestId);
    if (pending === undefined) {
      return;
    }
    pending.streamQueue.then(
      () => this.finishRequest(requestId, (current) => current.reject(error)),
      () => this.finishRequest(requestId, (current) => current.reject(error)),
    );
  }

  private finishRequest(requestId: string, settle: (pending: PendingRequest) => void): void {
    const pending = this.pending.get(requestId);
    if (pending === undefined || pending.settled) {
      return;
    }
    pending.settled = true;
    this.pending.delete(requestId);
    if (pending.signal !== undefined && pending.abortListener !== undefined) {
      pending.signal.removeEventListener('abort', pending.abortListener);
    }
    settle(pending);
  }

  private failAll(error: Error): void {
    for (const requestId of [...this.pending.keys()]) {
      this.finishRequest(requestId, (pending) => pending.reject(error));
    }
  }

  private handleBridgeError(child: ChildProcessWithoutNullStreams, error: unknown): void {
    if (this.child !== child) {
      return;
    }
    this.failAll(toError(error));
    this.resetBridge(child);
  }

  private handleBridgeClose(
    child: ChildProcessWithoutNullStreams,
    code: number | null,
    signal: NodeJS.Signals | null,
  ): void {
    if (this.child !== child) {
      return;
    }
    const detail = this.stderrBuffer.trim();
    const reason = signal !== null ? `signal ${signal}` : `code ${code}`;
    this.failAll(
      new Error(`Capability bridge exited with ${reason}${detail ? `: ${detail}` : '.'}`),
    );
    this.resetBridge(child);
  }

  private resetBridge(child: ChildProcessWithoutNullStreams): void {
    if (this.child !== child) {
      return;
    }
    if (this.terminate !== undefined) {
      unregisterBridge(this.terminate);
      this.terminate = undefined;
    }
    this.child = undefined;
    this.stdoutBuffer = '';
    this.stderrBuffer = '';
  }

  private terminateBridge(child: ChildProcessWithoutNullStreams): void {
    if (child.pid === undefined || child.exitCode !== null || child.signalCode !== null) {
      return;
    }
    if (process.platform !== 'win32') {
      try {
        process.kill(-child.pid, 'SIGTERM');
        return;
      } catch {
        // Sem grupo próprio, encerra apenas o processo do bridge.
      }
    }
    child.kill('SIGTERM');
  }
}

export function createCapabilityRuntime(
  options: NativeCapabilityRuntimeOptions = {},
): CapabilityRuntime {
  return new NativeCapabilityRuntime(options);
}
