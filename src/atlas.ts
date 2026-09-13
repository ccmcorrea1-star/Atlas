import { Agent, Runner } from '@openai/agents';

import {
  OpenCodeGoProvider,
  type OpenCodeGoProviderOptions,
  OPENCODE_GO_MODEL,
} from './opencode-go.js';

export const Atlas = new Agent({
  name: 'Atlas',
  instructions:
    'You are Atlas, a pragmatic coding agent. Give clear, concise answers and do not claim work you did not perform.',
  model: OPENCODE_GO_MODEL,
});

export function createAtlasRunner(options: OpenCodeGoProviderOptions = {}): Runner {
  return new Runner({
    modelProvider: new OpenCodeGoProvider(options),
    tracingDisabled: true,
  });
}

export async function runAtlas(input: string, options: OpenCodeGoProviderOptions = {}) {
  return createAtlasRunner(options).run(Atlas, input);
}
