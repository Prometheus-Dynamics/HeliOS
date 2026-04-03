import { browser, dev } from '$app/environment';
import { env } from '$env/dynamic/public';
import { OpenAPI } from '$generated/http/client';
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

function applyBase(normalized: string): string {
  OpenAPI.BASE = `${normalized}${API_PREFIX}`;
  return normalized;
}

export function getHttpClientApiBase(): string {
  if (browser) {
    getHttpClientBase();
  }
  return String(OpenAPI.BASE || `${getHttpClientBase()}${API_PREFIX}`).replace(/\/+$/, '');
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
  applyBase(normalized);
  const stored = storeBase(normalized);
  if (!stored) {
    throw new Error('Unable to persist API base (local storage unavailable).');
  }
  return normalized;
}

export function resetHttpClientBase(): void {
  const normalized = normalizeBase(DEFAULT_API_BASE);
  applyBase(normalized);
  if (browser) removeStorage(STORAGE_KEY);
}

export function getHttpClientBase(): string {
  const stored = loadSavedBase();
  if (stored) {
    try {
      const normalized = normalizeBase(stored);
      return applyBase(normalized);
    } catch {
      // fall through to derived defaults
    }
  }
  const candidate = OpenAPI.BASE ?? DEFAULT_API_BASE;
  try {
    const normalized = normalizeBase(candidate);
    return applyBase(normalized);
  } catch {
    // In dev, allow same-origin `/v1` fallback only when both stored and current OpenAPI base
    // are unavailable/invalid.
    if (dev && browser) {
      const origin = globalThis.location?.origin?.trim();
      if (origin) {
        try {
          return applyBase(normalizeBase(origin));
        } catch {
          // keep falling through to default
        }
      }
    }
    if (stored && browser) {
      removeStorage(STORAGE_KEY);
    }
    return applyBase(normalizeBase(DEFAULT_API_BASE));
  }
}

export function apiUrl(path = ''): string {
  const base = getHttpClientApiBase();
  const trimmedPath = String(path ?? '').trim();
  if (!trimmedPath.length) {
    return base;
  }
  const normalizedPath = trimmedPath.startsWith('/') ? trimmedPath : `/${trimmedPath}`;
  return `${base}${normalizedPath}`;
}

export async function withHttpClientBase<T>(
  base: string | null | undefined,
  run: () => Promise<T> | T
): Promise<T> {
  const trimmed = typeof base === 'string' ? base.trim() : '';
  if (!trimmed.length) {
    return await run();
  }
  const previous = OpenAPI.BASE;
  applyBase(normalizeBase(trimmed));
  try {
    return await run();
  } finally {
    OpenAPI.BASE = previous;
  }
}

// Initialize OpenAPI.BASE once at module import.
try {
  getHttpClientBase();
  OpenAPI.ENCODE_PATH = encodeURIComponent;
} catch {
  // ignore
}
