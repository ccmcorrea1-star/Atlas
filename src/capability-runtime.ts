import { spawn } from 'node:child_process';
import { resolve } from 'node:path';

export type CapabilityDiscoveryRequest = {
  path?: string;
  query?: string;
};

export type CapabilityDiscoveryResult = {
  id: string;
  type: string;
  summary: string;
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
  getDefinition(id: string): Promise<CapabilityDefinition | undefined>;
  execute(
    id: string,
    target: string,
    arguments_: Record<string, unknown>,
    options?: CapabilityExecutionOptions,
  ): Promise<CapabilityExecutionResult>;
}

export type NativeCapabilityRuntimeOptions = {
  executablePath?: string;
};

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

function discoveryResult(value: unknown): CapabilityDiscoveryResult {
  const result = asObject(value, 'Discovery result');
  return {
    id: requiredString(result.id, 'id'),
    type: requiredString(result.type, 'type'),
    summary: requiredString(result.summary, 'summary'),
  };
}

function jsonSchema(value: unknown): Record<string, unknown> {
  return asObject(value, 'Capability schema');
}

export class NativeCapabilityRuntime implements CapabilityRuntime {
  private readonly executablePath: string;

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
      ...(request.path === undefined ? {} : { path: request.path }),
      ...(request.query === undefined ? {} : { query: request.query }),
    });
    const results = response.results;
    if (!Array.isArray(results)) {
      throw new Error('Capability runtime response field "results" must be an array.');
    }

    return results.map(discoveryResult);
  }

  public async getDefinition(id: string): Promise<CapabilityDefinition | undefined> {
    const response = await this.request({ operation: 'get_definition', id });
    if (response.definition === null || response.definition === undefined) {
      return undefined;
    }

    const definition = asObject(response.definition, 'Capability definition');
    return {
      id: requiredString(definition.id, 'id'),
      type: requiredString(definition.type, 'type'),
      summary: requiredString(definition.summary, 'summary'),
      description: requiredString(definition.description, 'description'),
      schema: jsonSchema(definition.schema),
    };
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
    const result = asObject(response, 'Capability execution result');
    return {
      ...result,
      target: requiredString(result.target, 'target'),
      status: requiredString(result.status, 'status'),
      error: typeof result.error === 'string' ? result.error : '',
    };
  }

  private request(
    request: Record<string, unknown>,
    options: Pick<CapabilityExecutionOptions, 'signal' | 'onOutput'> = {},
  ): Promise<Record<string, unknown>> {
    return new Promise((resolveRequest, rejectRequest) => {
      const child = spawn(this.executablePath, [], {
        stdio: ['pipe', 'pipe', 'pipe'],
      });
      let output = '';
      let errorOutput = '';
      let settled = false;
      let streamBuffer = '';
      let streamQueue = Promise.resolve();

      const rejectOnce = (error: Error) => {
        if (!settled) {
          settled = true;
          rejectRequest(error);
        }
      };

      child.stdout.setEncoding('utf8');
      child.stderr.setEncoding('utf8');
      child.stdout.on('data', (chunk: string) => {
        output += chunk;
        if (options.onOutput === undefined) {
          return;
        }
        streamBuffer += chunk;
        let newline = streamBuffer.indexOf('\n');
        while (newline !== -1) {
          const line = streamBuffer.slice(0, newline).trim();
          streamBuffer = streamBuffer.slice(newline + 1);
          newline = streamBuffer.indexOf('\n');
          if (!line) {
            continue;
          }
          let event: unknown;
          try {
            event = JSON.parse(line) as unknown;
          } catch {
            continue;
          }
          const record =
            event !== null && typeof event === 'object' && !Array.isArray(event)
              ? (event as Record<string, unknown>)
              : undefined;
          if (record?.event !== 'execution.output.delta') {
            continue;
          }
          const channel = record.channel;
          const delta = record.delta;
          if ((channel !== 'stdout' && channel !== 'stderr') || typeof delta !== 'string') {
            continue;
          }
          streamQueue = streamQueue.then(() => options.onOutput?.(channel, delta));
          streamQueue.catch(rejectOnce);
        }
      });
      child.stderr.on('data', (chunk: string) => {
        errorOutput += chunk;
      });
      child.once('error', (error) => {
        rejectOnce(error);
      });
      child.once('close', (code, signal) => {
        void (async () => {
          await streamQueue;
          if (settled) {
            return;
          }
          if (code !== 0) {
            const detail = errorOutput.trim() || output.trim();
            rejectOnce(
              new Error(
                `Capability runtime exited with ${signal ? `signal ${signal}` : `code ${code}`}${
                  detail ? `: ${detail}` : '.'
                }`,
              ),
            );
            return;
          }

          try {
            const lines = output
              .split(/\r?\n/)
              .map((line) => line.trim())
              .filter(Boolean);
            const parsed = JSON.parse(lines.at(-1) ?? '') as unknown;
            settled = true;
            resolveRequest(asObject(parsed, 'Capability runtime response'));
          } catch (error) {
            rejectOnce(
              new Error(
                `Capability runtime returned invalid JSON: ${error instanceof Error ? error.message : String(error)}`,
              ),
            );
          }
        })().catch(rejectOnce);
      });

      const abort = () => {
        child.kill('SIGTERM');
        rejectOnce(new Error('Capability execution aborted.'));
      };
      if (options.signal !== undefined) {
        if (options.signal.aborted) {
          abort();
        } else {
          options.signal.addEventListener('abort', abort, { once: true });
        }
      }

      child.stdin.end(JSON.stringify(request));
    });
  }
}

export function createCapabilityRuntime(
  options: NativeCapabilityRuntimeOptions = {},
): CapabilityRuntime {
  return new NativeCapabilityRuntime(options);
}
