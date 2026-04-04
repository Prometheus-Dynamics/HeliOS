import { browser, dev } from '$app/environment';
import { env } from '$env/dynamic/public';
import { readStorage, removeStorage, writeStorage } from '$lib/utils/storage';

const API_PREFIX = '/v1';
const LOCAL_DEV_BASE = 'http://127.0.0.1:5800';
const STORAGE_KEY = 'helios.apiBase';

function deriveDefaultBase(): string {
  const envBase = env.PUBLIC_API_BASE?.trim();
  if (envBase) return envBase;
  if (browser) {
    const origin = globalThis.location?.origin?.trim();
    if (origin) {
      return origin;
    }
  }
  return LOCAL_DEV_BASE;
}

const DEFAULT_API_BASE = deriveDefaultBase();

function normalizeBase(raw: string): string {
  let trimmed = raw.trim();
  if (!trimmed.length) {
    throw new Error('API base URL is required.');
  }
  trimmed = trimmed.replace(/\/+$/, '');
  if (trimmed.endsWith(API_PREFIX)) {
    trimmed = trimmed.slice(0, -API_PREFIX.length);
  }
  const hasScheme = /^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed);
  const withScheme = hasScheme ? trimmed : `http://${trimmed}`;
  try {
    const parsed = new URL(withScheme);
    const path = parsed.pathname.replace(/\/+$/, '');
    return `${parsed.origin}${path}`;
  } catch {
    throw new Error('API base URL is invalid.');
  }
}

function resolveNormalizedBase(baseOverride?: string | null | undefined): string {
  const trimmed = typeof baseOverride === 'string' ? baseOverride.trim() : '';
  if (trimmed.length > 0) {
    return normalizeBase(trimmed);
  }
  return getHttpClientBase();
}

export function getHttpClientApiBase(): string {
  return `${getHttpClientBase()}${API_PREFIX}`.replace(/\/+$/, '');
}

function loadSavedBase(): string | null {
  if (!browser) return null;
  const stored = readStorage(STORAGE_KEY);
  return stored && stored.trim().length > 0 ? stored : null;
}

function storeBase(base: string): boolean {
  if (!browser) return false;
  return writeStorage(STORAGE_KEY, base);
}

export function setHttpClientBase(base: string): string {
  const normalized = normalizeBase(base);
  const stored = storeBase(normalized);
  if (!stored) {
    throw new Error('Unable to persist API base (local storage unavailable).');
  }
  return normalized;
}

export function resetHttpClientBase(): void {
  normalizeBase(DEFAULT_API_BASE);
  if (browser) removeStorage(STORAGE_KEY);
}

export function getHttpClientBase(): string {
  const stored = loadSavedBase();
  if (stored) {
    try {
      return normalizeBase(stored);
    } catch {
      // fall through to derived defaults
    }
  }
  try {
    return normalizeBase(DEFAULT_API_BASE);
  } catch {
    // In dev, allow same-origin `/v1` fallback only when both stored and current OpenAPI base
    // are unavailable/invalid.
    if (dev && browser) {
      const origin = globalThis.location?.origin?.trim();
      if (origin) {
        try {
          return normalizeBase(origin);
        } catch {
          // keep falling through to default
        }
      }
    }
    if (stored && browser) {
      removeStorage(STORAGE_KEY);
    }
    return normalizeBase(DEFAULT_API_BASE);
  }
}

export function apiUrl(path = '', baseOverride?: string | null): string {
  const base = `${resolveNormalizedBase(baseOverride)}${API_PREFIX}`.replace(/\/+$/, '');
  const trimmedPath = String(path ?? '').trim();
  if (!trimmedPath.length) {
    return base;
  }
  const normalizedPath = trimmedPath.startsWith('/') ? trimmedPath : `/${trimmedPath}`;
  return `${base}${normalizedPath}`;
}
