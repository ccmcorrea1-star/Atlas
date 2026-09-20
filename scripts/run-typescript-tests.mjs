import { readdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { buildNativeTargets, run } from './build-native.mjs';
import { ensureWebSearchRuntime } from './build-web-search.mjs';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

function collectTestFiles(directory) {
  return readdirSync(directory, { withFileTypes: true })
    .flatMap((entry) => {
      const path = resolve(directory, entry.name);
      return entry.isDirectory() ? collectTestFiles(path) : path.endsWith('.test.ts') ? [path] : [];
    })
    .sort();
}

function testFiles(domain) {
  const unitTests = resolve(projectRoot, 'tests/unit');
  const webSearchTest = resolve(unitTests, 'web-search.test.ts');
  if (domain === 'web-search') {
    return [webSearchTest];
  }
  if (domain !== 'runtime') {
    throw new Error(`unknown TypeScript test domain: ${domain}`);
  }
  return collectTestFiles(resolve(projectRoot, 'tests')).filter((path) => path !== webSearchTest);
}

const domain = process.argv[2] ?? 'runtime';
if (domain === 'runtime') {
  buildNativeTargets('runtime');
} else if (domain === 'web-search') {
  ensureWebSearchRuntime();
}

run(resolve(projectRoot, 'node_modules/.bin/tsx'), ['--test', ...testFiles(domain)]);
