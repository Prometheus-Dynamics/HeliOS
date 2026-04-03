import { apiUrl, getHttpClientBase } from '$lib/api/client';
import { extractError } from '$lib/api/errors';
import { DEFAULT_REQUEST_TIMEOUT_MS, fetchWithRetry } from '$lib/api/requestUtils';

type RetryOptions = {
  maxAttempts?: number;
  baseDelayMs?: number;
  maxDelayMs?: number;
};

type RequestJsonOptions = {
  timeoutMs?: number;
  retry?: RetryOptions;
};

function resolveRequestUrl(path: string): string {
  const trimmed = path.trim();
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed)) {
    return trimmed;
  }
  if (trimmed.startsWith('/v1/')) {
    return `${getHttpClientBase()}${trimmed}`;
  }
  return apiUrl(trimmed);
}

function buildJsonHeaders(init: RequestInit): Headers {
  const headers = new Headers(init.headers ?? {});
  if (!headers.has('accept')) {
    headers.set('accept', 'application/json');
  }
  if (init.body && !headers.has('content-type')) {
    headers.set('content-type', 'application/json');
  }
  return headers;
}

async function requestRaw(path: string, init: RequestInit, options: RequestJsonOptions): Promise<Response> {
  const url = resolveRequestUrl(path);
  const timeoutMs = options.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS;
  const retryOptions = options.retry ?? {};
  return fetchWithRetry(
    url,
    { ...init, headers: buildJsonHeaders(init) },
    { timeoutMs, ...retryOptions }
  );
}

export async function requestJson<T>(path: string, init: RequestInit = {}, options: RequestJsonOptions = {}): Promise<T> {
  const response = await requestRaw(path, init, options);
  if (!response.ok) {
    const detail = await response.text().catch(() => '');
    throw new Error(detail || `Request failed (${response.status})`);
  }
  const contentType = response.headers.get('Content-Type') ?? '';
  if (!contentType.toLowerCase().includes('application/json')) {
    throw new Error('Response was not JSON');
  }
  return (await response.json()) as T;
}

export async function requestOptionalJson<T>(
  path: string,
  init: RequestInit = {},
  options: RequestJsonOptions = {}
): Promise<T | null> {
  const response = await requestRaw(path, init, options);
  if (!response.ok) {
    const detail = await response.text().catch(() => '');
    throw new Error(detail || `Request failed (${response.status})`);
  }
  const contentType = response.headers.get('Content-Type') ?? '';
  if (!contentType.toLowerCase().includes('application/json')) {
    return null;
  }
  return (await response.json()) as T;
}

export function formatFailureReason(reason: unknown): string {
  return extractError(reason);
}

export function collectFailureReasons(results: PromiseSettledResult<unknown>[]): string[] {
  return results
    .filter((result): result is PromiseRejectedResult => result.status === 'rejected')
    .map((result) => formatFailureReason(result.reason))
    .filter((message) => message.length > 0);
}
