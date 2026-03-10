import type { CancelablePromise } from '$lib/ts-bindings/http/client';
import { getHttpClientBase } from '$lib/api/httpClient';

export const DEFAULT_REQUEST_TIMEOUT_MS = 5000;

const RETRYABLE_METHODS = new Set(['GET', 'HEAD', 'OPTIONS']);
const RETRYABLE_STATUS = new Set([408, 425, 429, 500, 502, 503, 504]);

type RetryOptions = {
  timeoutMs?: number;
  maxAttempts?: number;
  baseDelayMs?: number;
  maxDelayMs?: number;
  factor?: number;
  retryStatus?: (status: number) => boolean;
};

const DEFAULT_RETRY_OPTIONS: Required<Omit<RetryOptions, 'retryStatus'>> = {
  timeoutMs: DEFAULT_REQUEST_TIMEOUT_MS,
  maxAttempts: Number.POSITIVE_INFINITY,
  baseDelayMs: 2000,
  maxDelayMs: 20_000,
  factor: 1.5
};

function nextDelay(attempt: number, options: Required<Omit<RetryOptions, 'retryStatus'>>): number {
  const exponential = options.baseDelayMs * Math.pow(options.factor, Math.max(0, attempt - 1));
  return Math.min(options.maxDelayMs, Math.max(options.baseDelayMs, Math.round(exponential)));
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function shouldRetryStatus(status: number, options: RetryOptions): boolean {
  return (options.retryStatus ? options.retryStatus(status) : RETRYABLE_STATUS.has(status));
}

export async function fetchWithRetry(url: string, init: RequestInit = {}, options: RetryOptions = {}): Promise<Response> {
  const method = (init.method ?? 'GET').toUpperCase();
  const retryEligible = RETRYABLE_METHODS.has(method);
  const resolved = { ...DEFAULT_RETRY_OPTIONS, ...options };
  const maxAttempts = retryEligible ? Math.max(1, resolved.maxAttempts) : 1;
  const externalSignal = init.signal;
  let attempt = 0;

  while (attempt < maxAttempts) {
    if (externalSignal?.aborted) {
      throw new DOMException('Request aborted', 'AbortError');
    }
    const controller = new AbortController();
    let timedOut = false;
    const timeoutId = setTimeout(() => {
      timedOut = true;
      controller.abort();
    }, resolved.timeoutMs);
    const signal = controller.signal;
    let abortListener: (() => void) | null = null;

    if (externalSignal) {
      if (externalSignal.aborted) {
        controller.abort();
      } else {
        abortListener = () => controller.abort();
        externalSignal.addEventListener('abort', abortListener, { once: true });
      }
    }

    try {
      const response = await fetch(url, { ...init, method, signal });
      if (!response.ok && retryEligible && shouldRetryStatus(response.status, options) && attempt + 1 < maxAttempts) {
        attempt += 1;
        await sleep(nextDelay(attempt, resolved));
        continue;
      }
      return response;
    } catch (error) {
      if (externalSignal?.aborted) {
        throw error;
      }
      const resolvedError = timedOut ? new Error('Request timed out') : error;
      if (!retryEligible || attempt + 1 >= maxAttempts) {
        throw resolvedError;
      }
      attempt += 1;
      await sleep(nextDelay(attempt, resolved));
    } finally {
      clearTimeout(timeoutId);
      if (abortListener && externalSignal) {
        externalSignal.removeEventListener('abort', abortListener);
      }
    }
  }

  throw new Error('Request retry attempts exhausted');
}

/**
 * Resolve a generated API request with a timeout. Cancels the underlying request when the timeout fires.
 */
export async function cancellableWithTimeout<T>(
  factory: () => Promise<T>,
  timeoutMs: number = DEFAULT_REQUEST_TIMEOUT_MS
): Promise<T> {
  try {
    getHttpClientBase();
  } catch {
    // Fall through and let the request surface its own transport error.
  }
  const request = factory();
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => {
      try {
        // Some generated clients expose a cancel method; ignore failures if absent.
        (request as { cancel?: () => void })?.cancel?.();
      } catch {
        // ignore cancel errors
      }
      reject(new Error('timeout'));
    }, timeoutMs);

    request
      .then((value) => {
        clearTimeout(timer);
        resolve(value);
      })
      .catch((error) => {
        clearTimeout(timer);
        reject(error);
      });
  });
}

type ResilientOptions = {
  timeoutMs?: number;
  label?: string;
  onError?: (error: unknown) => void;
};

/**
 * Execute an API request and fall back to a default value if it fails or times out.
 */
export async function callOrFallback<T, F = T>(
  factory: () => CancelablePromise<T>,
  fallback: F,
  options: ResilientOptions = {}
): Promise<T | F> {
  const { timeoutMs = DEFAULT_REQUEST_TIMEOUT_MS, label, onError } = options;
  try {
    return await cancellableWithTimeout(factory, timeoutMs);
  } catch (error) {
    if (onError) {
      onError(error);
    } else {
      const context = label ? `${label} request failed` : 'API request failed';
      console.warn(context, error);
    }
    return fallback;
  }
}
