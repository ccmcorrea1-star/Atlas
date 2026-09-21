// Este e o ponto de entrada publico: consumidores importam o Agent e seu runtime daqui.
export {
  Atlas,
  createAtlasRunner,
  getAtlasRunner,
  resetAtlasConversation,
  runAtlas,
} from './agent/atlas.js';
export type { AtlasApprovalDecision, AtlasRunEvent, AtlasRunOptions } from './agent/atlas.js';

// O runtime de capabilities permanece independente do Agent SDK e pode ser substituido em testes.
export { createCapabilityRuntime, NativeCapabilityRuntime } from './capabilities/runtime-client.js';
export type {
  CapabilityType,
  CapabilityDiscoveryRequest,
  CapabilityDiscoveryResult,
  CapabilityExecutionResult,
  CapabilityRuntime,
  CapabilityToolListRequest,
  CapabilityToolListResult,
  NativeCapabilityRuntimeOptions,
  ToolDefinition,
} from './capabilities/runtime-client.js';
export type { SkillDefinition, SkillDiscovery, SkillFile } from './skills/types.js';

// Os hooks genericos permitem auditar ou bloquear execucoes sem alterar as capabilities.
export {
  HookableCapabilityRuntime,
  RetryGuard,
  stableSerialize,
} from './capabilities/execution-hooks.js';
export type {
  AfterExecuteHook,
  BeforeExecuteHook,
  CapabilityExecutionHookContext,
  CapabilityExecutionHooks,
  RetryGuardStatus,
} from './capabilities/execution-hooks.js';

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
} from './providers/opencode-go.js';

// Tipos publicos permitem configurar endpoints e modelos sem depender de tipos internos do SDK.
export type {
  OpenCodeGoEndpoint,
  OpenCodeGoModelDefinition,
  OpenCodeGoProviderOptions,
} from './providers/opencode-go.js';

export type {
  RuntimeAttachment,
  RuntimeAttachmentType,
  RuntimeApprovalRequestedData,
  RuntimeApprovalResolvedData,
  RuntimeOperationData,
  RuntimeRestartingData,
  RuntimeApprovalResponse,
  RuntimeCommandCompletedData,
  RuntimeCommandRequest,
  RuntimeCommandDefinition,
  RuntimeCommandName,
  RuntimeContextUsage,
  RuntimeSession,
  RuntimeSessionStatus,
  RuntimeTurnCancelledData,
  RuntimeTurnCompletedData,
  RuntimeTurnRequest,
} from './runtime/protocol.js';

export { HostSupervisor, OperationStore } from './host/index.js';
export type { HandoffRequest, HostOperation, HostOperationState } from './host/index.js';

export {
  RUNTIME_COMMANDS,
  RUNTIME_PROTOCOL,
  RUNTIME_PROTOCOL_VERSION,
  runtimeLifecycleEvent,
} from './runtime/protocol.js';
