import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import { randomUUID } from 'node:crypto';

import type { RuntimeEvent, RuntimeTurnRequest } from './protocol.js';

export type RuntimeRequestRecord = {
  request: RuntimeTurnRequest;
  fingerprint: string;
  state: 'running' | 'completed';
  events: RuntimeEvent[];
};

type RequestLedgerFile = {
  version: 1;
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
      if (parsed.version !== 1 || !Array.isArray(parsed.requests)) {
        throw new Error(`Invalid Runtime request ledger at ${this.filePath}.`);
      }
      for (const record of parsed.requests) {
        if (
          record !== null &&
          typeof record === 'object' &&
          typeof record.request?.request_id === 'string' &&
          (record.state === 'running' || record.state === 'completed') &&
          Array.isArray(record.events)
        ) {
          this.records.set(record.request.request_id, record);
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
    return [...this.records.values()].filter((record) => record.state === 'running');
  }

  public accept(request: RuntimeTurnRequest): RuntimeRequestRecord {
    const record: RuntimeRequestRecord = {
      request,
      fingerprint: requestFingerprint(request),
      state: 'running',
      events: [],
    };
    this.records.set(request.request_id, record);
    return record;
  }

  public complete(record: RuntimeRequestRecord): void {
    record.state = 'completed';
  }

  public persist(): Promise<void> {
    const write = this.writeChain
      .catch(() => undefined)
      .then(async () => {
        await mkdir(dirname(this.filePath), { recursive: true, mode: 0o700 });
        const temporaryPath = `${this.filePath}.${randomUUID()}.tmp`;
        await writeFile(
          temporaryPath,
          `${JSON.stringify({ version: 1, requests: [...this.records.values()] } satisfies RequestLedgerFile)}\n`,
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
  });
}
