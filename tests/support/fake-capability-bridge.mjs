#!/usr/bin/env node
import { appendFileSync, existsSync, writeFileSync } from 'node:fs';
import { createInterface } from 'node:readline';

const logPath = process.env.ATLAS_FAKE_BRIDGE_LOG;
if (logPath) {
  appendFileSync(logPath, `${process.pid}\n`);
}

const crashMarker = process.env.ATLAS_FAKE_BRIDGE_CRASH_ONCE;
if (crashMarker && !existsSync(crashMarker)) {
  writeFileSync(crashMarker, 'crashed\n');
  process.exit(1);
}

const discoverDelay = Number(process.env.ATLAS_FAKE_DISCOVER_DELAY_MS ?? '0');
const definition = {
  id: 'fake.tool',
  type: 'tool',
  summary: 'fake tool',
  description: 'fake tool used by the persistent bridge tests',
  schema: { type: 'object', properties: {} },
};
const skill = {
  id: 'fake.skill',
  type: 'skill',
  summary: 'fake skill',
  instructions: 'Use fake.tool, then report the result.',
  source: '/tmp/fake.skill/SKILL.md',
  files: [],
};

function write(payload) {
  process.stdout.write(`${JSON.stringify(payload)}\n`);
}

const input = createInterface({ input: process.stdin });
input.on('line', (line) => {
  const trimmed = line.trim();
  if (!trimmed) {
    return;
  }

  let request;
  try {
    request = JSON.parse(trimmed);
  } catch {
    write({ error: 'invalid json' });
    return;
  }

  const requestId = request.request_id;
  const respond = (payload) => write({ request_id: requestId, ...payload });

  if (request.operation === 'discover') {
    const reply = () =>
      respond({
        results: [
          { id: 'fake.tool', type: 'tool', summary: 'fake tool' },
          { id: 'fake.skill', type: 'skill', summary: 'fake skill' },
        ],
      });
    if (discoverDelay > 0) {
      setTimeout(reply, discoverDelay);
    } else {
      reply();
    }
    return;
  }

  if (request.operation === 'list_tools') {
    respond({ tools: [{ id: 'fake.tool', type: 'tool', summary: 'fake tool', group: 'fake' }] });
    return;
  }

  if (request.operation === 'get_definition') {
    respond({ definition: request.id === 'missing.tool' ? null : definition });
    return;
  }

  if (request.operation === 'get_skill') {
    respond({ skill: request.id === skill.id ? skill : null });
    return;
  }

  if (request.operation === 'execute') {
    if (request.stream) {
      write({
        request_id: requestId,
        event: 'execution.output.delta',
        channel: 'stdout',
        delta: 'first',
      });
      write({
        request_id: requestId,
        event: 'execution.output.delta',
        channel: 'stderr',
        delta: 'second',
      });
    }
    respond({
      target: request.target ?? 'local',
      status: 'success',
      error: '',
      output: { stdout: 'complete', stderr: '', exit_code: 0, duration_ms: 1 },
    });
    return;
  }

  respond({ error: `unknown operation '${request.operation}'` });
});

input.on('close', () => process.exit(0));
