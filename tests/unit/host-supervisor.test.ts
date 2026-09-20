import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { HostSupervisor, OperationStore, type HostCommandResult } from '../../src/host/index.js';

async function fixture() {
  const root = await mkdtemp(join(tmpdir(), 'atlas-host-'));
  const repositoryRoot = join(root, 'repo');
  const stateDirectory = join(root, 'state');
  await mkdir(join(repositoryRoot, 'dist', 'runtime'), { recursive: true });
  await mkdir(join(repositoryRoot, 'src', 'prompts'), { recursive: true });
  await writeFile(join(repositoryRoot, 'dist', 'runtime', 'server.js'), 'candidate');
  await writeFile(join(repositoryRoot, 'src', 'prompts', 'default.txt'), 'prompt');
  return { root, repositoryRoot, stateDirectory };
}

function commandRunner(results: readonly HostCommandResult[]) {
  let index = 0;
  return async (): Promise<HostCommandResult> =>
    results[index++] ?? { exit_code: 0, stdout: '', stderr: '' };
}

function fakeProcess(healthy: boolean) {
  const listeners = new Map<string, () => void>();
  let exitCode: number | null = null;
  const process = {
    get exitCode() {
      return exitCode;
    },
    signalCode: null as NodeJS.Signals | null,
    kill: () => {
      exitCode = 0;
      listeners.get('exit')?.();
      return true;
    },
    crash: () => {
      exitCode = 1;
      listeners.get('exit')?.();
    },
    once: (event: string, listener: () => void) => {
      listeners.set(event, listener);
      return process;
    },
    healthy,
  };
  return process as never;
}

async function createSupervisor(
  results: readonly HostCommandResult[],
  healthchecks: readonly boolean[] = [true],
) {
  const paths = await fixture();
  let healthIndex = 0;
  const processes: unknown[] = [];
  const supervisor = new HostSupervisor({
    repositoryRoot: paths.repositoryRoot,
    stateDirectory: paths.stateDirectory,
    socketPath: join(paths.root, 'runtime.sock'),
    validationCommands: [{ program: 'validate', args: [] }],
    buildCommand: { program: 'build', args: [] },
    runCommand: commandRunner(results),
    spawnRuntime: () => {
      const process = fakeProcess(healthchecks[healthIndex] ?? false);
      processes.push(process);
      return process;
    },
    healthcheck: async () => healthchecks[healthIndex++] ?? false,
  });
  return { ...paths, supervisor, processes };
}

test('completa restart bem-sucedido e publica handoff sem detalhes internos', async () => {
  const { supervisor, stateDirectory, root } = await createSupervisor([
    { exit_code: 0, stdout: 'abc\n', stderr: '' },
    { exit_code: 0, stdout: '', stderr: '' },
    { exit_code: 0, stdout: '', stderr: '' },
  ]);
  const events: string[] = [];
  supervisor.subscribe((event) => events.push(event.type));

  const operation = await supervisor.handoff({
    request_id: 'request-1',
    conversation_id: 'conversation-1',
    objective: 'Verificar a alteração.',
    next_step: 'Executar os testes finais.',
  });

  assert.equal(operation.state, 'resuming');
  assert.deepEqual(events, ['runtime.restarting', 'runtime.ready', 'operation.resuming']);
  const stored = await new OperationStore(join(stateDirectory, 'operations.json')).get(
    operation.operation_id,
  );
  assert.equal(stored?.base_commit, 'abc');
  assert.equal(stored?.candidate_build, `candidate:${operation.operation_id}`);
  await rm(root, { recursive: true, force: true });
});

