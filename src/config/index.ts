// Contrato publico da configuração global consumida pelo Runtime.
export {
  ATLAS_CONFIG_VERSION,
  ATLAS_DEFAULT_MODEL,
  ATLAS_DEFAULT_PROVIDER,
  AtlasConfigError,
  DEFAULT_ATLAS_CONFIG,
  DEFAULT_ATLAS_WEB_CONFIG,
  atlasApiKey,
  atlasRuntimeSessionData,
  loadAtlasConfig,
  normalizeAtlasModel,
  parseAtlasConfig,
  resolveAtlasConfigPath,
} from './loader.js';
export type {
  AtlasConfig,
  AtlasWebConfig,
  ProviderCredentials,
  WebProviderConfig,
} from './loader.js';
