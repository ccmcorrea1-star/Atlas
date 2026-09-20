import { copyFileSync, chmodSync, mkdirSync, statSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { run } from './build-native.mjs';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const source = resolve(projectRoot, 'src/capabilities/tools/web/search/search.ts');
const outputDirectory = resolve(projectRoot, '.web-search-build');
const compiled = resolve(outputDirectory, 'search.js');
const runtime = resolve(projectRoot, 'src/capabilities/tools/web/search/runtime');

function isFresh(path, input) {
  try {
    return statSync(path).mtimeMs >= statSync(input).mtimeMs;
  } catch {
    return false;
  }
}

export function ensureWebSearchRuntime() {
  if (isFresh(runtime, source) && isFresh(compiled, source)) {
    return;
  }

  mkdirSync(outputDirectory, { recursive: true });
  run(resolve(projectRoot, 'node_modules/.bin/tsc'), [
    source,
    '--target',
    'es2022',
    '--module',
    'nodenext',
    '--moduleResolution',
    'nodenext',
    '--strict',
    '--skipLibCheck',
    '--noEmitOnError',
    '--incremental',
    '--tsBuildInfoFile',
    resolve(outputDirectory, 'tsconfig.tsbuildinfo'),
    '--rootDir',
    resolve(projectRoot, 'src/capabilities/tools/web/search'),
    '--outDir',
    outputDirectory,
  ]);
  copyFileSync(compiled, runtime);
  chmodSync(runtime, 0o755);
}

const invokedPath = process.argv[1] === undefined ? '' : resolve(process.argv[1]);
if (import.meta.url === pathToFileURL(invokedPath).href) {
  ensureWebSearchRuntime();
}
