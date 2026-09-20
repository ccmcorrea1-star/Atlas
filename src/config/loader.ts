// Carrega a configuração global do Atlas: caminho, defaults e validação.
import { existsSync } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const ATLAS_CONFIG_VERSION = 1;
export const ATLAS_DEFAULT_PROVIDER = 'opencode-go';
export const ATLAS_DEFAULT_MODEL = 'gpt-5.6-luna';

export type ProviderCredentials = {
  apiKey?: string;
};

export type WebProviderConfig = {
  endpoint?: string;
  apiKey?: string;
};

export type AtlasWebConfig = {
  search: {
    provider: string;
    endpoint: string;
    timeoutMs: number;
    fallbackProviders: string[];
  };
  fetch: {
    extractor: 'native' | 'trafilatura';
  };
  browser: {
    provider: 'camoufox';
    executablePath?: string;
    userDataDir?: string;
    socketPath?: string;
    headless: boolean;
  };
  crawl: {
    provider: 'crawl4ai';
    endpoint?: string;
    timeoutMs: number;
  };
  providers?: Record<string, WebProviderConfig>;
};

export type AtlasConfig = {
  version: typeof ATLAS_CONFIG_VERSION;
  provider: string;
  model: string;
  providers?: { 'opencode-go'?: ProviderCredentials };
  web?: AtlasWebConfig;
};

export const DEFAULT_ATLAS_WEB_CONFIG: AtlasWebConfig = {
  search: {
    provider: 'searxng',
    endpoint: 'http://127.0.0.1:8080',
    timeoutMs: 15000,
    fallbackProviders: [],
  },
  fetch: { extractor: 'native' },
  browser: { provider: 'camoufox', headless: true },
  crawl: { provider: 'crawl4ai', timeoutMs: 30000 },
};

export const DEFAULT_ATLAS_CONFIG: AtlasConfig = {
  version: ATLAS_CONFIG_VERSION,
  provider: ATLAS_DEFAULT_PROVIDER,
  model: ATLAS_DEFAULT_MODEL,
  web: DEFAULT_ATLAS_WEB_CONFIG,
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
  return value.trim();
}

function optionalString(value: unknown, field: string, source: string): string | undefined {
  return value === undefined ? undefined : requiredString(value, field, source);
}

function positiveInteger(value: unknown, field: string, source: string, fallback: number): number {
  if (value === undefined) {
    return fallback;
  }
  if (typeof value !== 'number' || !Number.isInteger(value) || value < 1) {
    throw new AtlasConfigError(`Atlas config at ${source}: "${field}" must be a positive integer.`);
  }
  return value;
}

function fieldsExcept(value: Record<string, unknown>, allowed: readonly string[]): string[] {
  const accepted = new Set(allowed);
  return Object.keys(value)
    .filter((field) => !accepted.has(field))
    .sort();
}

function parseWebProviderConfigs(
  value: unknown,
  source: string,
): Record<string, WebProviderConfig> | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (!isRecord(value)) {
    throw new AtlasConfigError(`Atlas config at ${source}: "web.providers" must be an object.`);
  }

  const providers: Record<string, WebProviderConfig> = {};
  for (const [provider, rawConfig] of Object.entries(value)) {
    if (!isRecord(rawConfig)) {
      throw new AtlasConfigError(
        `Atlas config at ${source}: "web.providers.${provider}" must be an object.`,
      );
    }
    const unknown = fieldsExcept(rawConfig, ['endpoint', 'apiKey']);
    if (unknown.length > 0) {
      throw new AtlasConfigError(
        `Atlas config at ${source}: unsupported field(s) in "web.providers.${provider}": ${unknown.join(', ')}.`,
      );
    }
    providers[provider] = {
      ...(optionalString(rawConfig.endpoint, `web.providers.${provider}.endpoint`, source)
        ? { endpoint: rawConfig.endpoint as string }
        : {}),
      ...(optionalString(rawConfig.apiKey, `web.providers.${provider}.apiKey`, source)
        ? { apiKey: rawConfig.apiKey as string }
        : {}),
    };
  }
  return providers;
}

