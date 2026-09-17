// Carrega a configuração global do Atlas: caminho, defaults e validação.
import { readFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join } from 'node:path';

export const ATLAS_CONFIG_VERSION = 1;
export const ATLAS_DEFAULT_PROVIDER = 'opencode-go';
export const ATLAS_DEFAULT_MODEL = 'gpt-5.6-luna';

// O arquivo global define provider/model; credenciais continuam no ambiente.
export type AtlasConfig = {
  version: typeof ATLAS_CONFIG_VERSION;
  provider: string;
  model: string;
};

export const DEFAULT_ATLAS_CONFIG: AtlasConfig = {
  version: ATLAS_CONFIG_VERSION,
  provider: ATLAS_DEFAULT_PROVIDER,
  model: ATLAS_DEFAULT_MODEL,
};

export class AtlasConfigError extends Error {
  public constructor(message: string) {
    super(message);
    this.name = 'AtlasConfigError';
  }
}

// ORDEM fixa: ATLAS_CONFIG, XDG_CONFIG_HOME/atlas/config.json, ~/.config/atlas/config.json.
export function resolveAtlasConfigPath(
  env: NodeJS.ProcessEnv = process.env,
  userHome: string = homedir(),
): string {
  const override = env.ATLAS_CONFIG?.trim();
  if (override) {
    return override;
  }

  const xdgConfigHome = env.XDG_CONFIG_HOME?.trim();
  if (xdgConfigHome) {
    return join(xdgConfigHome, 'atlas', 'config.json');
  }

  return join(userHome, '.config', 'atlas', 'config.json');
}

// Aceita o modelo puro ou na forma "opencode-go/<modelo>".
export function normalizeAtlasModel(model: string): string {
  const prefix = `${ATLAS_DEFAULT_PROVIDER}/`;
  return model.startsWith(prefix) ? model.slice(prefix.length) : model;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function requiredString(value: unknown, field: string, source: string): string {
  if (typeof value !== 'string' || value.trim().length === 0) {
    throw new AtlasConfigError(`Atlas config at ${source}: "${field}" must be a non-empty string.`);
  }
  return value;
}

// Campos obrigatórios; campos desconhecidos são recusados para impedir credenciais no arquivo.
export function parseAtlasConfig(value: unknown, source: string): AtlasConfig {
  if (!isRecord(value)) {
    throw new AtlasConfigError(`Atlas config at ${source} must be a JSON object.`);
  }

  const knownFields = new Set(['version', 'provider', 'model']);
  const unknownFields = Object.keys(value)
    .filter((field) => !knownFields.has(field))
    .sort();
  if (unknownFields.length > 0) {
    throw new AtlasConfigError(
      `Atlas config at ${source} has unsupported field(s): ${unknownFields.join(', ')}. ` +
        'API keys must stay in the environment (OPENCODE_GO_API_KEY).',
    );
  }

  const version = value.version;
  if (typeof version !== 'number' || !Number.isInteger(version)) {
    throw new AtlasConfigError(
      `Atlas config at ${source}: "version" must be the integer 1, received ${JSON.stringify(version)}.`,
    );
  }
  if (version !== ATLAS_CONFIG_VERSION) {
    throw new AtlasConfigError(
      `Atlas config at ${source}: unsupported version ${JSON.stringify(version)}. Expected ${ATLAS_CONFIG_VERSION}.`,
    );
  }

  return {
    version: ATLAS_CONFIG_VERSION,
    provider: requiredString(value.provider, 'provider', source),
    model: requiredString(value.model, 'model', source),
  };
}

// Arquivo ausente volta para os defaults; o arquivo não é criado automaticamente.
export async function loadAtlasConfig(
  path: string = resolveAtlasConfigPath(),
): Promise<{ config: AtlasConfig; path: string; source: 'defaults' | 'file' }> {
  let contents: string;
  try {
    contents = await readFile(path, 'utf8');
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
      return { config: DEFAULT_ATLAS_CONFIG, path, source: 'defaults' };
    }
    throw new AtlasConfigError(
      `Atlas config at ${path} could not be read: ${error instanceof Error ? error.message : String(error)}`,
    );
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(contents) as unknown;
  } catch (error) {
    throw new AtlasConfigError(
      `Atlas config at ${path} is not valid JSON: ${error instanceof Error ? error.message : String(error)}`,
    );
  }

  return { path, source: 'file', config: parseAtlasConfig(parsed, path) };
}

// Providers extras ficam para uma fase futura; o erro precisa ser explícito.
export function atlasRuntimeSessionData(config: AtlasConfig): {
  provider: string;
  model: string;
} {
  if (config.provider !== ATLAS_DEFAULT_PROVIDER) {
    throw new AtlasConfigError(
      `Atlas provider "${config.provider}" is not supported yet. ` +
        `Only "${ATLAS_DEFAULT_PROVIDER}" is currently supported.`,
    );
  }
  if (typeof config.model !== 'string' || config.model.trim().length === 0) {
    throw new AtlasConfigError('Atlas config: "model" must be a non-empty string.');
  }

  return { provider: config.provider, model: normalizeAtlasModel(config.model) };
}
