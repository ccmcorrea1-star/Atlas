#!/usr/bin/env node
// Tool web.search: consulta o provider configurado e retorna resultados reais.
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export interface SearchResult {
  title: string;
  url: string;
  snippet?: string;
  source?: string;
}

export interface SearchOutcome {
  status: 'success' | 'failed' | 'unavailable';
  error: string;
  results: SearchResult[];
}

const DEFAULT_LIMIT = 5;
const MAX_LIMIT = 20;
const PROVIDER_ENV = 'ATLAS_WEB_SEARCH_COMMAND';
const PROVIDER_TIMEOUT_MS = 15000;

export function parseRequest(text: string): { target: string; query: string; limit: number } {
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
  return { target: record.target, query: record.query.trim(), limit };
}

// Divide o comando respeitando aspas simples e duplas.
export function splitCommand(template: string): string[] {
  const parts: string[] = [];
  let current = '';
  let quote = '';
  for (const char of template) {
    if (quote !== '') {
      if (char === quote) {
        quote = '';
      } else {
        current += char;
      }
    } else if (char === '"' || char === "'") {
      quote = char;
    } else if (char === ' ' || char === '\t') {
      if (current !== '') {
        parts.push(current);
        current = '';
      }
    } else {
      current += char;
    }
  }
  if (current !== '') {
    parts.push(current);
  }
  return parts;
}

// Substitui {query} e {limit}; sem placeholders, anexa query e limit ao final.
export function buildCommand(template: string, query: string, limit: number): string[] {
  if (template.trim() === '') {
    throw new Error('search provider command is empty');
  }
  const parts = splitCommand(template).map((part) => {
    if (part === '{query}') {
      return query;
    }
    return part === '{limit}' ? String(limit) : part;
  });
  if (!template.includes('{query}') && !template.includes('{limit}')) {
    parts.push(query, String(limit));
  }
  if (parts.length === 0) {
    throw new Error('search provider command is empty');
  }
  return parts;
}

// Mantém só entradas com título e URL; o resto é descartado, nunca inventado.
export function sanitizeResults(value: unknown): SearchResult[] {
  if (!Array.isArray(value)) {
    throw new Error('search provider returned invalid JSON: expected an array');
  }
  const results: SearchResult[] = [];
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
    const result: SearchResult = { title: record.title, url: record.url };
    if (typeof record.snippet === 'string' && record.snippet !== '') {
      result.snippet = record.snippet;
    }
    if (typeof record.source === 'string' && record.source !== '') {
      result.source = record.source;
    }
    results.push(result);
  }
  return results;
}

export function runSearch(
  query: string,
  limit: number,
  provider = process.env[PROVIDER_ENV],
): SearchOutcome {
  if (provider === undefined || provider.trim() === '') {
    return { status: 'unavailable', error: 'no search provider configured', results: [] };
  }
  let command: string[];
  try {
    command = buildCommand(provider, query, limit);
  } catch (error) {
    return { status: 'failed', error: String(error), results: [] };
  }
  const [program, ...args] = command;
  // Sem shell: o provider executa direto, sem interpretação.
  const completed = spawnSync(program, args, { encoding: 'utf8', timeout: PROVIDER_TIMEOUT_MS });
  if (completed.error !== undefined) {
    return {
      status: 'failed',
      error: `search provider failed: ${completed.error.message}`,
      results: [],
    };
  }
  if (completed.status !== 0) {
    const detail = completed.stderr.trim().slice(0, 500);
    return {
      status: 'failed',
      error: `search provider exited with code ${String(completed.status)}${detail === '' ? '' : `: ${detail}`}`,
      results: [],
    };
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(completed.stdout.trim());
  } catch {
    return { status: 'failed', error: 'search provider returned invalid JSON', results: [] };
  }
  try {
    return { status: 'success', error: '', results: sanitizeResults(parsed).slice(0, limit) };
  } catch (error) {
    return { status: 'failed', error: String(error), results: [] };
  }
}

function respond(target: string, outcome: SearchOutcome): void {
  process.stdout.write(
    `${JSON.stringify({ target, status: outcome.status, error: outcome.error, results: outcome.results })}\n`,
  );
}

function main(): void {
  let input = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', (chunk: string) => {
    input += chunk;
  });
  process.stdin.on('end', () => {
    try {
      const request = parseRequest(input);
      respond(request.target, runSearch(request.query, request.limit));
    } catch (error) {
      respond('', { status: 'failed', error: String(error), results: [] });
    }
  });
}

const invoked = process.argv[1] === undefined ? '' : resolve(process.argv[1]);
if (import.meta.url === pathToFileURL(invoked).href) {
  main();
}
