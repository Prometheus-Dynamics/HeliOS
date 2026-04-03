import type { ProbedBackend } from '$lib/api/client';

export const buildApiPath = (base: string, path: string): string => {
  const normalizedBase = String(base ?? '').replace(/\/+$/, '');
  const normalizedPath = path.startsWith('/') ? path : `/${path}`;
  return `${normalizedBase}${normalizedPath}`;
};

export const isTimeoutError = (err: unknown): boolean => {
  if (!err) return false;
  const message = typeof err === 'string' ? err : (err as { message?: unknown })?.message ?? '';
  return String(message).toLowerCase().includes('timeout');
};

export const backendLabel = (backend: ProbedBackend | null): string =>
  backend?.kind ? String(backend.kind) : 'Backend';

export const modeKey = (value: unknown): string | null => {
  if (value === null || value === undefined) return null;
  const raw =
    typeof value === 'string' || typeof value === 'number'
      ? value
      : (value as { id?: unknown; name?: unknown; key?: unknown })?.id ??
        (value as { id?: unknown; name?: unknown; key?: unknown })?.name ??
        (value as { id?: unknown; name?: unknown; key?: unknown })?.key ??
        null;
  if (raw === null || raw === undefined) return null;
  const str = String(raw).trim();
  return str.length ? str : null;
};
