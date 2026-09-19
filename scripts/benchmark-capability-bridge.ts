import { performance } from 'node:perf_hooks';

import { NativeCapabilityRuntime } from '../src/capability-runtime.js';

type Sample = {
  name: string;
  iterations: number;
  totalMs: number;
  avgMs: number;
  p50Ms: number;
  minMs: number;
  maxMs: number;
};

function summarize(name: string, samples: number[]): Sample {
  const sorted = [...samples].sort((left, right) => left - right);
  const total = samples.reduce((sum, value) => sum + value, 0);
  const middle = Math.floor(sorted.length / 2);
  const p50 =
    sorted.length % 2 === 0 ? (sorted[middle - 1]! + sorted[middle]!) / 2 : sorted[middle]!;
  return {
    name,
    iterations: samples.length,
    totalMs: Number(total.toFixed(2)),
    avgMs: Number((total / samples.length).toFixed(2)),
    p50Ms: Number(p50.toFixed(2)),
    minMs: Number(sorted[0]!.toFixed(2)),
    maxMs: Number(sorted.at(-1)!.toFixed(2)),
  };
}

async function measure(
  name: string,
  iterations: number,
  action: () => Promise<unknown>,
): Promise<Sample> {
  await action();
  const samples: number[] = [];
  for (let index = 0; index < iterations; index += 1) {
    const start = performance.now();
    await action();
    samples.push(performance.now() - start);
  }
  return summarize(name, samples);
}

const runtime = new NativeCapabilityRuntime();

try {
  const results: Sample[] = [];
  results.push(
    await measure('discover', 20, () => runtime.discover({ query: 'executar programa' })),
  );
  results.push(await measure('get_definition', 20, () => runtime.getDefinition('process.exec')));
  results.push(
    await measure('execute', 20, () =>
      runtime.execute('process.exec', 'local', { program: '/bin/true', args: [] }),
    ),
  );
  results.push(
    await measure('sequence_discover_describe_execute', 10, async () => {
      await runtime.discover({ query: 'executar programa' });
      await runtime.getDefinition('process.exec');
      await runtime.execute('process.exec', 'local', { program: '/bin/true', args: [] });
    }),
  );

  console.log(JSON.stringify(results, null, 2));
} finally {
  if ('close' in runtime && typeof runtime.close === 'function') {
    await runtime.close();
  }
}
