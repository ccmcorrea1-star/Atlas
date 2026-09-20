#!/usr/bin/env node
// Tool web.search: usa adapters substituiveis e mantém o SearXNG como default local.
import { readFileSync } from 'node:fs';

export interface SearchResult {
  title: string;
  url: string;
  snippet?: string;
  source?: string;
  publishedAt?: string;
  score?: number;
}

export interface SearchOutcome {
  status: 'success' | 'failed' | 'unavailable';
  error: string;
  results: SearchResult[];
}

export interface SearchConfig {
  provider?: string;
  endpoint?: string;
  timeoutMs?: number;
  fallbackProviders?: string[];
  providers?: Record<string, { endpoint?: string; apiKey?: string }>;
}

export type SearchCategory = 'general' | 'news';
export type SearchTimeRange = 'day' | 'week' | 'month';

export interface SearchOptions {
  category?: SearchCategory;
  timeRange?: SearchTimeRange;
  domains?: string[];
}

export interface SearchProvider {
  search(
    query: string,
    limit: number,
    config: SearchConfig,
    options?: SearchOptions,
  ): Promise<unknown>;
}

export type SearchProviderFactory = (config: SearchConfig) => SearchProvider;

const DEFAULT_LIMIT = 5;
const MAX_LIMIT = 20;
const DEFAULT_ENDPOINT = 'http://searxng.home';
const DEFAULT_TIMEOUT_MS = 15000;

const providerFactories = new Map<string, SearchProviderFactory>();

class ProviderError extends Error {}

class SearXNGProvider implements SearchProvider {
  public async search(
    query: string,
    limit: number,
    config: SearchConfig,
    options: SearchOptions = {},
  ): Promise<unknown> {
    const endpoint = config.endpoint ?? DEFAULT_ENDPOINT;
    const url = new URL(
      endpoint.endsWith('/search') ? endpoint : `${endpoint.replace(/\/$/, '')}/search`,
    );
    url.searchParams.set('q', queryWithDomains(query, options.domains));
    url.searchParams.set('format', 'json');
    url.searchParams.set('categories', options.category ?? 'general');
    if (options.timeRange !== undefined) {
      url.searchParams.set('time_range', options.timeRange);
    }

    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), config.timeoutMs ?? DEFAULT_TIMEOUT_MS);
    try {
      const response = await fetch(url, { signal: controller.signal });
      if (!response.ok) {
        throw new ProviderError(`SearXNG returned HTTP ${response.status}`);
      }
      const payload = (await response.json()) as { results?: unknown };
      if (!Array.isArray(payload.results)) {
        throw new ProviderError('SearXNG returned an invalid result envelope');
      }
      return sanitizeResults(payload.results.map(normalizeSearXNGResult)).slice(0, limit);
    } catch (error) {
      if (error instanceof Error && error.name === 'AbortError') {
        throw new ProviderError('SearXNG request timed out');
      }
      throw error instanceof Error ? error : new ProviderError(String(error));
    } finally {
      clearTimeout(timeout);
    }
  }
}

function queryWithDomains(query: string, domains: string[] | undefined): string {
  if (domains === undefined || domains.length === 0) {
    return query;
  }
  const filters = domains.map((domain) => `site:${domain}`).join(' OR ');
  return `${query} (${filters})`;
}

function normalizeSearXNGResult(entry: unknown): unknown {
  if (entry === null || typeof entry !== 'object') {
    return entry;
  }
  const result = entry as Record<string, unknown>;
  const normalized: Record<string, unknown> = {
    title: result.title,
    url: result.url,
  };
  const snippet = typeof result.content === 'string' ? result.content : result.snippet;
  if (typeof snippet === 'string') {
    normalized.snippet = snippet;
  }
  const source = typeof result.engine === 'string' ? result.engine : result.source;
  if (typeof source === 'string') {
    normalized.source = source;
  }
  const publishedAt =
    typeof result.publishedDate === 'string'
      ? result.publishedDate
      : typeof result.publishedAt === 'string'
        ? result.publishedAt
        : result.published_at;
  if (typeof publishedAt === 'string') {
    normalized.publishedAt = publishedAt;
  }
  if (typeof result.score === 'number' && Number.isFinite(result.score)) {
    normalized.score = result.score;
  }
  return normalized;
}

providerFactories.set('searxng', () => new SearXNGProvider());

export function registerSearchProvider(name: string, factory: SearchProviderFactory): void {
  if (!name.trim()) {
    throw new Error('search provider name cannot be empty');
  }
  providerFactories.set(name, factory);
}

export function unregisterSearchProvider(name: string): void {
  if (name !== 'searxng') {
    providerFactories.delete(name);
  }
}

