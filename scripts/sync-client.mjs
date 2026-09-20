import { copyFileSync, chmodSync, existsSync, mkdirSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { run } from './build-native.mjs';

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const source = resolve(projectRoot, 'clients/tui/target/debug/atlas');
const cargoHome = process.env.CARGO_HOME ?? join(homedir(), '.cargo');
const installRoot = process.env.CARGO_INSTALL_ROOT ?? cargoHome;
const destination = join(installRoot, 'bin', 'atlas');

run('cargo', ['build', '--locked', '--manifest-path', 'clients/tui/Cargo.toml']);

const shouldCopy =
  !existsSync(destination) || statSync(source).mtimeMs > statSync(destination).mtimeMs;
if (shouldCopy) {
  mkdirSync(dirname(destination), { recursive: true });
  copyFileSync(source, destination);
  chmodSync(destination, 0o755);
}
