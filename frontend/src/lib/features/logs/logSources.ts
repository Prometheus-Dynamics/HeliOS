import type { LogStreamSummary } from './types';

export const LOG_SOURCES_CACHE_KEY = 'logs:sources:v1';
export const LOG_SOURCES_CACHE_STALE_MS = 10_000;
export const LOG_SOURCES_CACHE_MAX_MS = 120_000;

export function normalizeCachedSources(sources: LogStreamSummary[]): LogStreamSummary[] {
  return sources.map((source) => ({
    id: source.id,
    label: source.label,
    rotation_max_bytes: source.rotation_max_bytes ?? null,
    retention_count: source.retention_count ?? null
  }));
}

export function normalizeApiSources(sources: LogStreamSummary[]): LogStreamSummary[] {
  return sources.map((source) => ({
    id: source.id,
    label: source.label,
    rotation_max_bytes: null,
    retention_count: null
  }));
}

export function selectPreferredStream(current: string | null, sources: LogStreamSummary[]): string | null {
  if (!sources.length) return null;
  if (current && sources.some((stream) => stream.id === current)) {
    return current;
  }
  return sources[0]?.id ?? null;
}