export function parseRequest(text: string): {
  target: string;
  query: string;
  limit: number;
  category?: SearchCategory;
  timeRange?: SearchTimeRange;
  domains?: string[];
} {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new Error('invalid JSON request');
  }
  if (typeof value !== 'object' || value === null) {
    throw new Error('request must be a JSON object');
  }
  const record = value as Record<string, unknown>;
  if (typeof record.target !== 'string') {
    throw new Error("field 'target' must be a string");
  }
  if (typeof record.query !== 'string' || record.query.trim() === '') {
    throw new Error("field 'query' must be a non-empty string");
  }
  let limit = DEFAULT_LIMIT;
  if (record.limit !== undefined) {
    if (typeof record.limit !== 'number' || !Number.isInteger(record.limit) || record.limit < 1) {
      throw new Error("field 'limit' must be a positive integer");
    }
    limit = Math.min(record.limit, MAX_LIMIT);
  }
  let category: SearchCategory | undefined;
  if (record.category !== undefined) {
    if (record.category !== 'general' && record.category !== 'news') {
      throw new Error("field 'category' must be 'general' or 'news'");
    }
    category = record.category;
  }
  let timeRange: SearchTimeRange | undefined;
  if (record.timeRange !== undefined) {
    if (record.timeRange !== 'day' && record.timeRange !== 'week' && record.timeRange !== 'month') {
      throw new Error("field 'timeRange' must be 'day', 'week' or 'month'");
    }
    timeRange = record.timeRange;
  }
  let domains: string[] | undefined;
  if (record.domains !== undefined) {
    if (
      !Array.isArray(record.domains) ||
      record.domains.some((domain) => typeof domain !== 'string' || domain.trim() === '')
    ) {
      throw new Error("field 'domains' must be an array of non-empty strings");
    }
    domains = record.domains.map((domain) => domain.trim());
  }
  return {
    target: record.target,
    query: record.query.trim(),
    limit,
    ...(category === undefined ? {} : { category }),
    ...(timeRange === undefined ? {} : { timeRange }),
    ...(domains === undefined ? {} : { domains }),
  };
}

export function sanitizeResults(value: unknown): SearchResult[] {
  if (!Array.isArray(value)) {
    throw new Error('search provider returned invalid JSON: expected an array');
  }
  const results: SearchResult[] = [];
  const seenUrls = new Set<string>();
  for (const entry of value) {
    if (typeof entry !== 'object' || entry === null) {
      continue;
    }
    const record = entry as Record<string, unknown>;
    if (typeof record.title !== 'string' || record.title === '') {
      continue;
    }
    if (typeof record.url !== 'string' || record.url === '') {
      continue;
    }
    if (seenUrls.has(record.url)) {
      continue;
    }
    seenUrls.add(record.url);
    const result: SearchResult = { title: record.title, url: record.url };
    if (typeof record.snippet === 'string' && record.snippet !== '') {
      result.snippet = record.snippet;
    }
    if (typeof record.source === 'string' && record.source !== '') {
      result.source = record.source;
    }
    const publishedAt =
      typeof record.publishedAt === 'string'
        ? record.publishedAt
        : typeof record.publishedDate === 'string'
          ? record.publishedDate
          : undefined;
    if (publishedAt !== undefined && publishedAt !== '') {
      result.publishedAt = publishedAt;
    }
    if (typeof record.score === 'number' && Number.isFinite(record.score)) {
      result.score = record.score;
    }
    results.push(result);
  }
  return results;
}

export function searchConfigFromEnvironment(env: NodeJS.ProcessEnv = process.env): SearchConfig {
  const inline = env.ATLAS_WEB_CONFIG_JSON;
  if (inline) {
    try {
      const parsed = JSON.parse(inline) as { search?: SearchConfig };
      return parsed.search ?? {};
    } catch {
      return {};
    }
  }
  const path = env.ATLAS_CONFIG;
  if (path) {
    try {
      const parsed = JSON.parse(readFileSync(path, 'utf8')) as { web?: { search?: SearchConfig } };
      return parsed.web?.search ?? {};
    } catch {
      return {};
    }
  }
  return {};
}

export async function runSearch(
  query: string,
  limit: number,
  config: SearchConfig = searchConfigFromEnvironment(),
  providers: ReadonlyMap<string, SearchProviderFactory> = providerFactories,
  options: SearchOptions = {},
): Promise<SearchOutcome> {
  const names = [config.provider ?? 'searxng', ...(config.fallbackProviders ?? [])];
  const attempted = new Set<string>();
  let lastError = 'no search provider configured';

  for (const name of names) {
    if (attempted.has(name)) {
      continue;
    }
    attempted.add(name);
    const factory = providers.get(name);
    if (factory === undefined) {
      lastError = `search provider '${name}' is not installed`;
      continue;
    }
    try {
      const raw = await factory(config).search(query, limit, config, options);
      return { status: 'success', error: '', results: sanitizeResults(raw).slice(0, limit) };
    } catch (error) {
      lastError = `${name}: ${error instanceof Error ? error.message : String(error)}`;
    }
  }

  return {
    status: names.length === 0 ? 'unavailable' : 'failed',
    error: lastError,
    results: [],
  };
}

async function main(): Promise<void> {
  let input = '';
  process.stdin.setEncoding('utf8');
  for await (const chunk of process.stdin) {
    input += chunk;
  }
  try {
    const request = parseRequest(input);
    const outcome = await runSearch(request.query, request.limit, undefined, undefined, request);
    process.stdout.write(`${JSON.stringify({ target: request.target, ...outcome })}\n`);
  } catch (error) {
    process.stdout.write(
      `${JSON.stringify({ target: '', status: 'failed', error: String(error), results: [] })}\n`,
    );
  }
}

if (process.argv[1]?.endsWith('/search.js') || process.argv[1]?.endsWith('/search/runtime')) {
  await main();
}
