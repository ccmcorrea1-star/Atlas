import assert from 'node:assert/strict';
import { spawn, type ChildProcess } from 'node:child_process';
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { test } from 'node:test';

import { NativeCapabilityRuntime } from '../../src/capability-runtime.js';

type BridgeRecord = Record<string, unknown>;

const bridgePath = resolve(process.cwd(), 'src/capabilities/runtime/bridge/runtime');
const fakeBridgePath = resolve(process.cwd(), 'tests/support/fake-capability-bridge.mjs');

function openBridge(executablePath: string): {
  child: ChildProcess;
  send: (payload: BridgeRecord) => Promise<BridgeRecord>;
} {
  const child = spawn(executablePath, [], { stdio: ['pipe', 'pipe', 'pipe'] });
  child.stdout?.setEncoding('utf8');
  let buffer = '';
  const waiters = new Map<string, (record: BridgeRecord) => void>();

  child.stdout?.on('data', (chunk: string) => {
    buffer += chunk;
    let newline = buffer.indexOf('\n');
    while (newline !== -1) {
      const line = buffer.slice(0, newline).trim();
      buffer = buffer.slice(newline + 1);
      newline = buffer.indexOf('\n');
      if (!line) {
        continue;
      }
      const record = JSON.parse(line) as BridgeRecord;
      const requestId = typeof record.request_id === 'string' ? record.request_id : undefined;
      if (requestId === undefined) {
        continue;
      }
      const waiter = waiters.get(requestId);
      if (waiter !== undefined) {
        waiters.delete(requestId);
        waiter(record);
      }
    }
  });

  const send = (payload: BridgeRecord) =>
    new Promise<BridgeRecord>((resolveRecord) => {
      waiters.set(String(payload.request_id), resolveRecord);
      child.stdin?.write(`${JSON.stringify(payload)}\n`);
    });

  return { child, send };
}

function closeBridge(child: ChildProcess): Promise<void> {
  return new Promise<void>((resolveClose) => {
    if (child.exitCode !== null || child.signalCode !== null) {
      resolveClose();
      return;
    }
    child.once('close', () => resolveClose());
    child.stdin?.end();
  });
}

function writeWrapper(
  directory: string,
  name: string,
  options: { logPath?: string; crashMarker?: string },
): string {
  const lines = ['#!/bin/sh'];
  if (options.logPath !== undefined) {
    lines.push(`printf 'start\\n' >> '${options.logPath}'`);
  }
  if (options.crashMarker !== undefined) {
    lines.push(
      `if [ ! -f '${options.crashMarker}' ]; then printf 'crashed\\n' > '${options.crashMarker}'; exit 1; fi`,
    );
  }
  lines.push(`exec '${bridgePath}'`);
  const wrapper = resolve(directory, name);
  writeFileSync(wrapper, `${lines.join('\n')}\n`);
  chmodSync(wrapper, 0o755);
  return wrapper;
}

test('answers multiple requests on one bridge process and echoes request_id', async () => {
  const { child, send } = openBridge(bridgePath);
  try {
    const discover = await send({
      request_id: 'req-discover',
      operation: 'discover',
      query: 'executar programa',
    });
    assert.equal(discover.request_id, 'req-discover');
    assert.ok(Array.isArray(discover.results));

    const definition = await send({
      request_id: 'req-definition',
      operation: 'get_definition',
      id: 'process.exec',
    });
    assert.equal(definition.request_id, 'req-definition');
    assert.equal((definition.definition as BridgeRecord).id, 'process.exec');

    // Um erro de protocolo não encerra o processo residente.
    const unknown = await send({ request_id: 'req-unknown', operation: 'nope' });
    assert.equal(unknown.request_id, 'req-unknown');
    assert.match(String(unknown.error), /unknown capability bridge operation/);

    const missing = await send({
      request_id: 'req-missing',
      operation: 'execute',
      id: 'missing.tool',
      target: 'local',
      arguments: {},
    });
    assert.equal(missing.request_id, 'req-missing');
    assert.match(String(missing.error), /is not registered/);
    assert.equal(missing.status, undefined);

    const execute = await send({
      request_id: 'req-execute',
      operation: 'execute',
      id: 'process.exec',
      target: 'local',
      arguments: { program: '/bin/echo', args: ['persistent'] },
    });
    assert.equal(execute.request_id, 'req-execute');
    assert.equal(execute.status, 'success');
    assert.equal(execute.stdout, 'persistent\n');
    assert.equal(child.exitCode, null);
  } finally {
    await closeBridge(child);
  }
});

