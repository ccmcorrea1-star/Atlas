import { copyFileSync, chmodSync, mkdirSync, statSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { run } from './build-native.mjs';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outputDirectory = resolve(projectRoot, '.web-tools-build');
const tools = {
  browser: {
    source: resolve(projectRoot, 'src/capabilities/tools/web/browser.ts'),
    compiled: resolve(outputDirectory, 'browser.js'),
    runtime: resolve(projectRoot, 'src/capabilities/tools/web/browser/runtime'),
  },
  crawl: {
    source: resolve(projectRoot, 'src/capabilities/tools/web/crawl.ts'),
    compiled: resolve(outputDirectory, 'crawl.js'),
    runtime: resolve(projectRoot, 'src/capabilities/tools/web/crawl/runtime'),
  },
};

function isFresh(path, input) {
  try {
    return statSync(path).mtimeMs >= statSync(input).mtimeMs;
  } catch {
    return false;
  }
}

export function ensureWebToolRuntime(name) {
  const tool = tools[name];
  if (tool === undefined) {
    throw new Error(`invalid web tool: ${name}`);
  }
  if (isFresh(tool.runtime, tool.source) && isFresh(tool.compiled, tool.source)) {
    return;
  }
  mkdirSync(outputDirectory, { recursive: true });
  run(resolve(projectRoot, 'node_modules/.bin/tsc'), [
    tool.source,
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
    resolve(outputDirectory, `${name}.tsbuildinfo`),
    '--rootDir',
    resolve(projectRoot, 'src/capabilities/tools/web'),
    '--outDir',
    outputDirectory,
  ]);
  copyFileSync(tool.compiled, tool.runtime);
  chmodSync(tool.runtime, 0o755);
}

const invokedPath = process.argv[1] === undefined ? '' : resolve(process.argv[1]);
if (import.meta.url === pathToFileURL(invokedPath).href) {
  ensureWebToolRuntime(process.argv[2]);
}
