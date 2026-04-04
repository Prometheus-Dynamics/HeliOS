import { apiUrl, getHttpClientBase } from '$lib/api/client';
import { connectionMonitor } from '$lib/api/connection';
import { summarizeErrorBody } from '$lib/api/errors';
import { DEFAULT_REQUEST_TIMEOUT_MS, cancellableWithTimeout, fetchWithRetry } from '$lib/api/requestUtils';
import {
  cacheResourceData,
  cacheResourceNotModified,
  type ResourceCacheContext,
  type ResourceCacheMetadata,
  type ResourceCacheResult
} from '$lib/api/resourceCache';
import { OpenAPI, type CancelablePromise } from '$lib/api/client';
import { recordApiError } from '$lib/utils/errorAnalytics';

export type ApiRequestOptions = {
  timeoutMs?: number;
  maxAttempts?: number;
  label?: string;
  endpoint?: string;
  baseUrl?: string;
  onError?: (error: unknown) => void;
  cacheMs?: number;
  forceRefresh?: boolean;
  recordConnection?: boolean;
};

export type ApiRequestBody = BodyInit | Record<string, unknown> | unknown[] | null;

export type ApiFetchInit = Omit<RequestInit, 'body'> & {
  body?: ApiRequestBody;
  responseMode?: 'auto' | 'json' | 'text' | 'response';
};

function readResponseCacheMetadata(response: Response): ResourceCacheMetadata {
  const etag = response.headers.get('ETag');
  const rawRevision = response.headers.get('x-helios-revision');
  const revision =
    rawRevision && /^\d+$/.test(rawRevision.trim())
      ? Number.parseInt(rawRevision.trim(), 10)
      : null;
  return {
    etag: etag?.trim().length ? etag.trim() : null,
    revision: Number.isFinite(revision) ? revision : null
  };
}

function resolveRequestUrl(pathOrUrl: string, baseUrl?: string): string {
  const trimmed = String(pathOrUrl ?? '').trim();
  if (!trimmed.length) {
    throw new Error('Request path is required');
  }
  if (/^https?:\/\//i.test(trimmed)) {
    return trimmed;
  }
  return apiUrl(trimmed, baseUrl);
}

function isBodyInit(value: unknown): value is BodyInit {
  return (
    typeof value === 'string' ||
    value instanceof Blob ||
    value instanceof FormData ||
    value instanceof URLSearchParams ||
    value instanceof ArrayBuffer ||
    ArrayBuffer.isView(value)
  );
}

function prepareRequest(init: ApiFetchInit = {}): { headers: Headers; body: BodyInit | undefined; responseMode: NonNullable<ApiFetchInit['responseMode']> } {
  const headers = new Headers(init.headers);
  if (!headers.has('Accept')) {
    headers.set('Accept', 'application/json');
  }

  let body: BodyInit | undefined;
  if (typeof init.body !== 'undefined' && init.body !== null) {
    if (isBodyInit(init.body)) {
      body = init.body;
    } else {
      if (!headers.has('Content-Type')) {
        headers.set('Content-Type', 'application/json');
      }
      body = JSON.stringify(init.body);
    }
  }

  return {
    headers,
    body,
    responseMode: init.responseMode ?? 'auto'
  };
}

async function executeRequest(pathOrUrl: string, init: ApiFetchInit = {}, options: ApiRequestOptions = {}): Promise<Response> {
  const url = resolveRequestUrl(pathOrUrl, options.baseUrl);
  const { headers, body } = prepareRequest(init);
  const startedAt = typeof performance !== 'undefined' ? performance.now() : Date.now();
  const recordConnection = options.recordConnection ?? false;

  try {
    const response = await fetchWithRetry(
      url,
      {
        ...init,
        method: init.method ?? 'GET',
        headers,
        body,
        signal: init.signal
      },
      {
        timeoutMs: options.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS,
        maxAttempts: options.maxAttempts ?? 1
      }
    );
    const durationMs = Math.max(0, (typeof performance !== 'undefined' ? performance.now() : Date.now()) - startedAt);
    if (recordConnection) connectionMonitor.recordSuccess(durationMs);
    return response;
  } catch (error) {
    const durationMs = Math.max(0, (typeof performance !== 'undefined' ? performance.now() : Date.now()) - startedAt);
    if (recordConnection) connectionMonitor.recordFailure('api request failed', durationMs);
    if (error instanceof DOMException && error.name === 'AbortError') {
      throw new Error('Request timed out');
    }
    throw error;
  }
}

async function parseResponse<T>(response: Response, responseMode: NonNullable<ApiFetchInit['responseMode']>): Promise<T> {
  if (response.status === 204) {
    return undefined as T;
  }
  if (responseMode === 'response') {
    return response as T;
  }
  if (responseMode === 'text') {
    return (await response.text()) as T;
  }
  if (responseMode === 'json') {
    return (await response.json()) as T;
  }

  const contentType = response.headers.get('Content-Type') ?? '';
  if (contentType.toLowerCase().startsWith('application/json')) {
    return (await response.json()) as T;
  }
  return (await response.text()) as unknown as T;
}

export async function apiFetchResponse(pathOrUrl: string, init?: ApiFetchInit, options?: ApiRequestOptions): Promise<Response> {
  return executeRequest(pathOrUrl, init, options);
}

export async function apiFetchCachedJson<T>(
  pathOrUrl: string,
  context: ResourceCacheContext<T> = {},
  init: ApiFetchInit = {},
  options: ApiRequestOptions = {}
): Promise<ResourceCacheResult<T>> {
  const headers = new Headers(init.headers);
  if (context.etag?.trim()) {
    headers.set('If-None-Match', context.etag.trim());
  }

  const response = await executeRequest(
    pathOrUrl,
    {
      ...init,
      headers
    },
    options
  );

  const metadata = readResponseCacheMetadata(response);
  if (response.status === 304) {
    if (typeof context.cached === 'undefined') {
      throw new Error('Received 304 without cached payload');
    }
    return cacheResourceNotModified<T>({
      etag: metadata.etag ?? context.etag ?? null,
      revision: metadata.revision ?? context.revision ?? null
    });
  }

  if (!response.ok) {
    const text = await response.text().catch(() => '');
    const statusLabel = `Request failed (${response.status})`;
    throw new Error(summarizeErrorBody(text, statusLabel));
  }

  const contentType = response.headers.get('Content-Type') ?? '';
  if (!contentType.toLowerCase().startsWith('application/json')) {
    const text = await response.text().catch(() => '');
    throw new Error(summarizeErrorBody(text, 'Expected JSON response'));
  }

  const data = (await response.json()) as T;
  return cacheResourceData(data, metadata);
}

export async function apiFetch<T>(pathOrUrl: string, init: ApiFetchInit = {}, options: ApiRequestOptions = {}): Promise<T> {
  const response = await executeRequest(pathOrUrl, init, options);
  if (!response.ok) {
    const text = await response.text().catch(() => '');
    const statusLabel = `Request failed (${response.status})`;
    throw new Error(summarizeErrorBody(text, statusLabel));
  }
  return parseResponse<T>(response, init.responseMode ?? 'auto');
}

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
    onError?.(error);
    throw error;
  }
}

export function withAbort<T>(task: (controller: AbortController) => Promise<T>): CancelablePromise<T> {
  const controller = new AbortController();
  const promise = task(controller) as CancelablePromise<T>;
  promise.cancel = () => controller.abort();
  return promise;
}

try {
  getHttpClientBase();
  OpenAPI.ENCODE_PATH = encodeURIComponent;
} catch {
  // ignore
}