test('reuses one bridge process for sequential client requests', async () => {
  const directory = mkdtempSync(resolve(tmpdir(), 'atlas-bridge-reuse-'));
  const logPath = resolve(directory, 'starts.log');
  const wrapper = writeWrapper(directory, 'reuse.sh', { logPath });
  const runtime = new NativeCapabilityRuntime({ executablePath: wrapper });
  try {
    await runtime.discover({ query: 'executar programa' });
    await runtime.getDefinition('process.exec');
    await runtime.execute('process.exec', 'local', { program: '/bin/true', args: [] });

    const starts = readFileSync(logPath, 'utf8').trim().split('\n');
    assert.equal(starts.length, 1);
  } finally {
    await runtime.close();
    rmSync(directory, { recursive: true, force: true });
  }
});

test('correlates concurrent responses by request_id', async () => {
  chmodSync(fakeBridgePath, 0o755);
  process.env.ATLAS_FAKE_DISCOVER_DELAY_MS = '60';
  const runtime = new NativeCapabilityRuntime({ executablePath: fakeBridgePath });
  try {
    const order: string[] = [];
    const discoverPromise = runtime.discover().then((results) => {
      order.push('discover');
      return results;
    });
    const definitionPromise = runtime.getDefinition('fake.tool').then((definition) => {
      order.push('definition');
      return definition;
    });

    const [results, definition] = await Promise.all([discoverPromise, definitionPromise]);
    assert.deepEqual(results, [{ id: 'fake.tool', type: 'tool', summary: 'fake tool' }]);
    assert.equal(definition?.id, 'fake.tool');
    // A resposta atrasada do discover chega depois; a correlação ignora a ordem.
    assert.deepEqual(order, ['definition', 'discover']);
  } finally {
    await runtime.close();
    delete process.env.ATLAS_FAKE_DISCOVER_DELAY_MS;
  }
});

test('rejects pending requests on crash and restarts cleanly', async () => {
  const directory = mkdtempSync(resolve(tmpdir(), 'atlas-bridge-crash-'));
  const crashMarker = resolve(directory, 'crashed');
  const wrapper = writeWrapper(directory, 'crash-once.sh', { crashMarker });
  const runtime = new NativeCapabilityRuntime({ executablePath: wrapper });
  try {
    await assert.rejects(runtime.discover(), (error: unknown) => error instanceof Error);
    assert.equal(existsSync(crashMarker), true);

    // A próxima request reinicia o bridge do zero.
    const results = await runtime.discover({ query: 'executar programa' });
    assert.ok(results.some((result) => result.id === 'process.exec'));
  } finally {
    await runtime.close();
    rmSync(directory, { recursive: true, force: true });
  }
});

test('preserves streaming deltas before resolving a persistent request', async () => {
  chmodSync(fakeBridgePath, 0o755);
  const runtime = new NativeCapabilityRuntime({ executablePath: fakeBridgePath });
  try {
    const deltas: Array<{ channel: string; delta: string }> = [];
    const result = await runtime.execute(
      'fake.tool',
      'local',
      {},
      {
        onOutput: async (channel, delta) => {
          await new Promise((resolveDelay) => setTimeout(resolveDelay, 5));
          deltas.push({ channel, delta });
        },
      },
    );

    assert.deepEqual(deltas, [
      { channel: 'stdout', delta: 'first' },
      { channel: 'stderr', delta: 'second' },
    ]);
    assert.equal(result.status, 'success');
    assert.deepEqual(result.output, {
      stdout: 'complete',
      stderr: '',
      exit_code: 0,
      duration_ms: 1,
    });
  } finally {
    await runtime.close();
  }
});
