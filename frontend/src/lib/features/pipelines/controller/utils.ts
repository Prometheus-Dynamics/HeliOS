import { buildErrorMessage } from '$lib/ui/errorPolicy';

export const optionalString = (value: string | null | undefined): string | undefined =>
  value ?? undefined;

const extractApiMessage = (body: unknown): string | null => {
  if (!body) return null;
  if (typeof body === 'string') {
    const trimmed = body.trim();
    return trimmed.length ? trimmed : null;
  }
  if (typeof body === 'object') {
    const candidate =
      (body as { message?: unknown; error?: unknown; reason?: unknown }).message ??
      (body as { error?: unknown }).error ??
      (body as { reason?: unknown }).reason;
    if (typeof candidate === 'string') {
      const trimmed = candidate.trim();
      return trimmed.length ? trimmed : null;
    }
  }
  return null;
};

export const describeError = (error: unknown): string => {
  const mapped = buildErrorMessage({ error, fallback: 'Unexpected error' });
  if (mapped !== 'Unexpected error') return mapped;
  const fallback = extractApiMessage(error);
  return fallback ?? mapped;
};

export function coercePlanHash(value: unknown): string | null {
  if (typeof value === 'string') {
    const trimmed = value.trim();
    return trimmed ? trimmed : null;
  }
  if (typeof value === 'number' && Number.isFinite(value) && Number.isInteger(value)) {
    return Math.trunc(value).toString(10);
  }
  return null;
}

export function truncatedPlanHashFromRevision(revision?: string | null): string | null {
  if (!revision) return null;
  const trimmed = revision.trim();
  if (!trimmed) return null;
  const len = Math.min(trimmed.length, 16);
  const slice = trimmed.slice(0, len);
  try {
    const value = BigInt(`0x${slice}`);
    if (value === 0n) {
      return null;
    }
    return value.toString(10);
  } catch (error) {
    console.warn('Failed to derive plan hash from revision', error);
    return null;
  }
}

export function joinUrlPath(basePath: string, ...segments: string[]): string {
  const baseParts = basePath.split('/').filter(Boolean);
  const extraParts = segments.flatMap((segment) => segment.split('/').filter(Boolean));
  const parts = [...baseParts, ...extraParts];
  return parts.length === 0 ? '/' : `/${parts.join('/')}`;
}

export function generateNodeId() {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}