function parseWebConfig(value: unknown, source: string): AtlasWebConfig {
  if (value === undefined) {
    return DEFAULT_ATLAS_WEB_CONFIG;
  }
  if (!isRecord(value)) {
    throw new AtlasConfigError(`Atlas config at ${source}: "web" must be an object.`);
  }
  const unknown = fieldsExcept(value, ['search', 'fetch', 'browser', 'crawl', 'providers']);
  if (unknown.length > 0) {
    throw new AtlasConfigError(
      `Atlas config at ${source}: unsupported field(s) in "web": ${unknown.join(', ')}.`,
    );
  }

  const rawSearch = value.search ?? {};
  const rawFetch = value.fetch ?? {};
  const rawBrowser = value.browser ?? {};
  const rawCrawl = value.crawl ?? {};
  if (!isRecord(rawSearch) || !isRecord(rawFetch) || !isRecord(rawBrowser) || !isRecord(rawCrawl)) {
    throw new AtlasConfigError(
      `Atlas config at ${source}: web capability settings must be objects.`,
    );
  }

  const searchUnknown = fieldsExcept(rawSearch, [
    'provider',
    'endpoint',
    'timeoutMs',
    'fallbackProviders',
  ]);
  if (searchUnknown.length > 0) {
    throw new AtlasConfigError(
      `Atlas config at ${source}: unsupported field(s) in "web.search": ${searchUnknown.join(', ')}.`,
    );
  }
  const fallbacks = rawSearch.fallbackProviders ?? [];
  if (
    !Array.isArray(fallbacks) ||
    fallbacks.some((provider) => typeof provider !== 'string' || provider.trim().length === 0)
  ) {
    throw new AtlasConfigError(
      `Atlas config at ${source}: "web.search.fallbackProviders" must be an array of non-empty strings.`,
    );
  }

  const fetchUnknown = fieldsExcept(rawFetch, ['extractor']);
  if (fetchUnknown.length > 0) {
    throw new AtlasConfigError(
      `Atlas config at ${source}: unsupported field(s) in "web.fetch": ${fetchUnknown.join(', ')}.`,
    );
  }
  const extractor = rawFetch.extractor ?? DEFAULT_ATLAS_WEB_CONFIG.fetch.extractor;
  if (extractor !== 'native' && extractor !== 'trafilatura') {
    throw new AtlasConfigError(
      `Atlas config at ${source}: "web.fetch.extractor" must be "native" or "trafilatura".`,
    );
  }

  const browserUnknown = fieldsExcept(rawBrowser, [
    'provider',
    'executablePath',
    'userDataDir',
    'socketPath',
    'headless',
  ]);
  if (browserUnknown.length > 0) {
    throw new AtlasConfigError(
      `Atlas config at ${source}: unsupported field(s) in "web.browser": ${browserUnknown.join(', ')}.`,
    );
  }
  const browserProvider = rawBrowser.provider ?? DEFAULT_ATLAS_WEB_CONFIG.browser.provider;
  if (browserProvider !== 'camoufox') {
    throw new AtlasConfigError(
      `Atlas config at ${source}: unsupported web.browser provider "${String(browserProvider)}".`,
    );
  }
  if (rawBrowser.headless !== undefined && typeof rawBrowser.headless !== 'boolean') {
    throw new AtlasConfigError(
      `Atlas config at ${source}: "web.browser.headless" must be boolean.`,
    );
  }

  const crawlUnknown = fieldsExcept(rawCrawl, ['provider', 'endpoint', 'timeoutMs']);
  if (crawlUnknown.length > 0) {
    throw new AtlasConfigError(
      `Atlas config at ${source}: unsupported field(s) in "web.crawl": ${crawlUnknown.join(', ')}.`,
    );
  }
  const crawlProvider = rawCrawl.provider ?? DEFAULT_ATLAS_WEB_CONFIG.crawl.provider;
  if (crawlProvider !== 'crawl4ai') {
    throw new AtlasConfigError(
      `Atlas config at ${source}: unsupported web.crawl provider "${String(crawlProvider)}".`,
    );
  }

  const providers = parseWebProviderConfigs(value.providers, source);
  return {
    search: {
      provider:
        optionalString(rawSearch.provider, 'web.search.provider', source) ??
        DEFAULT_ATLAS_WEB_CONFIG.search.provider,
      endpoint:
        optionalString(rawSearch.endpoint, 'web.search.endpoint', source) ??
        DEFAULT_ATLAS_WEB_CONFIG.search.endpoint,
      timeoutMs: positiveInteger(
        rawSearch.timeoutMs,
        'web.search.timeoutMs',
        source,
        DEFAULT_ATLAS_WEB_CONFIG.search.timeoutMs,
      ),
      fallbackProviders: fallbacks.map((provider) => (provider as string).trim()),
    },
    fetch: { extractor },
    browser: {
      provider: 'camoufox',
      ...(optionalString(rawBrowser.executablePath, 'web.browser.executablePath', source)
        ? { executablePath: rawBrowser.executablePath as string }
        : {}),
      ...(optionalString(rawBrowser.userDataDir, 'web.browser.userDataDir', source)
        ? { userDataDir: rawBrowser.userDataDir as string }
        : {}),
      ...(optionalString(rawBrowser.socketPath, 'web.browser.socketPath', source)
        ? { socketPath: rawBrowser.socketPath as string }
        : {}),
      headless: rawBrowser.headless ?? DEFAULT_ATLAS_WEB_CONFIG.browser.headless,
    },
    crawl: {
      provider: 'crawl4ai',
      ...(optionalString(rawCrawl.endpoint, 'web.crawl.endpoint', source)
        ? { endpoint: rawCrawl.endpoint as string }
        : {}),
      timeoutMs: positiveInteger(
        rawCrawl.timeoutMs,
        'web.crawl.timeoutMs',
        source,
        DEFAULT_ATLAS_WEB_CONFIG.crawl.timeoutMs,
      ),
    },
    ...(providers === undefined ? {} : { providers }),
  };
}

// Campos obrigatórios; credenciais ficam apenas em "providers".
export function parseAtlasConfig(value: unknown, source: string): AtlasConfig {
  if (!isRecord(value)) {
    throw new AtlasConfigError(`Atlas config at ${source} must be a JSON object.`);
  }

  const knownFields = new Set(['version', 'provider', 'model', 'providers', 'web']);
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
    web: parseWebConfig(value.web, source),
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

export function atlasRuntimeSessionData(config: AtlasConfig): { provider: string; model: string } {
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
