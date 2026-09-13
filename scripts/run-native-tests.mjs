import { spawnSync } from 'node:child_process';
import { mkdirSync, rmSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outputDirectory = resolve(projectRoot, '.native-test');
const processExecutable = resolve(outputDirectory, 'process-exec-test');
const loaderExecutable = resolve(outputDirectory, 'capability-loader-test');
const capabilitiesDirectory = resolve(projectRoot, 'src/capabilities');
const sourceDirectory = resolve(projectRoot, 'src/capabilities/tools/process');
const processTestSource = resolve(projectRoot, 'tests/native/process_exec_test.cpp');
const loaderTestSource = resolve(projectRoot, 'tests/native/capability_loader_test.cpp');

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
    resolve(capabilitiesDirectory, 'registry.cpp'),
    resolve(capabilitiesDirectory, 'discovery.cpp'),
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
    resolve(capabilitiesDirectory, 'registry.cpp'),
    resolve(capabilitiesDirectory, 'discovery.cpp'),
    resolve(capabilitiesDirectory, 'loader.cpp'),
    loaderTestSource,
    '-o',
    loaderExecutable,
  ]);
  run(processExecutable, []);
  run(loaderExecutable, []);
} finally {
  rmSync(outputDirectory, { recursive: true, force: true });
}
