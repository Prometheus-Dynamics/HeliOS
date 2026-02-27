import type { CancelablePromise } from '$lib/ts-bindings/http/client';
import { connectionMonitor } from '$lib/api/connection';
import { cancellableWithTimeout, DEFAULT_REQUEST_TIMEOUT_MS } from '$lib/api/requestUtils';
import { getHttpClientBase } from '$lib/api/httpClient';
import { recordApiError } from '$lib/utils/errorAnalytics';

export type ApiRequestOptions = {
  timeoutMs?: number;
  label?: string;
  endpoint?: string;
  onError?: (error: unknown) => void;
  cacheMs?: number;
  forceRefresh?: boolean;
  recordConnection?: boolean;
};

export async function runApiRequest<T>(
  factory: () => CancelablePromise<T>,
  options: ApiRequestOptions = {}
): Promise<T> {
  try {
    getHttpClientBase();
  } catch {
    // Keep request path behavior unchanged when base initialization fails.
  }
  const { timeoutMs = DEFAULT_REQUEST_TIMEOUT_MS, label, endpoint, onError, recordConnection = false } = options;
  const startedAt = typeof performance !== 'undefined' ? performance.now() : Date.now();
  try {
    const result = await cancellableWithTimeout(factory, timeoutMs);
    const durationMs = Math.max(0, (typeof performance !== 'undefined' ? performance.now() : Date.now()) - startedAt);
    if (recordConnection) connectionMonitor.recordSuccess(durationMs);
    return result;
  } catch (error) {
    const durationMs = Math.max(0, (typeof performance !== 'undefined' ? performance.now() : Date.now()) - startedAt);
    const reason = label ? `${label} failed` : 'API request failed';
    if (recordConnection) connectionMonitor.recordFailure(reason, durationMs);
    recordApiError({ endpoint: endpoint ?? label ?? null, error });
    if (onError) {
      onError(error);
    }
    throw error;
  }
}
