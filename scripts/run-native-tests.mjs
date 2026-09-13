import { spawnSync } from 'node:child_process';
import { mkdirSync, rmSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outputDirectory = resolve(projectRoot, '.native-test');
const processExecutable = resolve(outputDirectory, 'process-exec-test');
const capabilityExecutable = resolve(
  projectRoot,
  'src/capabilities/tools/process/exec/implementation',
);
const loaderExecutable = resolve(outputDirectory, 'capability-loader-test');
const executorExecutable = resolve(outputDirectory, 'capability-executor-test');
const capabilitiesDirectory = resolve(projectRoot, 'src/capabilities');
const coreDirectory = resolve(capabilitiesDirectory, 'core');
const sourceDirectory = resolve(capabilitiesDirectory, 'tools/process/exec');
const processTestSource = resolve(projectRoot, 'tests/native/process_exec_test.cpp');
const loaderTestSource = resolve(projectRoot, 'tests/native/capability_loader_test.cpp');
const executorTestSource = resolve(projectRoot, 'tests/native/capability_executor_test.cpp');

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
    '-pthread',
    resolve(coreDirectory, 'registry.cpp'),
    resolve(coreDirectory, 'executor.cpp'),
    resolve(sourceDirectory, 'implementation.cpp'),
    resolve(sourceDirectory, 'exec.cpp'),
    '-o',
    capabilityExecutable,
  ]);
  run('g++', [
    '-std=c++23',
    '-Wall',
    '-Wextra',
    '-Werror',
    '-pedantic',
    '-pthread',
    resolve(coreDirectory, 'registry.cpp'),
    resolve(coreDirectory, 'discovery.cpp'),
    resolve(coreDirectory, 'loader.cpp'),
    resolve(sourceDirectory, 'exec.cpp'),
    processTestSource,
    '-o',
    processExecutable,
  ]);
  run('g++', [
    '-std=c++23',
    '-Wall',
    '-Wextra',
    '-Werror',
    '-pedantic',
    '-pthread',
    resolve(coreDirectory, 'registry.cpp'),
    resolve(coreDirectory, 'discovery.cpp'),
    resolve(coreDirectory, 'loader.cpp'),
    loaderTestSource,
    '-o',
    loaderExecutable,
  ]);
  run('g++', [
    '-std=c++23',
    '-Wall',
    '-Wextra',
    '-Werror',
    '-pedantic',
    '-pthread',
    resolve(coreDirectory, 'registry.cpp'),
    resolve(coreDirectory, 'executor.cpp'),
    resolve(coreDirectory, 'loader.cpp'),
    resolve(sourceDirectory, 'exec.cpp'),
    executorTestSource,
    '-o',
    executorExecutable,
  ]);
  run(processExecutable, []);
  run(loaderExecutable, []);
  run(executorExecutable, []);
} finally {
  rmSync(outputDirectory, { recursive: true, force: true });
  rmSync(capabilityExecutable, { force: true });
}
