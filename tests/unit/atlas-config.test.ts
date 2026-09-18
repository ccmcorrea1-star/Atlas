import assert from 'node:assert/strict';
import { readFile, stat, writeFile, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import {
  atlasApiKey,
  atlasRuntimeSessionData,
  AtlasConfigError,
  DEFAULT_ATLAS_CONFIG,
  loadAtlasConfig,
  normalizeAtlasModel,
  parseAtlasConfig,
  resolveAtlasConfigPath,
} from '../../src/config/index.js';

async function tempConfig(
  content: string,
): Promise<{ path: string; cleanup: () => Promise<void> }> {
  const directory = await mkdtemp(join(tmpdir(), 'atlas-config-test-'));
  const path = join(directory, 'config.json');
  await writeFile(path, content, 'utf8');
  return { path, cleanup: () => rm(directory, { recursive: true, force: true }) };
}

test('resolves ATLAS_CONFIG before the other standard paths', () => {
  assert.equal(
    resolveAtlasConfigPath({ ATLAS_CONFIG: ' /tmp/custom-atlas.json ' }, '/home/user'),
    '/tmp/custom-atlas.json',
  );
});

test('resolves XDG_CONFIG_HOME/atlas/config.json when XDG_CONFIG_HOME is set', () => {
  assert.equal(
    resolveAtlasConfigPath({ XDG_CONFIG_HOME: '/home/user/.local/config' }, '/home/user'),
    join('/home/user/.local/config', 'atlas', 'config.json'),
  );
});

test('falls back to ~/.config/atlas/config.json without environment overrides', () => {
  assert.equal(
    resolveAtlasConfigPath({}, '/home/user'),
    join('/home/user', '.config', 'atlas', 'config.json'),
  );
});

test('returns the current defaults when the config file is absent', async () => {
  const path = join(tmpdir(), `atlas-config-missing-${Date.now()}`, 'config.json');
  const loaded = await loadAtlasConfig(path);

  assert.equal(loaded.source, 'defaults');
  assert.deepEqual(loaded.config, DEFAULT_ATLAS_CONFIG);

  // O arquivo ausente continua ausente; o loader não cria configuração.
  let statError: NodeJS.ErrnoException | undefined;
  try {
    await stat(path);
  } catch (error) {
    statError = error as NodeJS.ErrnoException;
  }
  assert.equal(statError?.code, 'ENOENT');
});

test('loads a valid configuration file under ATLAS_CONFIG', async () => {
  const { path, cleanup } = await tempConfig(
    JSON.stringify({ version: 1, provider: 'opencode-go', model: 'model-test' }),
  );
  try {
    const loaded = await loadAtlasConfig(path);
    assert.equal(loaded.source, 'file');
    assert.deepEqual(loaded.config, {
      version: 1,
      provider: 'opencode-go',
      model: 'model-test',
    });
    assert.deepEqual(atlasRuntimeSessionData(loaded.config), {
      provider: 'opencode-go',
      model: 'model-test',
    });
  } finally {
    await cleanup();
  }
});

test('rejects an invalid JSON configuration', async () => {
  const { path, cleanup } = await tempConfig('{ not json ');
  try {
    await assert.rejects(loadAtlasConfig(path), (error: unknown) => {
      assert.ok(error instanceof AtlasConfigError);
      assert.match(error.message, /is not valid JSON/);
      assert.match(error.message, new RegExp(path.slice(1)));
      return true;
    });
  } finally {
    await cleanup();
  }
});

for (const missingField of ['version', 'provider', 'model'] as const) {
  test(`rejects configuration without "${missingField}"`, async () => {
    const fields = {
      version: 1,
      provider: 'opencode-go',
      model: 'model-test',
    };
    // Copia os campos menos o que o caso exige ausente.
    const partial = Object.fromEntries(
      Object.entries(fields).filter(([key]) => key !== missingField),
    );
    const { path, cleanup } = await tempConfig(JSON.stringify(partial));
    try {
      await assert.rejects(loadAtlasConfig(path), AtlasConfigError);
      assert.throws(() => parseAtlasConfig(partial, path), AtlasConfigError);
    } finally {
      await cleanup();
    }
  });
}

test('rejects an unknown version', async () => {
  const config = { version: 2, provider: 'opencode-go', model: 'model-test' };
  const { path, cleanup } = await tempConfig(JSON.stringify(config));
  try {
    await assert.rejects(loadAtlasConfig(path), (error: unknown) => {
      assert.ok(error instanceof AtlasConfigError);
      assert.match(error.message, /unsupported version 2/);
      return true;
    });
  } finally {
    await cleanup();
  }
});

test('rejects a non-integer version', () => {
  assert.throws(
    () => parseAtlasConfig({ version: '1', provider: 'opencode-go', model: 'x' }, 'inline'),
    AtlasConfigError,
  );
});

test('rejects empty provider and model strings', () => {
  assert.throws(
    () => parseAtlasConfig({ version: 1, provider: ' ', model: 'x' }, 'inline'),
    /"provider" must be a non-empty string/,
  );
  assert.throws(
    () => parseAtlasConfig({ version: 1, provider: 'opencode-go', model: '' }, 'inline'),
    /"model" must be a non-empty string/,
  );
});

test('rejects unsupported providers before the Runtime starts a session', () => {
  assert.throws(
    () => atlasRuntimeSessionData({ version: 1, provider: 'anthropic', model: 'claude-x' }),
    (error: unknown) => {
      assert.ok(error instanceof AtlasConfigError);
      assert.match(error.message, /"anthropic" is not supported yet/);
      assert.match(error.message, /"opencode-go" is currently supported/);
      return true;
    },
  );
});

test('rejects unknown top-level fields instead of accepting bare credentials', async () => {
  const config = {
    version: 1,
    provider: 'opencode-go',
    model: 'model-test',
    apiKey: 'should-not-be-here',
  };
  const { path, cleanup } = await tempConfig(JSON.stringify(config));
  try {
    await assert.rejects(loadAtlasConfig(path), (error: unknown) => {
      assert.ok(error instanceof AtlasConfigError);
      assert.match(error.message, /unsupported field\(s\): apiKey/);
      assert.match(error.message, /API keys belong under "providers"/);
      return true;
    });
  } finally {
    await cleanup();
  }
});

test('resolves the API key from the config file without environment overrides', async () => {
  const { path, cleanup } = await tempConfig(
    JSON.stringify({
      version: 1,
      provider: 'opencode-go',
      model: 'model-test',
      providers: { 'opencode-go': { apiKey: 'atlas-config-key' } },
    }),
  );
  try {
    const { config } = await loadAtlasConfig(path);
    assert.deepEqual(atlasApiKey(config, {}), {
      apiKey: 'atlas-config-key',
      origin: 'config',
    });
  } finally {
    await cleanup();
  }
});

test('the environment override beats the key from the config file', async () => {
  const { path, cleanup } = await tempConfig(
    JSON.stringify({
      version: 1,
      provider: 'opencode-go',
      model: 'model-test',
      providers: { 'opencode-go': { apiKey: 'atlas-config-key' } },
    }),
  );
  try {
    const { config } = await loadAtlasConfig(path);
    assert.deepEqual(atlasApiKey(config, { OPENCODE_GO_API_KEY: 'atlas-env-key' }), {
      apiKey: 'atlas-env-key',
      origin: 'environment',
    });
  } finally {
    await cleanup();
  }
});

test('requires an API key when the environment and the config provide none', async () => {
  const { path, cleanup } = await tempConfig(
    JSON.stringify({ version: 1, provider: 'opencode-go', model: 'model-test' }),
  );
  try {
    const { config } = await loadAtlasConfig(path);
    await assert.throws(
      () => atlasApiKey(config, {}),
      (error: unknown) => {
        assert.ok(error instanceof AtlasConfigError);
        assert.match(error.message, /no API key for provider "opencode-go"/);
        assert.match(error.message, /OPENCODE_GO_API_KEY/);
        assert.match(error.message, /providers\.opencode-go\.apiKey/);
        return true;
      },
    );
  } finally {
    await cleanup();
  }
});

test('accepts a config without "providers" for the credential release', async () => {
  const { path, cleanup } = await tempConfig(
    JSON.stringify({ version: 1, provider: 'opencode-go', model: 'model-test' }),
  );
  try {
    const loaded = await loadAtlasConfig(path);
    assert.equal(loaded.config.providers, undefined);
  } finally {
    await cleanup();
  }
});

test('rejects invalid "providers" shapes with clear errors', async () => {
  const cases: unknown[] = [
    { providers: 'not-an-object' },
    { providers: { anthropic: { apiKey: 'other-provider' } } },
    { providers: { 'opencode-go': 'not-an-object' } },
    { providers: { 'opencode-go': { apiKey: 1 } } },
    { providers: { 'opencode-go': { apiKey: '' } } },
    { providers: { 'opencode-go': { apiKey: '  ' } } },
    { providers: { 'opencode-go': { extra: 'field' } } },
  ];

  for (const providers of cases) {
    const parsed = providers as { providers: unknown };
    assert.throws(
      () =>
        parseAtlasConfig(
          { version: 1, provider: 'opencode-go', model: 'model-test', providers: parsed.providers },
          'inline',
        ),
      AtlasConfigError,
    );
  }
});

test('error messages for invalid credentials never expose key values', async () => {
  const { path, cleanup } = await tempConfig(
    JSON.stringify({
      version: 1,
      provider: 'opencode-go',
      model: 'model-test',
      providers: { 'opencode-go': { apiKey: 42 } },
    }),
  );
  try {
    await assert.rejects(loadAtlasConfig(path), (error: unknown) => {
      assert.ok(error instanceof AtlasConfigError);
      assert.match(error.message, /\.apiKey" must be a non-empty string/);
      assert.doesNotMatch(error.message, /42/);
      return true;
    });
  } finally {
    await cleanup();
  }
});

test('normalizes provider-qualified model IDs to the bare model', () => {
  assert.equal(normalizeAtlasModel('gpt-5.6-luna'), 'gpt-5.6-luna');
  assert.equal(normalizeAtlasModel('opencode-go/gpt-5.6-luna'), 'gpt-5.6-luna');
});

test('loads a provider-qualified model from the config file', async () => {
  const { path, cleanup } = await tempConfig(
    JSON.stringify({
      version: 1,
      provider: 'opencode-go',
      model: 'opencode-go/model-test',
    }),
  );
  try {
    const loaded = await loadAtlasConfig(path);
    assert.deepEqual(atlasRuntimeSessionData(loaded.config), {
      provider: 'opencode-go',
      model: 'model-test',
    });
  } finally {
    await cleanup();
  }
});

test('keeps the public JSON schema aligned with the canonical loader', async () => {
  const schema = JSON.parse(
    await readFile(new URL('../../protocol/config.schema.json', import.meta.url), 'utf8'),
  ) as Record<string, unknown>;

  assert.equal(schema.$schema, 'https://json-schema.org/draft/2020-12/schema');
  assert.equal(schema.type, 'object');
  assert.deepEqual(schema.required, ['version', 'provider', 'model']);
  assert.equal(schema.additionalProperties, false);
  const properties = schema.properties as Record<string, Record<string, unknown>>;
  assert.equal(properties.version?.const, 1);
  assert.equal(properties.provider?.minLength, 1);
  assert.equal(properties.model?.minLength, 1);

  const providers = properties.providers as Record<string, unknown>;
  assert.equal(providers.type, 'object');
  assert.equal(providers.additionalProperties, false);
  const opencodeGo = providers.properties as Record<string, unknown>;
  assert.ok(Object.hasOwn(opencodeGo, 'opencode-go'));
});
