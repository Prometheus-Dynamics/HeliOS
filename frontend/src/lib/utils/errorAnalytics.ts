import { ApiError } from '$lib/api/client';
import { extractError } from '$lib/api/errors';

type ErrorAnalyticsDetail = {
  endpoint: string;
  status: number | null;
  code: string | null;
  engineCode: string | null;
  message: string;
  occurredAt: number;
};

type ErrorAnalyticsSnapshot = {
  endpoint: string;
  total: number;
  lastAt: number;
  codes: Record<string, number>;
};

const errorCounts = new Map<string, ErrorAnalyticsSnapshot>();

function normalizeString(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed.length ? trimmed.toLowerCase() : null;
}

function parseErrorBody(body: unknown): { code: string | null; engineCode: string | null } {
  if (!body) return { code: null, engineCode: null };
  if (typeof body === 'string') {
    try {
      return parseErrorBody(JSON.parse(body) as unknown);
    } catch {
      return { code: null, engineCode: null };
    }
  }
  if (typeof body !== 'object') return { code: null, engineCode: null };
  const payload = body as Record<string, unknown>;
  const code = normalizeString(payload.code);
  const engineRaw = payload.engine_code ?? payload.engineCode ?? null;
  const engineCode = typeof engineRaw === 'string' ? normalizeString(engineRaw) : engineRaw != null ? String(engineRaw) : null;
  return { code, engineCode };
}

function recordCounts(endpoint: string, code: string | null, occurredAt: number): void {
  const key = endpoint || 'unknown';
  const snapshot = errorCounts.get(key) ?? { endpoint: key, total: 0, lastAt: occurredAt, codes: {} };
  snapshot.total += 1;
  snapshot.lastAt = occurredAt;
  const codeKey = code ?? 'unknown';
  snapshot.codes[codeKey] = (snapshot.codes[codeKey] ?? 0) + 1;
  errorCounts.set(key, snapshot);
}

export function recordApiError(options: { endpoint?: string | null; error: unknown }): void {
  const { endpoint, error } = options;
  const occurredAt = Date.now();
  let status: number | null = null;
  let code: string | null = null;
  let engineCode: string | null = null;
  let endpointLabel = endpoint?.trim() || null;

  if (error instanceof ApiError) {
    status = Number.isFinite(error.status) ? error.status : null;
    const parsed = parseErrorBody(error.body);
    code = parsed.code;
    engineCode = parsed.engineCode;
    if (!endpointLabel) {
      endpointLabel = error.url || null;
    }
  }

  const message = extractError(error);
  const detail: ErrorAnalyticsDetail = {
    endpoint: endpointLabel ?? 'unknown',
    status,
    code,
    engineCode,
    message,
    occurredAt
  };

  recordCounts(detail.endpoint, detail.code ?? detail.engineCode, occurredAt);

  if (typeof window !== 'undefined' && typeof window.dispatchEvent === 'function' && typeof CustomEvent !== 'undefined') {
    window.dispatchEvent(new CustomEvent('helios:api-error', { detail }));
  }
}

export function listApiErrorSnapshots(): ErrorAnalyticsSnapshot[] {
  return Array.from(errorCounts.values()).sort((a, b) => b.lastAt - a.lastAt);
}
