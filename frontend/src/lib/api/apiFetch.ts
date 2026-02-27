import { OpenAPI } from '$lib/ts-bindings/http/client';
import { summarizeErrorBody } from '$lib/api/errors';
import { connectionMonitor } from '$lib/api/connection';
import { DEFAULT_REQUEST_TIMEOUT_MS, fetchWithRetry } from '$lib/api/requestUtils';

type ApiFetchOptions = {
  timeoutMs?: number;
  maxAttempts?: number;
  // Most API calls should affect the global backend connection banner.
  // Some calls (e.g. probes that depend on external devices) should not.
  recordConnection?: boolean;
};

export async function apiFetch<T>(path: string, init?: RequestInit, options: ApiFetchOptions = {}): Promise<T> {
  const url = `${OpenAPI.BASE}${path}`;
  const headers = new Headers(init?.headers);
  if (!headers.has('Accept')) {
    headers.set('Accept', 'application/json');
  }
  const hasBody = typeof init?.body !== 'undefined' && !(init?.body instanceof FormData);
  if (hasBody && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json');
  }

  const startedAt = typeof performance !== 'undefined' ? performance.now() : Date.now();
  // Connectivity should be driven by the heartbeat probe; treat other requests as application-level.
  const recordConnection = options.recordConnection ?? false;
  let response: Response;
  try {
    response = await fetchWithRetry(
      url,
      {
        method: init?.method ?? 'GET',
        body: init?.body,
        headers,
        signal: init?.signal
      },
      { timeoutMs: options.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS, maxAttempts: options.maxAttempts ?? 1 }
    );
  } catch (error) {
    const durationMs = Math.max(0, (typeof performance !== 'undefined' ? performance.now() : Date.now()) - startedAt);
    if (recordConnection) connectionMonitor.recordFailure('api request failed', durationMs);
    if (error instanceof DOMException && error.name === 'AbortError') {
      throw new Error('Request timed out');
    }
    throw error;
  }

  if (!response.ok) {
    const text = await response.text().catch(() => '');
    const statusLabel = `Request failed (${response.status})`;
    const durationMs = Math.max(0, (typeof performance !== 'undefined' ? performance.now() : Date.now()) - startedAt);
    if (recordConnection) connectionMonitor.recordFailure(statusLabel, durationMs);
    throw new Error(summarizeErrorBody(text, statusLabel));
  }

  {
    const durationMs = Math.max(0, (typeof performance !== 'undefined' ? performance.now() : Date.now()) - startedAt);
    if (recordConnection) connectionMonitor.recordSuccess(durationMs);
  }

  if (response.status === 204) {
    return undefined as T;
  }

  const contentType = response.headers.get('Content-Type') ?? '';
  if (contentType.toLowerCase().startsWith('application/json')) {
    return (await response.json()) as T;
  }

  const text = await response.text();
  return text as unknown as T;
}
