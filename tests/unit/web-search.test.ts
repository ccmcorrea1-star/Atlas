import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import {
  buildCommand,
  parseRequest,
  runSearch,
  sanitizeResults,
  splitCommand,
} from '../../src/capabilities/tools/web/search/search.js';

function stubProvider(body: string): string {
  const directory = mkdtempSync(join(tmpdir(), 'atlas-web-search-'));
  const script = join(directory, 'provider.mjs');
  writeFileSync(script, `process.stdout.write(${JSON.stringify(body)});\n`);
  return `${process.execPath} ${script}`;
}

test('parseRequest extrai query e aplica limite padrão', () => {
  assert.deepEqual(parseRequest('{"target":"local","query":"atlas"}'), {
    target: 'local',
    query: 'atlas',
    limit: 5,
  });
});

test('parseRequest limita ao máximo e rejeita entradas inválidas', () => {
  assert.equal(parseRequest('{"target":"local","query":"a","limit":100}').limit, 20);
  assert.throws(() => parseRequest('{"target":"local","query":"  "}'), /non-empty/);
  assert.throws(() => parseRequest('{"target":"local"}'), /query/);
  assert.throws(() => parseRequest('{"target":"local","query":"a","limit":0}'), /limit/);
  assert.throws(() => parseRequest('{invalido}'), /invalid JSON/);
});

test('splitCommand respeita aspas', () => {
  assert.deepEqual(splitCommand(`bin --opt 'a b' "c d" e`), ['bin', '--opt', 'a b', 'c d', 'e']);
});

test('buildCommand substitui placeholders ou anexa query e limit', () => {
  assert.deepEqual(buildCommand('bin --q {query} --n {limit}', 'oi', 3), [
    'bin',
    '--q',
    'oi',
    '--n',
    '3',
  ]);
  assert.deepEqual(buildCommand('bin', 'oi', 3), ['bin', 'oi', '3']);
  assert.throws(() => buildCommand('   ', 'oi', 3), /empty/);
});

test('sanitizeResults mantém só entradas válidas', () => {
  assert.deepEqual(
    sanitizeResults([
      { title: 'T', url: 'https://x.test', snippet: 'S', source: 'w' },
      { title: '', url: 'https://x.test' },
      { title: 'Sem url' },
      'texto',
      42,
    ]),
    [{ title: 'T', url: 'https://x.test', snippet: 'S', source: 'w' }],
  );
  assert.throws(() => sanitizeResults({}), /array/);
});

test('runSearch retorna unavailable sem provider', () => {
  const saved = process.env.ATLAS_WEB_SEARCH_COMMAND;
  delete process.env.ATLAS_WEB_SEARCH_COMMAND;
  try {
    assert.deepEqual(runSearch('atlas', 5, undefined), {
      status: 'unavailable',
      error: 'no search provider configured',
      results: [],
    });
  } finally {
    if (saved !== undefined) {
      process.env.ATLAS_WEB_SEARCH_COMMAND = saved;
    }
  }
});

test('runSearch retorna resultados reais do provider', () => {
  const provider = stubProvider(
    JSON.stringify([{ title: 'T', url: 'https://x.test', snippet: 'S', source: 'w' }]),
  );
  const outcome = runSearch('atlas', 5, provider);
  assert.equal(outcome.status, 'success');
  assert.equal(outcome.error, '');
  assert.deepEqual(outcome.results, [
    { title: 'T', url: 'https://x.test', snippet: 'S', source: 'w' },
  ]);
});

test('runSearch nunca inventa resultados', () => {
  const outcome = runSearch('atlas', 5, stubProvider('[]'));
  assert.deepEqual(outcome, { status: 'success', error: '', results: [] });
});

test('runSearch falha com provider inválido', () => {
  assert.match(runSearch('atlas', 5, stubProvider('não json')).error, /invalid JSON/);
  const failing = runSearch('atlas', 5, `${process.execPath} --eval "process.exit(3)"`);
  assert.equal(failing.status, 'failed');
  assert.match(failing.error, /exited with code 3/);
});
