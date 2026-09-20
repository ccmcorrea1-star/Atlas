import { spawn, type ChildProcess } from 'node:child_process';
import { createConnection } from 'node:net';
import { cp, mkdir, rm, rename } from 'node:fs/promises';
import { join, resolve } from 'node:path';

import { runtimeLifecycleEvent, type RuntimeEvent } from '../runtime/protocol.js';
import { OperationStore, type HostOperation } from './operations.js';

export type HostCommand = {
  program: string;
  args: string[];
};

export type HostCommandResult = {
  exit_code: number;
  stdout: string;
  stderr: string;
};

export type HostSupervisorOptions = {
  repositoryRoot: string;
  stateDirectory: string;
  socketPath: string;
  validationCommands?: readonly HostCommand[];
  buildCommand?: HostCommand;
  operationStore?: OperationStore;
  runCommand?: (command: HostCommand, cwd: string) => Promise<HostCommandResult>;
  spawnRuntime?: (entrypoint: string, cwd: string, env: NodeJS.ProcessEnv) => ChildProcess;
  healthcheck?: (socketPath: string, timeoutMs: number) => Promise<boolean>;
  healthcheckTimeoutMs?: number;
};

export type HostSupervisorEvent = RuntimeEvent;
export type HostEventListener = (event: HostSupervisorEvent) => void;

const DEFAULT_VALIDATION_COMMANDS: readonly HostCommand[] = [
  { program: 'npm', args: ['run', 'check:full'] },
];

const DEFAULT_BUILD_COMMAND: HostCommand = { program: 'npm', args: ['run', 'build'] };
const DEFAULT_HEALTHCHECK_TIMEOUT_MS = 10_000;

export type HandoffRequest = {
  request_id: string;
  conversation_id: string;
  objective: string;
  next_step: string;
  resume_context?: string;
};

export class HostSupervisor {
  public readonly operationStore: OperationStore;
  public readonly socketPath: string;

  private readonly repositoryRoot: string;
  private readonly stateDirectory: string;
  private readonly validationCommands: readonly HostCommand[];
  private readonly buildCommand: HostCommand;
  private readonly runCommand: (command: HostCommand, cwd: string) => Promise<HostCommandResult>;
  private readonly spawnRuntime: (
    entrypoint: string,
    cwd: string,
    env: NodeJS.ProcessEnv,
  ) => ChildProcess;
  private readonly healthcheck: (socketPath: string, timeoutMs: number) => Promise<boolean>;
  private readonly healthcheckTimeoutMs: number;
  private readonly listeners = new Set<HostEventListener>();
  private runtimeProcess: ChildProcess | undefined;

  public constructor(options: HostSupervisorOptions) {
    this.repositoryRoot = resolve(options.repositoryRoot);
    this.stateDirectory = resolve(options.stateDirectory);
    this.socketPath = options.socketPath;
    this.operationStore =
      options.operationStore ?? new OperationStore(join(this.stateDirectory, 'operations.json'));
    this.validationCommands = options.validationCommands ?? DEFAULT_VALIDATION_COMMANDS;
    this.buildCommand = options.buildCommand ?? DEFAULT_BUILD_COMMAND;
    this.runCommand = options.runCommand ?? runHostCommand;
    this.spawnRuntime = options.spawnRuntime ?? spawnRuntimeProcess;
    this.healthcheck = options.healthcheck ?? waitForUnixSocket;
    this.healthcheckTimeoutMs = options.healthcheckTimeoutMs ?? DEFAULT_HEALTHCHECK_TIMEOUT_MS;
  }

