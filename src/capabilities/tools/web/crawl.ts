#!/usr/bin/env node
// Tool web.crawl: adapter opcional para um serviço local Crawl4AI.
import { readFileSync } from 'node:fs';

export type CrawlRequest = {
  url: string;
  maxPages?: number;
  maxDepth?: number;
};

export type CrawlConfig = {
  provider?: 'crawl4ai';
  endpoint?: string;
  timeoutMs?: number;
  apiKey?: string;
};

type WebEnvironmentConfig = {
  crawl?: CrawlConfig;
  providers?: Record<string, { apiKey?: string }>;
};

export function parseRequest(text: string): { target: string; request: CrawlRequest } {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new Error('invalid JSON request');
  }
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('request must be a JSON object');
  }
  const record = value as Record<string, unknown>;
  if (typeof record.target !== 'string') {
    throw new Error("field 'target' must be a string");
  }
  if (typeof record.url !== 'string' || !/^https?:\/\//i.test(record.url)) {
    throw new Error("field 'url' must be an HTTP or HTTPS URL");
  }
  for (const field of ['maxPages', 'maxDepth']) {
    const valueForField = record[field];
    if (
      valueForField !== undefined &&
      (typeof valueForField !== 'number' || !Number.isInteger(valueForField) || valueForField < 1)
    ) {
      throw new Error(`field '${field}' must be a positive integer`);
    }
  }
  return {
    target: record.target,
    request: {
      url: record.url,
      ...(record.maxPages === undefined ? {} : { maxPages: record.maxPages as number }),
      ...(record.maxDepth === undefined ? {} : { maxDepth: record.maxDepth as number }),
    },
  };
}

export function crawlConfigFromEnvironment(env: NodeJS.ProcessEnv = process.env): CrawlConfig {
  try {
    if (env.ATLAS_WEB_CONFIG_JSON) {
      const config = JSON.parse(env.ATLAS_WEB_CONFIG_JSON) as WebEnvironmentConfig;
      return withProviderApiKey(config.crawl ?? {}, config.providers?.crawl4ai?.apiKey);
    }
    if (env.ATLAS_CONFIG) {
      const config = JSON.parse(readFileSync(env.ATLAS_CONFIG, 'utf8')) as {
        web?: WebEnvironmentConfig;
      };
      return withProviderApiKey(config.web?.crawl ?? {}, config.web?.providers?.crawl4ai?.apiKey);
    }
  } catch {
    return {};
  }
  return {};
}

function withProviderApiKey(config: CrawlConfig, apiKey: string | undefined): CrawlConfig {
  return apiKey === undefined ? config : { ...config, apiKey };
}

export async function runCrawl(
  request: CrawlRequest,
  config: CrawlConfig,
): Promise<Record<string, unknown>> {
  if (config.provider !== undefined && config.provider !== 'crawl4ai') {
    return { status: 'unavailable', error: `crawl provider '${config.provider}' is not installed` };
  }
  if (config.endpoint === undefined || config.endpoint.trim() === '') {
    return { status: 'unavailable', error: 'Crawl4AI endpoint is not configured' };
  }
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), config.timeoutMs ?? 30000);
  try {
    const crawlerConfig = {
      ...(request.maxPages === undefined ? {} : { max_pages: request.maxPages }),
      ...(request.maxDepth === undefined ? {} : { max_depth: request.maxDepth }),
    };
    const response = await fetch(config.endpoint, {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        ...(config.apiKey === undefined ? {} : { authorization: `Bearer ${config.apiKey}` }),
      },
      body: JSON.stringify({
        urls: [request.url],
        ...(Object.keys(crawlerConfig).length === 0 ? {} : { crawler_config: crawlerConfig }),
      }),
      signal: controller.signal,
    });
    if (!response.ok) {
      return { status: 'failed', error: `Crawl4AI returned HTTP ${response.status}` };
    }
    const payload = (await response.json()) as unknown;
    return { status: 'success', pages: payload };
  } catch (error) {
    const message =
      error instanceof Error && error.name === 'AbortError'
        ? 'Crawl4AI request timed out'
        : error instanceof Error
          ? error.message
          : String(error);
    return { status: 'failed', error: message };
  } finally {
    clearTimeout(timeout);
  }
}

async function main(): Promise<void> {
  let input = '';
  process.stdin.setEncoding('utf8');
  for await (const chunk of process.stdin) {
    input += chunk;
  }
  try {
    const parsed = parseRequest(input);
    const result = await runCrawl(parsed.request, crawlConfigFromEnvironment());
    process.stdout.write(`${JSON.stringify({ target: parsed.target, ...result })}\n`);
  } catch (error) {
    process.stdout.write(
      `${JSON.stringify({ target: '', status: 'failed', error: String(error) })}\n`,
    );
  }
}

if (process.argv[1]?.endsWith('/crawl.js') || process.argv[1]?.endsWith('/crawl/runtime')) {
  await main();
}
