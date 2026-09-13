import { spawnSync } from 'node:child_process';
import { mkdirSync, rmSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outputDirectory = resolve(projectRoot, '.native-test');
const executable = resolve(outputDirectory, 'process-exec-test');
const sourceDirectory = resolve(projectRoot, 'src/capabilities/tools/process');
const testSource = resolve(projectRoot, 'tests/native/process_exec_test.cpp');

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: projectRoot,
    encoding: 'utf8',
  });

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`${command} failed:\n${result.stdout}${result.stderr}`);
  }
}

mkdirSync(outputDirectory, { recursive: true });

try {
  run('g++', [
    '-std=c++23',
    '-Wall',
    '-Wextra',
    '-Werror',
    '-pedantic',
    resolve(sourceDirectory, 'exec.cpp'),
    testSource,
    '-o',
    executable,
  ]);
  run(executable, []);
} finally {
  rmSync(outputDirectory, { recursive: true, force: true });
}
