import type {
  EngineErrorBody,
  ErrorBody,
  ValidationErrorBody,
  ValidationIssue,
  ValidationWarning
} from '$lib/api/client';
import { ApiError } from '$lib/api/client';

type ErrorPayload = Partial<ErrorBody> &
  Partial<ValidationErrorBody> &
  Partial<EngineErrorBody> & {
    message?: string | null;
    issues?: Array<ValidationIssue> | null;
    warnings?: Array<ValidationWarning> | null;
    timestamp_ms?: number | null;
    requestId?: string | null;
    traceId?: string | null;
    reportedBy?: string | null;
  };

export type ValidationReport = {
  issues: ValidationIssue[];
  warnings: ValidationWarning[];
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

function formatValidationEntry(entry: ValidationIssue | ValidationWarning): string | null {
  const message = typeof entry.message === 'string' ? entry.message.trim() : '';
  if (!message.length) return null;
  const path = typeof entry.path === 'string' ? entry.path.trim() : '';
  return path.length ? `${path}: ${message}` : message;
}

function summarizeValidationEntries(entries: Array<ValidationIssue | ValidationWarning> | null | undefined): string | null {
  if (!Array.isArray(entries) || entries.length === 0) return null;
  const messages = entries.map((entry) => formatValidationEntry(entry)).filter((entry): entry is string => Boolean(entry));
  if (!messages.length) return null;
  const primary = messages.slice(0, 3).join('; ');
  const remaining = messages.length - 3;
  return remaining > 0 ? `${primary}; +${remaining} more` : primary;
}

function normalizeValidationIssues(entries: Array<ValidationIssue> | null | undefined): ValidationIssue[] {
  if (!Array.isArray(entries)) return [];
  return entries.filter(
    (entry): entry is ValidationIssue =>
      Boolean(entry) &&
      typeof entry.code === 'string' &&
      typeof entry.message === 'string' &&
      typeof entry.path === 'string'
  );
}

function normalizeValidationWarnings(entries: Array<ValidationWarning> | null | undefined): ValidationWarning[] {
  if (!Array.isArray(entries)) return [];
  return entries.filter(
    (entry): entry is ValidationWarning =>
      Boolean(entry) &&
      typeof entry.code === 'string' &&
      typeof entry.message === 'string' &&
      typeof entry.path === 'string'
  );
}

function validationReportFromPayload(payload: ErrorPayload | null | undefined): ValidationReport | null {
  if (!payload || typeof payload !== 'object') return null;
  const issues = normalizeValidationIssues(payload.issues ?? null);
  const warnings = normalizeValidationWarnings(payload.warnings ?? null);
  if (!issues.length && !warnings.length) return null;
  return { issues, warnings };
}

function formatPayloadMessage(payload: ErrorPayload): string | null {
  const codeMessage = mapErrorCode(payload.code ?? undefined);
  const message = typeof payload.error === 'string' && payload.error.trim().length ? payload.error.trim() : null;
  const apiMessage = typeof payload.message === 'string' && payload.message.trim().length ? payload.message.trim() : null;
  const details = typeof payload.details === 'string' && payload.details.trim().length ? payload.details.trim() : null;
  const issues = summarizeValidationEntries(payload.issues ?? null);
  if (message && issues) return `${message} ${issues}`;
  if (apiMessage && issues) return `${apiMessage} ${issues}`;
  if (codeMessage && issues) return `${codeMessage} ${issues}`;
  if (issues) return issues;
  if (codeMessage && message) return `${codeMessage} ${message}`;
  if (codeMessage && apiMessage) return `${codeMessage} ${apiMessage}`;
  if (codeMessage && details) return `${codeMessage} ${details}`;
  if (codeMessage) return codeMessage;
  return message ?? apiMessage ?? details;
}

export function summarizeErrorBody(raw: string | null | undefined, statusLabel: string): string {
  if (!raw) return statusLabel;
  try {
    const parsed = JSON.parse(raw) as ErrorPayload | string;
    if (typeof parsed === 'string' && parsed.trim().length) return parsed.trim();
    if (parsed && typeof parsed === 'object') {
      const payloadMessage = formatPayloadMessage(parsed);
      if (payloadMessage) return payloadMessage;
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
  return formatPayloadMessage(payload as ErrorPayload);
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
  validationIssues?: ValidationIssue[] | null;
  validationWarnings?: ValidationWarning[] | null;
};

export function extractErrorMetadata(err: unknown): ErrorMetadata | null {
  if (!(err instanceof ApiError)) return null;
  const payload = err.body as ErrorPayload | null | undefined;
  if (!payload || typeof payload !== 'object') return null;
  const validation = validationReportFromPayload(payload);
  const remediationFromIssue = validation?.issues.find((issue) => typeof issue.remediation === 'string' && issue.remediation.trim().length)?.remediation ?? null;
  return {
    code: payload.code ?? null,
    timestampMs:
      typeof payload.timestampMs === 'number'
        ? payload.timestampMs
        : typeof payload.timestamp_ms === 'number'
          ? payload.timestamp_ms
          : null,
    source: payload.source ?? null,
    operation: payload.operation ?? null,
    requestId: payload.requestId ?? payload.request_id ?? null,
    traceId: payload.traceId ?? payload.trace_id ?? null,
    retryable: payload.retryable ?? null,
    remediation: payload.remediation ?? remediationFromIssue,
    reportedBy: payload.reportedBy ?? payload.reported_by ?? null,
    validationIssues: validation?.issues ?? null,
    validationWarnings: validation?.warnings ?? null
  };
}

export function extractValidationReport(err: unknown): ValidationReport | null {
  if (!(err instanceof ApiError)) return null;
  const payload = err.body as ErrorPayload | null | undefined;
  return validationReportFromPayload(payload);
}

export function extractMessage(raw: string | null | undefined): string | null {
  if (!raw) return null;
  const trimmed = raw.trim();
  if (!trimmed.length) return null;
  try {
    const parsed = JSON.parse(trimmed) as ErrorPayload | string;
    if (typeof parsed === 'string' && parsed.trim().length) return parsed.trim();
    if (parsed && typeof parsed === 'object') {
      const payloadMessage = formatPayloadMessage(parsed);
      if (payloadMessage) return payloadMessage;
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
