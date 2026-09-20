import { randomUUID } from 'node:crypto';
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';

export type HostOperationState =
  | 'modified'
  | 'verified'
  | 'checkpointed'
  | 'handoff'
  | 'healthchecking'
  | 'resuming'
  | 'resumed'
  | 'failed'
  | 'rolled_back';

export type HostOperationResult = {
  content: string;
  message_id?: string;
};

export type HostOperation = {
  operation_id: string;
  request_id: string;
  conversation_id: string;
  objective: string;
  state: HostOperationState;
  base_commit: string;
  candidate_build: string | null;
  next_step: string;
  resume_context?: string;
  result?: HostOperationResult;
  error?: string;
  updated_at: string;
};

type OperationFile = {
  version: 1;
  operations: HostOperation[];
};

const PENDING_STATES: readonly HostOperationState[] = [
  'checkpointed',
  'handoff',
  'healthchecking',
  'resuming',
];

export class OperationStore {
  public constructor(public readonly filePath: string) {}

  public async create(
    input: Omit<HostOperation, 'operation_id' | 'updated_at'> &
      Partial<Pick<HostOperation, 'operation_id'>>,
  ): Promise<HostOperation> {
    const operation: HostOperation = {
      ...input,
      operation_id: input.operation_id ?? randomUUID(),
      updated_at: new Date().toISOString(),
    };
    const file = await this.read();
    file.operations.push(operation);
    await this.write(file);
    return operation;
  }

  public async get(operationId: string): Promise<HostOperation | undefined> {
    const file = await this.read();
    return file.operations.find((operation) => operation.operation_id === operationId);
  }

  public async find(conversationId: string, requestId: string): Promise<HostOperation | undefined> {
    const file = await this.read();
    return file.operations.find(
      (operation) =>
        operation.conversation_id === conversationId && operation.request_id === requestId,
    );
  }

  public async pending(conversationId?: string, requestId?: string): Promise<HostOperation[]> {
    const file = await this.read();
    return file.operations.filter(
      (operation) =>
        PENDING_STATES.includes(operation.state) &&
        (conversationId === undefined || operation.conversation_id === conversationId) &&
        (requestId === undefined || operation.request_id === requestId),
    );
  }

  public async update(
    operationId: string,
    patch: Partial<Omit<HostOperation, 'operation_id'>>,
  ): Promise<HostOperation> {
    const file = await this.read();
    const index = file.operations.findIndex((operation) => operation.operation_id === operationId);
    if (index === -1) {
      throw new Error(`Operation "${operationId}" was not found.`);
    }
    const operation = {
      ...file.operations[index],
      ...patch,
      updated_at: new Date().toISOString(),
    };
    file.operations[index] = operation;
    await this.write(file);
    return operation;
  }

  private async read(): Promise<OperationFile> {
    try {
      const payload = JSON.parse(await readFile(this.filePath, 'utf8')) as Partial<OperationFile>;
      if (payload.version !== 1 || !Array.isArray(payload.operations)) {
        throw new Error(`Invalid operation store format at ${this.filePath}.`);
      }
      return { version: 1, operations: payload.operations };
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
        return { version: 1, operations: [] };
      }
      throw error;
    }
  }

  private async write(file: OperationFile): Promise<void> {
    await mkdir(dirname(this.filePath), { recursive: true, mode: 0o700 });
    const temporaryPath = `${this.filePath}.${randomUUID()}.tmp`;
    await writeFile(temporaryPath, `${JSON.stringify(file, null, 2)}\n`, {
      encoding: 'utf8',
      mode: 0o600,
    });
    await rename(temporaryPath, this.filePath);
  }
}