test('reinicia automaticamente após crash inesperado do Runtime', async () => {
  const { supervisor, stateDirectory, processes, root } = await createSupervisor([], [true, true]);
  await mkdir(join(stateDirectory, 'current', 'runtime'), { recursive: true });
  await writeFile(join(stateDirectory, 'current', 'runtime', 'server.js'), 'current');
  const events: string[] = [];
  supervisor.subscribe((event) => events.push(event.type));

  await supervisor.start();
  (processes[0] as { crash: () => void }).crash();
  await new Promise((resolve) => setTimeout(resolve, 10));

  assert.equal(processes.length, 2);
  assert.deepEqual(events, ['runtime.restarting', 'runtime.ready']);
  await supervisor.stop();
  await rm(root, { recursive: true, force: true });
});

test('faz rollback para o build anterior quando o healthcheck falha', async () => {
  const { supervisor, stateDirectory, root } = await createSupervisor(
    [
      { exit_code: 0, stdout: 'abc\n', stderr: '' },
      { exit_code: 0, stdout: '', stderr: '' },
      { exit_code: 0, stdout: '', stderr: '' },
    ],
    [false, true],
  );
  await mkdir(join(stateDirectory, 'current', 'runtime'), { recursive: true });
  await writeFile(join(stateDirectory, 'current', 'runtime', 'server.js'), 'previous');

  await assert.rejects(
    supervisor.handoff({
      request_id: 'request-2',
      conversation_id: 'conversation-2',
      objective: 'Falhar no healthcheck.',
      next_step: 'Corrigir e tentar novamente.',
    }),
    /healthcheck/,
  );

  const store = new OperationStore(join(stateDirectory, 'operations.json'));
  const operations = await store.pending();
  assert.equal(operations.length, 0);
  const operationFile = JSON.parse(
    await readFile(join(stateDirectory, 'operations.json'), 'utf8'),
  ) as {
    operations: Array<{ state: string; error?: string; operation_id: string }>;
  };
  assert.equal(operationFile.operations[0]?.state, 'rolled_back');
  assert.match(operationFile.operations[0]?.error ?? '', /healthcheck/);
  assert.equal(
    await readFile(join(stateDirectory, 'current', 'runtime', 'server.js'), 'utf8'),
    'previous',
  );
  assert.equal(
    await readFile(
      join(
        stateDirectory,
        'failed',
        operationFile.operations[0]?.operation_id ?? '',
        'runtime',
        'server.js',
      ),
      'utf8',
    ),
    'candidate',
  );
  await rm(root, { recursive: true, force: true });
});

test('marca a operação como falha quando o build candidato falha', async () => {
  const { supervisor, stateDirectory, root } = await createSupervisor([
    { exit_code: 0, stdout: 'abc\n', stderr: '' },
    { exit_code: 0, stdout: '', stderr: '' },
    { exit_code: 1, stdout: '', stderr: 'compiler error' },
  ]);

  await assert.rejects(
    supervisor.handoff({
      request_id: 'request-build-fail',
      conversation_id: 'conversation-build-fail',
      objective: 'Gerar o build candidato.',
      next_step: 'Corrigir o erro de compilação.',
    }),
    /Candidate build failed/,
  );

  const operationFile = JSON.parse(
    await readFile(join(stateDirectory, 'operations.json'), 'utf8'),
  ) as {
    operations: Array<{ state: string; error?: string }>;
  };
  assert.equal(operationFile.operations[0]?.state, 'failed');
  assert.match(operationFile.operations[0]?.error ?? '', /compiler error/);
  await rm(root, { recursive: true, force: true });
});

test('preserva a operação pendente para recuperação após crash', async () => {
  const paths = await fixture();
  const store = new OperationStore(join(paths.stateDirectory, 'operations.json'));
  const operation = await store.create({
    request_id: 'request-3',
    conversation_id: 'conversation-3',
    objective: 'Concluir a alteração.',
    state: 'checkpointed',
    base_commit: 'abc',
    candidate_build: 'candidate:crash',
    next_step: 'Rodar a verificação final.',
  });

  const pending = await store.pending('conversation-3', 'request-3');
  assert.equal(pending[0]?.operation_id, operation.operation_id);
  await rm(paths.root, { recursive: true, force: true });
});
