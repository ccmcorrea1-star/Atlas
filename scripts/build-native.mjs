import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const buildDirectory = '.native-cmake';

const nativeTestTargets = [
  'atlas-shell-exec-test',
  'atlas-system-info-test',
  'atlas-filesystem-test',
  'atlas-git-test',
  'atlas-capability-loader-test',
  'atlas-capability-discovery-test',
  'atlas-capability-executor-test',
  'atlas-command-runner-test',
];

const targetGroups = {
  all: undefined,
  runtime: ['atlas-shell-exec-runtime', 'atlas-capability-bridge-runtime'],
  native: nativeTestTargets,
};

export function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: projectRoot,
    encoding: 'utf8',
    stdio: ['inherit', 'pipe', 'pipe'],
  });

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`${command} failed:\n${result.stdout}${result.stderr}`);
  }
  if (result.stdout) {
    process.stdout.write(result.stdout);
  }
  if (result.stderr) {
    process.stderr.write(result.stderr);
  }
}

export function buildNativeTargets(group) {
  const targets = targetGroups[group];
  if (targets === undefined && group !== 'all') {
    throw new Error(`unknown native build group: ${group}`);
  }

  run('cmake', ['-S', '.', '-B', buildDirectory]);
  run('cmake', [
    '--build',
    buildDirectory,
    ...(targets === undefined ? [] : ['--target', ...targets]),
  ]);
}

const invokedPath = process.argv[1] === undefined ? '' : resolve(process.argv[1]);
if (import.meta.url === pathToFileURL(invokedPath).href) {
  buildNativeTargets(process.argv[2] ?? 'all');
}
