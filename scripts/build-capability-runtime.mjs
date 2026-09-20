import { copyFileSync, chmodSync, rmSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { run } from './build-native.mjs';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

const runtimes = {
  lsp: {
    manifest: 'src/capabilities/tools/lsp/Cargo.toml',
    binary: 'atlas-lsp-runtime',
    output: 'src/capabilities/tools/lsp/diagnostics/runtime',
  },
  'web-fetch': {
    manifest: 'src/capabilities/tools/web/fetch/Cargo.toml',
    binary: 'atlas-web-fetch-runtime',
    output: 'src/capabilities/tools/web/fetch/runtime',
  },
};

export function buildCapabilityRuntime(name, profile = 'release') {
  const runtime = runtimes[name];
  if (runtime === undefined || (profile !== 'debug' && profile !== 'release')) {
    throw new Error(`invalid capability runtime: ${name} (${profile})`);
  }

  run('cargo', [
    'build',
    '--locked',
    ...(profile === 'release' ? ['--release'] : []),
    '--manifest-path',
    runtime.manifest,
  ]);

  const binary = resolve(
    projectRoot,
    runtime.manifest.replace('Cargo.toml', `target/${profile}/${runtime.binary}`),
  );
  const output = resolve(projectRoot, runtime.output);
  // Remove antes de copiar para não substituir um processo em execução.
  rmSync(output, { force: true });
  copyFileSync(binary, output);
  chmodSync(output, 0o755);
}

const invokedPath = process.argv[1] === undefined ? '' : resolve(process.argv[1]);
if (import.meta.url === pathToFileURL(invokedPath).href) {
  buildCapabilityRuntime(process.argv[2], process.argv[3] ?? 'release');
}
