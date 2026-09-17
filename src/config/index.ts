// Contrato publico da configuração global consumida pelo Runtime.
export {
  ATLAS_CONFIG_VERSION,
  ATLAS_DEFAULT_MODEL,
  ATLAS_DEFAULT_PROVIDER,
  AtlasConfigError,
  DEFAULT_ATLAS_CONFIG,
  atlasRuntimeSessionData,
  loadAtlasConfig,
  normalizeAtlasModel,
  parseAtlasConfig,
  resolveAtlasConfigPath,
} from './loader.js';
export type { AtlasConfig } from './loader.js';
