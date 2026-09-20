import type {
  CapabilityDiscoveryRequest,
  CapabilityDiscoveryResult,
  CapabilityExecutionOptions,
  CapabilityExecutionResult,
  CapabilityRuntime,
  CapabilityToolListRequest,
  CapabilityToolListResult,
  SkillDefinition,
  ToolDefinition,
} from './capability-runtime.js';

export type CapabilityExecutionHookContext = {
  id: string;
  target: string;
  arguments: Record<string, unknown>;
};

// before_execute pode curto-circuitar a chamada devolvendo o resultado anterior.
export type BeforeExecuteHook = (
  context: CapabilityExecutionHookContext,
) => CapabilityExecutionResult | undefined | Promise<CapabilityExecutionResult | undefined>;

// after_execute observa toda execucao real, inclusive as que lancam excecao.
export type AfterExecuteHook = (
  context: CapabilityExecutionHookContext,
  result: CapabilityExecutionResult,
) => void | Promise<void>;

// Pontos de extensao genericos aplicados a qualquer capability.
export type CapabilityExecutionHooks = {
  before_execute?: BeforeExecuteHook;
  after_execute?: AfterExecuteHook;
};

// Serializacao ordenada mantem fingerprints estaveis mesmo com a ordem das chaves JSON.
export function stableSerialize(value: unknown): string {
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

function failedResult(target: string, error: unknown): CapabilityExecutionResult {
  return {
    target,
    status: 'failed',
    error: error instanceof Error ? error.message : String(error),
  };
}

// Aplica hooks antes e depois de cada execucao, sem alterar o contrato do runtime.
export class HookableCapabilityRuntime implements CapabilityRuntime {
  private readonly runtime: CapabilityRuntime;
  private readonly hooks: readonly CapabilityExecutionHooks[];

  public constructor(
    runtime: CapabilityRuntime,
    hooks: CapabilityExecutionHooks | readonly CapabilityExecutionHooks[] = [],
  ) {
    this.runtime = runtime;
    this.hooks = Array.isArray(hooks) ? hooks : [hooks as CapabilityExecutionHooks];
  }

  public discover(request: CapabilityDiscoveryRequest = {}): Promise<CapabilityDiscoveryResult[]> {
    return this.runtime.discover(request);
  }

  public listTools(request: CapabilityToolListRequest = {}): Promise<CapabilityToolListResult[]> {
    return this.runtime.listTools(request);
  }

  public getDefinition(id: string): Promise<ToolDefinition | undefined> {
    return this.runtime.getDefinition(id);
  }

  public getSkill(id: string): Promise<SkillDefinition | undefined> {
    return this.runtime.getSkill(id);
  }

  // Repassa o encerramento para o runtime decorado, quando ele for persistente.
  public close(): Promise<void> {
    return this.runtime.close?.() ?? Promise.resolve();
  }

  public async execute(
    id: string,
    target: string,
    arguments_: Record<string, unknown>,
    options: CapabilityExecutionOptions = {},
  ): Promise<CapabilityExecutionResult> {
    const context: CapabilityExecutionHookContext = { id, target, arguments: arguments_ };

    for (const hooks of this.hooks) {
      const blocked = await hooks.before_execute?.(context);
      if (blocked !== undefined) {
        return blocked;
      }
    }

    let result: CapabilityExecutionResult;
    try {
      result = await this.runtime.execute(id, target, arguments_, options);
    } catch (error) {
      for (const hooks of this.hooks) {
        await hooks.after_execute?.(context, failedResult(target, error));
      }
      throw error;
    }

    for (const hooks of this.hooks) {
      await hooks.after_execute?.(context, result);
    }
    return result;
  }
}

const RETRY_BLOCKING_STATUSES: ReadonlySet<string> = new Set([
  'failed',
  'unavailable',
  'invalid_arguments',
  'permission_denied',
]);

export type RetryGuardStatus = 'failed' | 'unavailable' | 'invalid_arguments' | 'permission_denied';

// Um guard por turno impede repeticao identica de chamada que ja falhou.
export class RetryGuard implements CapabilityExecutionHooks {
  private readonly attempts = new Map<string, CapabilityExecutionResult>();

  // O fingerprint inclui id, target e argumentos normalizados.
  public fingerprint(context: CapabilityExecutionHookContext): string {
    return stableSerialize({
      id: context.id,
      target: context.target,
      arguments: context.arguments,
    });
  }

  public before_execute(
    context: CapabilityExecutionHookContext,
  ): CapabilityExecutionResult | undefined {
    const previous = this.attempts.get(this.fingerprint(context));
    if (previous === undefined || !RETRY_BLOCKING_STATUSES.has(previous.status)) {
      return undefined;
    }

    return {
      target: context.target,
      status: previous.status,
      error:
        `Identical capability call "${context.id}" with target "${context.target}" ` +
        `already failed in this turn with status "${previous.status}": ${previous.error}. ` +
        'Change the arguments, target, or underlying state before retrying.',
      blocked: true,
    };
  }

  public after_execute(
    context: CapabilityExecutionHookContext,
    result: CapabilityExecutionResult,
  ): void {
    this.attempts.set(this.fingerprint(context), result);
  }

  // Invalida o estado apos uma acao explicita que possa corrigir a causa.
  public invalidate(): void {
    this.attempts.clear();
  }

  public reset(): void {
    this.invalidate();
  }
}
