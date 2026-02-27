import { ApiError } from '$lib/ts-bindings/http/client';

type ErrorPayload = {
  code?: string | null;
  error?: string | null;
  message?: string | null;
  details?: string | null;
  timestamp_ms?: number | null;
  source?: string | null;
  operation?: string | null;
  request_id?: string | null;
  trace_id?: string | null;
  retryable?: boolean | null;
  remediation?: string | null;
  reported_by?: string | null;
};

const CODE_MESSAGES: Record<string, string> = {
  bad_request: 'Invalid request.',
  not_found: 'Not found.',
  bad_gateway: 'Backend is unavailable.',
  service_unavailable: 'Service unavailable.',
  payload_too_large: 'Payload too large.',
  internal: 'Internal server error.',
  engine_error: 'Engine error.',
  engine_unavailable: 'Engine unavailable.',
  engine_unexpected: 'Unexpected engine response.',
  unimplemented: 'Feature not implemented.',
  invalid_state: 'Invalid state.',
  invalid_input: 'Invalid input.',
  conflict: 'Conflict.',
  timeout: 'Request timed out.',
  busy: 'Resource is busy.'
};

export function mapErrorCode(code: string | null | undefined): string | null {
  if (!code) return null;
  const normalized = code.trim().toLowerCase();
  return CODE_MESSAGES[normalized] ?? null;
}

export function summarizeErrorBody(raw: string | null | undefined, statusLabel: string): string {
  if (!raw) return statusLabel;
  try {
    const parsed = JSON.parse(raw) as ErrorPayload | string;
    if (typeof parsed === 'string' && parsed.trim().length) return parsed.trim();
    if (parsed && typeof parsed === 'object') {
      const codeMessage = mapErrorCode(parsed.code ?? undefined);
      const message = typeof parsed.error === 'string' && parsed.error.trim().length ? parsed.error.trim() : null;
      const details = typeof parsed.details === 'string' && parsed.details.trim().length ? parsed.details.trim() : null;
      if (codeMessage && message) return `${codeMessage} ${message}`;
      if (codeMessage) return codeMessage;
      if (message) return message;
      if (details) return details;
    }
  } catch {
    // not JSON; fall through
  }
  const trimmed = raw.trim();
  if (!trimmed.length) return statusLabel;
  const lower = trimmed.toLowerCase();
  if (lower.startsWith('<!doctype') || lower.startsWith('<html') || lower.includes('</html>')) {
    return statusLabel;
  }
  return trimmed.length > 240 ? `${trimmed.slice(0, 240)}...` : trimmed;
}

function formatErrorPayload(payload: unknown): string | null {
  if (!payload) return null;
  if (typeof payload === 'string') {
    const trimmed = payload.trim();
    return trimmed.length ? trimmed : null;
  }
  if (typeof payload !== 'object') return null;
  const record = payload as ErrorPayload;
  const codeMessage = mapErrorCode(record.code ?? undefined);
  const message = typeof record.error === 'string' && record.error.trim().length ? record.error.trim() : null;
  const details = typeof record.details === 'string' && record.details.trim().length ? record.details.trim() : null;
  const apiMessage = typeof record.message === 'string' && record.message.trim().length ? record.message.trim() : null;
  if (codeMessage && message) return `${codeMessage} ${message}`;
  if (codeMessage && apiMessage) return `${codeMessage} ${apiMessage}`;
  if (codeMessage && details) return `${codeMessage} ${details}`;
  if (codeMessage) return codeMessage;
  return message ?? apiMessage ?? details;
}

export function extractError(err: unknown): string {
  if (err instanceof ApiError) {
    const payloadMessage = formatErrorPayload(err.body);
    if (payloadMessage) return payloadMessage;
    const fallback = `${err.status} ${err.statusText}`.trim();
    return fallback.length ? fallback : err.message || 'Request failed';
  }
  if (err instanceof Error && err.message) {
    if (err.message.trim().toLowerCase() === 'timeout') return CODE_MESSAGES.timeout ?? 'Request timed out.';
    return err.message;
  }
  return typeof err === 'string' && err.length ? err : 'Unexpected error';
}

export type ErrorMetadata = {
  code?: string | null;
  timestampMs?: number | null;
  source?: string | null;
  operation?: string | null;
  requestId?: string | null;
  traceId?: string | null;
  retryable?: boolean | null;
  remediation?: string | null;
  reportedBy?: string | null;
};

export function extractErrorMetadata(err: unknown): ErrorMetadata | null {
  if (!(err instanceof ApiError)) return null;
  const payload = err.body as ErrorPayload | null | undefined;
  if (!payload || typeof payload !== 'object') return null;
  return {
    code: payload.code ?? null,
    timestampMs: typeof payload.timestamp_ms === 'number' ? payload.timestamp_ms : null,
    source: payload.source ?? null,
    operation: payload.operation ?? null,
    requestId: payload.request_id ?? null,
    traceId: payload.trace_id ?? null,
    retryable: payload.retryable ?? null,
    remediation: payload.remediation ?? null,
    reportedBy: payload.reported_by ?? null
  };
}

export function extractMessage(raw: string | null | undefined): string | null {
  if (!raw) return null;
  const trimmed = raw.trim();
  if (!trimmed.length) return null;
  try {
    const parsed = JSON.parse(trimmed) as ErrorPayload | string;
    if (typeof parsed === 'string' && parsed.trim().length) return parsed.trim();
    if (parsed && typeof parsed === 'object') {
      const codeMessage = mapErrorCode(parsed.code ?? undefined);
      const message = typeof parsed.error === 'string' && parsed.error.trim().length ? parsed.error.trim() : null;
      if (codeMessage && message) return `${codeMessage} ${message}`;
      if (codeMessage) return codeMessage;
      if (message) return message;
      if (typeof parsed.message === 'string' && parsed.message.trim().length) return parsed.message.trim();
    }
  } catch {
    // not JSON; fall through
  }
  const lower = trimmed.toLowerCase();
  if (lower.startsWith('<!doctype') || lower.startsWith('<html') || lower.includes('</html>')) {
    return null;
  }
  return trimmed.length > 240 ? `${trimmed.slice(0, 240)}...` : trimmed;
}
