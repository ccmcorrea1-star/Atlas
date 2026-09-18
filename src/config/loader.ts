// Carrega a configuração global do Atlas: caminho, defaults e validação.
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const ATLAS_CONFIG_VERSION = 1;
export const ATLAS_DEFAULT_PROVIDER = 'opencode-go';
export const ATLAS_DEFAULT_MODEL = 'gpt-5.6-luna';

// O arquivo global define provider, model e credenciais por provider.
export type ProviderCredentials = {
  apiKey?: string;
};

export type AtlasConfig = {
  version: typeof ATLAS_CONFIG_VERSION;
  provider: string;
  model: string;
  providers?: { 'opencode-go'?: ProviderCredentials };
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

// Localiza a raiz do projeto subindo até o package.json, a partir deste modulo.
// Funciona tanto do código-fonte quanto do build transpilado para dist/.
function atlasProjectDirectory(): string {
  let directory = dirname(fileURLToPath(import.meta.url));
  while (!existsSync(join(directory, 'package.json'))) {
    const parent = dirname(directory);
    if (parent === directory) {
      return directory;
    }
    directory = parent;
  }
  return directory;
}

// ORDEM fixa: ATLAS_CONFIG, XDG_CONFIG_HOME/atlas/config.json,
// <projeto>/config/atlas/config.json. A config do projeto é global desta máquina.
export function resolveAtlasConfigPath(env: NodeJS.ProcessEnv = process.env): string {
  const override = env.ATLAS_CONFIG?.trim();
  if (override) {
    return override;
  }

  const xdgConfigHome = env.XDG_CONFIG_HOME?.trim();
  if (xdgConfigHome) {
    return join(xdgConfigHome, 'atlas', 'config.json');
  }

  // O arquivo vive no projeto, junto de todos os clientes deste checkout.
  return join(atlasProjectDirectory(), 'config', 'atlas', 'config.json');
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

// Campos obrigatórios; credenciais ficam apenas em "providers".
export function parseAtlasConfig(value: unknown, source: string): AtlasConfig {
  if (!isRecord(value)) {
    throw new AtlasConfigError(`Atlas config at ${source} must be a JSON object.`);
  }

  const knownFields = new Set(['version', 'provider', 'model', 'providers']);
  const unknownFields = Object.keys(value)
    .filter((field) => !knownFields.has(field))
    .sort();
  if (unknownFields.length > 0) {
    throw new AtlasConfigError(
      `Atlas config at ${source} has unsupported field(s): ${unknownFields.join(', ')}. ` +
        `API keys belong under "providers".`,
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

  let providers: AtlasConfig['providers'];
  if (Object.hasOwn(value, 'providers')) {
    if (!isRecord(value.providers)) {
      throw new AtlasConfigError(`Atlas config at ${source}: "providers" must be an object.`);
    }
    const unknownProviders = Object.keys(value.providers).filter(
      (provider) => provider !== ATLAS_DEFAULT_PROVIDER,
    );
    if (unknownProviders.length > 0) {
      throw new AtlasConfigError(
        `Atlas config at ${source}: unsupported provider(s) in "providers": ${unknownProviders.join(', ')}. ` +
          `Only "${ATLAS_DEFAULT_PROVIDER}" is currently supported.`,
      );
    }
    if (Object.hasOwn(value.providers, ATLAS_DEFAULT_PROVIDER)) {
      const credentials = value.providers[ATLAS_DEFAULT_PROVIDER];
      if (!isRecord(credentials)) {
        throw new AtlasConfigError(
          `Atlas config at ${source}: "providers.${ATLAS_DEFAULT_PROVIDER}" must be an object.`,
        );
      }
      const unknownCredentialFields = Object.keys(credentials).filter(
        (field) => field !== 'apiKey',
      );
      if (unknownCredentialFields.length > 0) {
        throw new AtlasConfigError(
          `Atlas config at ${source}: unsupported field(s) in "providers.${ATLAS_DEFAULT_PROVIDER}": ` +
            `${unknownCredentialFields.join(', ')}.`,
        );
      }
      if (Object.hasOwn(credentials, 'apiKey')) {
        if (typeof credentials.apiKey !== 'string' || credentials.apiKey.trim().length === 0) {
          throw new AtlasConfigError(
            `Atlas config at ${source}: "providers.${ATLAS_DEFAULT_PROVIDER}.apiKey" must be a non-empty string.`,
          );
        }
        providers = {
          [ATLAS_DEFAULT_PROVIDER]: { apiKey: credentials.apiKey },
        };
      }
    }
  }

  return {
    version: ATLAS_CONFIG_VERSION,
    provider: requiredString(value.provider, 'provider', source),
    model: requiredString(value.model, 'model', source),
    ...(providers ? { providers } : {}),
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

// Resolve a API key: env tem prioridade, config serve como alternativa e a ausencia falha.
export function atlasApiKey(
  config: AtlasConfig,
  env: NodeJS.ProcessEnv = process.env,
): { apiKey: string; origin: 'environment' | 'config' } {
  const prefix = `${ATLAS_DEFAULT_PROVIDER.toUpperCase().replaceAll('-', '_')}_API_KEY`;

  const fromEnvironment = env[prefix]?.trim();
  if (fromEnvironment) {
    return { apiKey: fromEnvironment, origin: 'environment' };
  }

  const apiKey = config.providers?.[ATLAS_DEFAULT_PROVIDER]?.apiKey?.trim();
  if (apiKey) {
    return { apiKey, origin: 'config' };
  }

  throw new AtlasConfigError(
    `Atlas config: no API key for provider "${ATLAS_DEFAULT_PROVIDER}". ` +
      `Set ${prefix} in the environment or "providers.${ATLAS_DEFAULT_PROVIDER}.apiKey" in the config file.`,
  );
}
