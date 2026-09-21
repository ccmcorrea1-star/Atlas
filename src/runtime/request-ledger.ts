import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import { randomUUID } from 'node:crypto';

import type { RuntimeEvent, RuntimeInputRequestedData, RuntimeTurnRequest } from './protocol.js';

export type RuntimeRequestState = 'accepted' | 'executing' | 'suspended' | 'ambiguous' | 'terminal';

export type RuntimeRequestSuspension = {
  kind: 'approval' | 'input';
  ids: string[];
  checkpoint: string;
  input?: RuntimeInputRequestedData;
};

export type RuntimeRequestRecord = {
  request: RuntimeTurnRequest;
  fingerprint: string;
  state: RuntimeRequestState;
  events: RuntimeEvent[];
  suspension?: RuntimeRequestSuspension;
};

type RequestLedgerFile = {
  version: 2;
  requests: RuntimeRequestRecord[];
};

export class RuntimeRequestLedger {
  private readonly records = new Map<string, RuntimeRequestRecord>();
  private writeChain: Promise<void> = Promise.resolve();

  public constructor(public readonly filePath: string) {}

  public async load(): Promise<void> {
    try {
      const parsed = JSON.parse(
        await readFile(this.filePath, 'utf8'),
      ) as Partial<RequestLedgerFile>;
      const legacy = (parsed as { version?: unknown }).version === 1;
      if ((!legacy && parsed.version !== 2) || !Array.isArray(parsed.requests)) {
        throw new Error(`Invalid Runtime request ledger at ${this.filePath}.`);
      }
      for (const record of parsed.requests) {
        if (
          record !== null &&
          typeof record === 'object' &&
          typeof record.request?.request_id === 'string' &&
          (record.state === 'accepted' ||
            record.state === 'executing' ||
            record.state === 'suspended' ||
            record.state === 'ambiguous' ||
            record.state === 'terminal' ||
            (legacy &&
              ((record.state as unknown) === 'running' ||
                (record.state as unknown) === 'completed'))) &&
          Array.isArray(record.events)
        ) {
          this.records.set(record.request.request_id, {
            ...record,
            state: legacy
              ? (record.state as unknown) === 'completed'
                ? 'terminal'
                : 'ambiguous'
              : record.state === 'executing'
                ? 'ambiguous'
                : record.state,
          });
        }
      }
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') {
        throw error;
      }
    }
  }

  public get(requestId: string): RuntimeRequestRecord | undefined {
    return this.records.get(requestId);
  }

  public running(): RuntimeRequestRecord[] {
    return [...this.records.values()].filter((record) => record.state === 'accepted');
  }

  public recoverable(): RuntimeRequestRecord[] {
    return [...this.records.values()].filter(
      (record) => record.state === 'accepted' || record.state === 'suspended',
    );
  }

  public accept(request: RuntimeTurnRequest): RuntimeRequestRecord {
    const record: RuntimeRequestRecord = {
      request,
      fingerprint: requestFingerprint(request),
      state: 'accepted',
      events: [],
    };
    this.records.set(request.request_id, record);
    return record;
  }

  public markExecuting(record: RuntimeRequestRecord): void {
    record.state = 'executing';
  }

  public suspend(record: RuntimeRequestRecord, suspension: RuntimeRequestSuspension): void {
    record.state = 'suspended';
    record.suspension = suspension;
  }

  public complete(record: RuntimeRequestRecord): void {
    record.state = 'terminal';
    delete record.suspension;
  }

  public markAmbiguous(record: RuntimeRequestRecord): void {
    record.state = 'ambiguous';
  }

  public persist(): Promise<void> {
    const write = this.writeChain
      .catch(() => undefined)
      .then(async () => {
        await mkdir(dirname(this.filePath), { recursive: true, mode: 0o700 });
        const temporaryPath = `${this.filePath}.${randomUUID()}.tmp`;
        await writeFile(
          temporaryPath,
          `${JSON.stringify({ version: 2, requests: [...this.records.values()] } satisfies RequestLedgerFile)}\n`,
          { encoding: 'utf8', mode: 0o600 },
        );
        await rename(temporaryPath, this.filePath);
      });
    this.writeChain = write.catch(() => undefined);
    return write;
  }
}

export function requestFingerprint(request: RuntimeTurnRequest): string {
  return JSON.stringify({
    conversation_id: request.conversation_id,
    input: request.input,
    attachments: request.attachments ?? [],
    context: request.context ?? {},
  });
}
