import { spawnSync } from 'node:child_process';
import { mkdirSync, readdirSync, rmSync, copyFileSync, chmodSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outputDirectory = resolve(projectRoot, '.native-test');
const systemExecutable = resolve(outputDirectory, 'system-info-test');
const filesystemExecutable = resolve(outputDirectory, 'filesystem-test');
const systemInfoExecutable = resolve(projectRoot, 'src/capabilities/tools/system/info/runtime');
const filesystemTestSource = resolve(projectRoot, 'tests/native/filesystem_test.cpp');
const bridgeExecutable = resolve(projectRoot, 'src/capabilities/runtime/bridge/runtime');
const loaderExecutable = resolve(outputDirectory, 'capability-loader-test');
const discoveryExecutable = resolve(outputDirectory, 'discovery-test');
const gitExecutable = resolve(outputDirectory, 'git-test');
const executorExecutable = resolve(outputDirectory, 'capability-executor-test');
const commandRunnerExecutable = resolve(outputDirectory, 'command-runner-test');
const capabilitiesDirectory = resolve(projectRoot, 'src/capabilities');
const coreDirectory = resolve(capabilitiesDirectory, 'core');
const executableRuntimeDirectory = resolve(capabilitiesDirectory, 'runtime/executable');
const bridgeRuntimeDirectory = resolve(capabilitiesDirectory, 'runtime/bridge');
const lspDirectory = resolve(capabilitiesDirectory, 'tools/lsp');
const lspRuntime = resolve(lspDirectory, 'diagnostics/runtime');
const webFetchDirectory = resolve(capabilitiesDirectory, 'tools/web/fetch');
const webFetchRuntime = resolve(webFetchDirectory, 'runtime');
const webSearchDirectory = resolve(capabilitiesDirectory, 'tools/web/search');
const webSearchRuntime = resolve(webSearchDirectory, 'runtime');
const shellDirectory = resolve(capabilitiesDirectory, 'tools/shell/exec');
const shellRuntime = resolve(shellDirectory, 'runtime');
const systemInfoDirectory = resolve(capabilitiesDirectory, 'tools/system/info');
const filesystemDirectory = resolve(capabilitiesDirectory, 'tools/filesystem');
const filesystemRuntimeSource = resolve(filesystemDirectory, 'filesystem.cpp');
const shellTestSource = resolve(projectRoot, 'tests/native/shell_exec_test.cpp');
const shellTestExecutable = resolve(outputDirectory, 'shell-exec-test');
const gitDirectory = resolve(capabilitiesDirectory, 'tools/git');
const systemTestSource = resolve(projectRoot, 'tests/native/system_info_test.cpp');
const loaderTestSource = resolve(projectRoot, 'tests/native/capability_loader_test.cpp');
const discoveryTestSource = resolve(projectRoot, 'tests/native/discovery_test.cpp');
const gitTestSource = resolve(projectRoot, 'tests/native/git_test.cpp');
const executorTestSource = resolve(projectRoot, 'tests/native/capability_executor_test.cpp');
const commandRunnerTestSource = resolve(projectRoot, 'tests/native/command_runner_test.cpp');

function collectTestFiles(directory) {
  return readdirSync(directory, { withFileTypes: true })
    .flatMap((entry) => {
      const path = resolve(directory, entry.name);
      return entry.isDirectory() ? collectTestFiles(path) : path.endsWith('.test.ts') ? [path] : [];
    })
    .sort();
}

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
    resolve(executableRuntimeDirectory, 'protocol.cpp'),
    resolve(executableRuntimeDirectory, 'adapter.cpp'),
    resolve(executableRuntimeDirectory, 'main.cpp'),
    resolve(coreDirectory, 'spawn.cpp'),
    resolve(coreDirectory, 'arguments.cpp'),
    resolve(coreDirectory, 'command_runner.cpp'),
    resolve(shellDirectory, 'shell.cpp'),
    '-o',
    shellRuntime,
  ]);
  run('cargo', ['build', '--locked', '--manifest-path', resolve(lspDirectory, 'Cargo.toml')]);
  // rm+copy em vez de sobrescrever: o daemon pode estar executando o binario.
  rmSync(lspRuntime, { force: true });
  copyFileSync(resolve(lspDirectory, 'target/debug/atlas-lsp-runtime'), lspRuntime);
  chmodSync(lspRuntime, 0o755);
  run('cargo', ['build', '--locked', '--manifest-path', resolve(webFetchDirectory, 'Cargo.toml')]);
  rmSync(webFetchRuntime, { force: true });
  copyFileSync(resolve(webFetchDirectory, 'target/debug/atlas-web-fetch-runtime'), webFetchRuntime);
  chmodSync(webFetchRuntime, 0o755);
  run('npx', [
    'tsc',
    resolve(webSearchDirectory, 'search.ts'),
    '--target',
    'es2022',
    '--module',
    'nodenext',
    '--moduleResolution',
    'nodenext',
    '--strict',
    '--skipLibCheck',
    '--noEmitOnError',
    '--rootDir',
    webSearchDirectory,
    '--outDir',
    resolve(webSearchDirectory, '.web-search-build'),
  ]);
  copyFileSync(resolve(webSearchDirectory, '.web-search-build/search.js'), webSearchRuntime);
  rmSync(resolve(webSearchDirectory, '.web-search-build'), { recursive: true, force: true });
  chmodSync(webSearchRuntime, 0o755);
  run('g++', [
    '-std=c++23',
    '-Wall',
    '-Wextra',
    '-Werror',
    '-pedantic',
    '-pthread',
    resolve(executableRuntimeDirectory, 'protocol.cpp'),
    resolve(executableRuntimeDirectory, 'adapter.cpp'),
    resolve(executableRuntimeDirectory, 'main.cpp'),
    resolve(systemInfoDirectory, 'info.cpp'),
    '-o',
    systemInfoExecutable,
  ]);
  for (const [tool, toolSource] of [
    ['read', 'read.cpp'],
    ['list', 'list.cpp'],
    ['search', 'search.cpp'],
    ['glob', 'glob.cpp'],
    ['patch', 'patch.cpp'],
  ]) {
    run('g++', [
      '-std=c++23',
      '-Wall',
      '-Wextra',
      '-Werror',
      '-pedantic',
      '-pthread',
      resolve(executableRuntimeDirectory, 'protocol.cpp'),
      resolve(executableRuntimeDirectory, 'adapter.cpp'),
      resolve(executableRuntimeDirectory, 'main.cpp'),
      filesystemRuntimeSource,
      // O filesystem.cpp compartilhado referencia as regras de ignore da busca.
      resolve(filesystemDirectory, 'search/ignore.cpp'),
      resolve(filesystemDirectory, tool, toolSource),
      '-o',
      resolve(filesystemDirectory, tool, 'runtime'),
    ]);
  }
  run('g++', [
    '-std=c++23',
    '-Wall',
    '-Wextra',
    '-Werror',
    '-pedantic',
    '-pthread',
    resolve(coreDirectory, 'registry.cpp'),
    resolve(coreDirectory, 'discovery.cpp'),
    resolve(coreDirectory, 'executor.cpp'),
    resolve(coreDirectory, 'schema.cpp'),
    resolve(coreDirectory, 'loader.cpp'),
    resolve(executableRuntimeDirectory, 'protocol.cpp'),
    resolve(bridgeRuntimeDirectory, 'main.cpp'),
    '-o',
    bridgeExecutable,
  ]);
  run(resolve(projectRoot, 'node_modules/.bin/tsx'), [
    '--test',
    ...collectTestFiles(resolve(projectRoot, 'tests')),
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
    resolve(coreDirectory, 'discovery.cpp'),
    resolve(coreDirectory, 'loader.cpp'),
    discoveryTestSource,
    '-o',
    discoveryExecutable,
  ]);
  run('g++', [
    '-std=c++23',
    '-Wall',
    '-Wextra',
    '-Werror',
    '-pedantic',
    '-pthread',
    resolve(coreDirectory, 'spawn.cpp'),
    resolve(coreDirectory, 'command_runner.cpp'),
    resolve(gitDirectory, 'status/status.cpp'),
    resolve(gitDirectory, 'diff/diff.cpp'),
    gitTestSource,
    '-o',
    gitExecutable,
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
    resolve(coreDirectory, 'schema.cpp'),
    resolve(executableRuntimeDirectory, 'protocol.cpp'),
    resolve(coreDirectory, 'loader.cpp'),
    resolve(coreDirectory, 'spawn.cpp'),
    resolve(coreDirectory, 'arguments.cpp'),
    resolve(coreDirectory, 'command_runner.cpp'),
    resolve(shellDirectory, 'shell.cpp'),
    executorTestSource,
    '-o',
    executorExecutable,
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
    resolve(coreDirectory, 'executor.cpp'),
    resolve(coreDirectory, 'schema.cpp'),
    resolve(coreDirectory, 'loader.cpp'),
    resolve(executableRuntimeDirectory, 'protocol.cpp'),
    resolve(systemInfoDirectory, 'info.cpp'),
    systemTestSource,
    '-o',
    systemExecutable,
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
    resolve(coreDirectory, 'executor.cpp'),
    resolve(coreDirectory, 'schema.cpp'),
    resolve(coreDirectory, 'loader.cpp'),
    resolve(executableRuntimeDirectory, 'protocol.cpp'),
    filesystemRuntimeSource,
    resolve(filesystemDirectory, 'search/ignore.cpp'),
    filesystemTestSource,
    '-o',
    filesystemExecutable,
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
    resolve(coreDirectory, 'spawn.cpp'),
    resolve(coreDirectory, 'arguments.cpp'),
    resolve(coreDirectory, 'command_runner.cpp'),
    resolve(executableRuntimeDirectory, 'protocol.cpp'),
    resolve(shellDirectory, 'shell.cpp'),
    shellTestSource,
    '-o',
    shellTestExecutable,
  ]);
  run('g++', [
    '-std=c++23',
    '-Wall',
    '-Wextra',
    '-Werror',
    '-pedantic',
    '-pthread',
    resolve(coreDirectory, 'spawn.cpp'),
    resolve(coreDirectory, 'command_runner.cpp'),
    commandRunnerTestSource,
    '-o',
    commandRunnerExecutable,
  ]);
  run(shellTestExecutable, []);
  run(systemExecutable, []);
  run(filesystemExecutable, []);
  run(loaderExecutable, []);
  run(discoveryExecutable, []);
  run(gitExecutable, []);
  run(executorExecutable, []);
  run(commandRunnerExecutable, []);
} finally {
  rmSync(outputDirectory, { recursive: true, force: true });
}
