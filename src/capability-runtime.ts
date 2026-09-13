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

export interface CapabilityRuntime {
  discover(request?: CapabilityDiscoveryRequest): Promise<CapabilityDiscoveryResult[]>;
  getDefinition(id: string): Promise<CapabilityDefinition | undefined>;
  execute(
    id: string,
    target: string,
    arguments_: Record<string, unknown>,
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
  ): Promise<CapabilityExecutionResult> {
    const response = await this.request({
      operation: 'execute',
      id,
      target,
      arguments: arguments_,
    });
    const result = asObject(response, 'Capability execution result');
    return {
      ...result,
      target: requiredString(result.target, 'target'),
      status: requiredString(result.status, 'status'),
      error: typeof result.error === 'string' ? result.error : '',
    };
  }

  private request(request: Record<string, unknown>): Promise<Record<string, unknown>> {
    return new Promise((resolveRequest, rejectRequest) => {
      const child = spawn(this.executablePath, [], {
        stdio: ['pipe', 'pipe', 'pipe'],
      });
      let output = '';
      let errorOutput = '';
      let settled = false;

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
      });
      child.stderr.on('data', (chunk: string) => {
        errorOutput += chunk;
      });
      child.once('error', (error) => {
        rejectOnce(error);
      });
      child.once('close', (code, signal) => {
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
          const parsed = JSON.parse(output) as unknown;
          settled = true;
          resolveRequest(asObject(parsed, 'Capability runtime response'));
        } catch (error) {
          rejectOnce(
            new Error(
              `Capability runtime returned invalid JSON: ${error instanceof Error ? error.message : String(error)}`,
            ),
          );
        }
      });

      child.stdin.end(JSON.stringify(request));
    });
  }
}

export function createCapabilityRuntime(
  options: NativeCapabilityRuntimeOptions = {},
): CapabilityRuntime {
  return new NativeCapabilityRuntime(options);
}
