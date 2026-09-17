// Este e o ponto de entrada publico: consumidores importam o Agent e seu runtime daqui.
export { Atlas, createAtlasRunner, getAtlasRunner, runAtlas } from './atlas.js';
export type { AtlasRunEvent, AtlasRunOptions } from './atlas.js';

// O runtime de capabilities permanece independente do Agent SDK e pode ser substituido em testes.
export { createCapabilityRuntime, NativeCapabilityRuntime } from './capability-runtime.js';
export type {
  CapabilityDefinition,
  CapabilityDiscoveryRequest,
  CapabilityDiscoveryResult,
  CapabilityExecutionResult,
  CapabilityRuntime,
  CapabilityToolListRequest,
  CapabilityToolListResult,
  NativeCapabilityRuntimeOptions,
} from './capability-runtime.js';

// O provider e a sessao ficam publicos para configuracao e integracao com o OpenCode Go.
export {
  ATLAS_USER_AGENT,
  OpenCodeGoProvider,
  OpenCodeGoSession,
  OPENCODE_GO_BASE_URL,
  OPENCODE_GO_MODEL,
  OPENCODE_GO_MODEL_ID,
  OPENCODE_GO_MODELS,
  OPENCODE_GO_PROVIDER,
  OPENCODE_GO_RESPONSES_PATH,
  OPENCODE_GO_RESPONSES_URL,
  withOpenCodeGoSession,
} from './opencode-go.js';

// Tipos publicos permitem configurar endpoints e modelos sem depender de tipos internos do SDK.
export type {
  OpenCodeGoEndpoint,
  OpenCodeGoModelDefinition,
  OpenCodeGoProviderOptions,
} from './opencode-go.js';

export type { RuntimeContextUsage, RuntimeTurnCompletedData } from './runtime/protocol.js';