  public subscribe(listener: HostEventListener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  public async handoff(request: HandoffRequest): Promise<HostOperation> {
    const baseCommit = await this.readBaseCommit();
    const operation = await this.operationStore.create({
      request_id: request.request_id,
      conversation_id: request.conversation_id,
      objective: request.objective,
      state: 'modified',
      base_commit: baseCommit,
      candidate_build: null,
      next_step: request.next_step,
      ...(request.resume_context === undefined ? {} : { resume_context: request.resume_context }),
    });

    try {
      await this.runValidations();
      await this.operationStore.update(operation.operation_id, { state: 'verified' });
      const candidateBuild = await this.buildCandidate(operation.operation_id);
      await this.operationStore.update(operation.operation_id, {
        state: 'checkpointed',
        candidate_build: candidateBuild,
      });

      await this.operationStore.update(operation.operation_id, { state: 'handoff' });
      this.emit(
        runtimeLifecycleEvent(
          'runtime.restarting',
          request.conversation_id,
          {
            reason: 'handoff',
          },
          request.request_id,
        ),
      );
      await this.stopRuntime();
      await this.promoteCandidate();
      await this.operationStore.update(operation.operation_id, { state: 'healthchecking' });
      try {
        await this.startRuntime();
      } catch (error) {
        await this.rollback(operation.operation_id, request);
        throw error;
      }

      await this.operationStore.update(operation.operation_id, { state: 'resuming' });
      this.emit(
        runtimeLifecycleEvent('runtime.ready', request.conversation_id, {}, request.request_id),
      );
      this.emit(
        runtimeLifecycleEvent(
          'operation.resuming',
          request.conversation_id,
          {
            operation_id: operation.operation_id,
          },
          request.request_id,
        ),
      );
      return (await this.operationStore.get(operation.operation_id)) as HostOperation;
    } catch (error) {
      const current = await this.operationStore.get(operation.operation_id);
      if (current?.state !== 'rolled_back' && current?.state !== 'failed') {
        await this.operationStore.update(operation.operation_id, {
          state: 'failed',
          error: error instanceof Error ? error.message : String(error),
        });
      }
      throw error;
    }
  }

  public async start(): Promise<void> {
    await this.startRuntime();
  }

  public async stop(): Promise<void> {
    await this.stopRuntime();
  }

  private async runValidations(): Promise<void> {
    for (const command of this.validationCommands) {
      const result = await this.runCommand(command, this.repositoryRoot);
      if (result.exit_code !== 0) {
        throw new Error(
          `Validation failed for ${command.program} ${command.args.join(' ')}: ${result.stderr || result.stdout}`,
        );
      }
    }
  }

  private async buildCandidate(operationId: string): Promise<string> {
    const result = await this.runCommand(this.buildCommand, this.repositoryRoot);
    if (result.exit_code !== 0) {
      throw new Error(
        `Candidate build failed for ${this.buildCommand.program} ${this.buildCommand.args.join(' ')}: ${result.stderr || result.stdout}`,
      );
    }

    const sourceDirectory = join(this.repositoryRoot, 'dist');
    const candidateDirectory = join(this.stateDirectory, 'candidate');
    await mkdir(this.stateDirectory, { recursive: true, mode: 0o700 });
    await rm(candidateDirectory, { recursive: true, force: true });
    await cp(sourceDirectory, candidateDirectory, { recursive: true });
    const promptsDirectory = join(this.repositoryRoot, 'src', 'prompts');
    await cp(promptsDirectory, join(candidateDirectory, 'prompts'), { recursive: true });
    return `candidate:${operationId}`;
  }

  private async promoteCandidate(): Promise<void> {
    const candidateDirectory = join(this.stateDirectory, 'candidate');
    const currentDirectory = join(this.stateDirectory, 'current');
    const previousDirectory = join(this.stateDirectory, 'previous');
    await mkdir(this.stateDirectory, { recursive: true, mode: 0o700 });
    await rm(previousDirectory, { recursive: true, force: true });
    await moveIfPresent(currentDirectory, previousDirectory);
    await rename(candidateDirectory, currentDirectory);
  }

  private async rollback(operationId: string, request: HandoffRequest): Promise<void> {
    await this.stopRuntime();
    const currentDirectory = join(this.stateDirectory, 'current');
    const previousDirectory = join(this.stateDirectory, 'previous');
    const failedDirectory = join(this.stateDirectory, 'failed', operationId);
    await mkdir(join(this.stateDirectory, 'failed'), { recursive: true, mode: 0o700 });
    await rm(failedDirectory, { recursive: true, force: true });
    await moveIfPresent(currentDirectory, failedDirectory);
    await rename(previousDirectory, currentDirectory);
    await this.operationStore.update(operationId, {
      state: 'rolled_back',
      error: 'Candidate Runtime failed its healthcheck.',
    });
    this.emit(
      runtimeLifecycleEvent(
        'runtime.restarting',
        request.conversation_id,
        {
          reason: 'rollback',
        },
        request.request_id,
      ),
    );
    await this.startRuntime();
    this.emit(
      runtimeLifecycleEvent('runtime.ready', request.conversation_id, {}, request.request_id),
    );
  }

  private async startRuntime(): Promise<void> {
    if (this.runtimeProcess !== undefined) {
      return;
    }
    const entrypoint = join(this.stateDirectory, 'current', 'runtime', 'server.js');
    const childProcess = this.spawnRuntime(entrypoint, this.repositoryRoot, {
      ...process.env,
      ATLAS_RUNTIME_SOCKET: this.socketPath,
      ATLAS_HOST_OPERATIONS_FILE: this.operationStore.filePath,
      ATLAS_RUNTIME_PROCESS: '1',
    });
    this.runtimeProcess = childProcess;
    let processWasHealthy = false;
    childProcess.once('exit', () => {
      if (this.runtimeProcess === childProcess) {
        this.runtimeProcess = undefined;
        if (processWasHealthy) {
          void this.restartAfterCrash();
        }
      }
    });
    const healthy = await this.healthcheck(this.socketPath, this.healthcheckTimeoutMs);
    if (!healthy) {
      await this.stopRuntime();
      throw new Error('Candidate Runtime failed its healthcheck.');
    }
    processWasHealthy = true;
  }

  private async restartAfterCrash(): Promise<void> {
    this.emit(runtimeLifecycleEvent('runtime.restarting', undefined, { reason: 'crash' }));
    try {
      await this.startRuntime();
      this.emit(runtimeLifecycleEvent('runtime.ready'));
    } catch (error) {
      // O próximo supervisor/start poderá tentar novamente; não há operação de build para marcar.
      console.error(
        `Runtime crash recovery failed: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }

  private async stopRuntime(): Promise<void> {
    const process = this.runtimeProcess;
    if (process === undefined) {
      return;
    }
    this.runtimeProcess = undefined;
    if (process.exitCode === null && process.signalCode === null) {
      process.kill('SIGTERM');
      await waitForProcessExit(process, this.healthcheckTimeoutMs);
    }
  }

  private async readBaseCommit(): Promise<string> {
    const result = await this.runCommand(
      { program: 'git', args: ['rev-parse', 'HEAD'] },
      this.repositoryRoot,
    );
    if (result.exit_code !== 0 || !result.stdout.trim()) {
      throw new Error(
        `Unable to read the repository base commit: ${result.stderr || result.stdout}`,
      );
    }
    return result.stdout.trim();
  }

  private emit(event: HostSupervisorEvent): void {
    for (const listener of this.listeners) {
      listener(event);
    }
  }
}

async function runHostCommand(command: HostCommand, cwd: string): Promise<HostCommandResult> {
  return new Promise((resolveResult, rejectResult) => {
    const child = spawn(command.program, command.args, { cwd, stdio: ['ignore', 'pipe', 'pipe'] });
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    child.stdout?.on('data', (chunk: Buffer) => stdout.push(Buffer.from(chunk)));
    child.stderr?.on('data', (chunk: Buffer) => stderr.push(Buffer.from(chunk)));
    child.once('error', rejectResult);
    child.once('close', (code) => {
      resolveResult({
        exit_code: code ?? 1,
        stdout: Buffer.concat(stdout).toString('utf8'),
        stderr: Buffer.concat(stderr).toString('utf8'),
      });
    });
  });
}

function spawnRuntimeProcess(
  entrypoint: string,
  cwd: string,
  env: NodeJS.ProcessEnv,
): ChildProcess {
  return spawn(process.execPath, [entrypoint], {
    cwd,
    env,
    stdio: 'ignore',
  });
}

async function waitForUnixSocket(socketPath: string, timeoutMs: number): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const connected = await new Promise<boolean>((resolveConnection) => {
      const socket = createConnection(socketPath);
      socket.once('connect', () => {
        socket.destroy();
        resolveConnection(true);
      });
      socket.once('error', () => {
        socket.destroy();
        resolveConnection(false);
      });
    });
    if (connected) {
      return true;
    }
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 25));
  }
  return false;
}

async function waitForProcessExit(process: ChildProcess, timeoutMs: number): Promise<void> {
  if (process.exitCode !== null || process.signalCode !== null) {
    return;
  }
  await new Promise<void>((resolveExit) => {
    const timer = setTimeout(() => {
      process.kill('SIGKILL');
      resolveExit();
    }, timeoutMs);
    process.once('exit', () => {
      clearTimeout(timer);
      resolveExit();
    });
  });
}

async function moveIfPresent(source: string, destination: string): Promise<void> {
  try {
    await rename(source, destination);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw error;
    }
  }
}

export { DEFAULT_BUILD_COMMAND, DEFAULT_VALIDATION_COMMANDS, waitForUnixSocket };
