import { get } from 'svelte/store';
import { toaster } from '$lib';
import { pushNotification } from '$lib/ui/notifications';
import { extractError, extractErrorMetadata } from '$lib/api/errors';
import { connectionState, type ConnectionStatus } from '$lib/api/connection';

type ErrorPolicyOptions = {
  title?: string;
  context?: string;
  source?: string;
  operation?: string;
  error?: unknown;
  fallback?: string;
  description?: string;
  remediation?: string;
  retryable?: boolean;
  toast?: boolean;
  inline?: (message: string | null) => void;
};

const NETWORK_TOAST_THROTTLE_MS = 10_000;
let lastNetworkToastAt = 0;

export function buildErrorMessage(options: Omit<ErrorPolicyOptions, 'toast' | 'inline'>): string {
  const { context, error, fallback, description } = options;
  if (description && description.trim().length) {
    return description.trim();
  }
  const base = error ? extractError(error) : '';
  const message = base || fallback || 'Unexpected error';
  if (context && context.trim().length) {
    return `${context.trim()}. ${message}`;
  }
  return message;
}

function currentConnectionStatus(): ConnectionStatus | null {
  try {
    return get(connectionState).status;
  } catch {
    return null;
  }
}

function isNetworkFailure(error: unknown, message: string): boolean {
  if (error instanceof TypeError) {
    return true;
  }
  if (error && typeof error === 'object' && 'name' in error) {
    const name = String((error as { name?: unknown }).name ?? '').toLowerCase();
    if (name.includes('network') || name.includes('timeout')) {
      return true;
    }
  }
  const normalized = message.toLowerCase();
  return (
    normalized.includes('failed to fetch') ||
    normalized.includes('networkerror') ||
    normalized.includes('load failed') ||
    normalized.includes('connection refused') ||
    normalized.includes('request failed') ||
    normalized.includes('timed out') ||
    normalized.includes('timeout')
  );
}

export function reportError(options: ErrorPolicyOptions): string {
  const message = buildErrorMessage(options);
  const metadata = extractErrorMetadata(options.error);
  if (options.inline) {
    options.inline(message);
  }
  const isNetworkError = isNetworkFailure(options.error, message);
  const status = isNetworkError ? currentConnectionStatus() : null;
  const throttleMs = status === 'offline' ? NETWORK_TOAST_THROTTLE_MS * 2 : NETWORK_TOAST_THROTTLE_MS;
  const now = Date.now();
  const canToastNetwork = !isNetworkError || now - lastNetworkToastAt >= throttleMs;
  if (options.toast !== false && canToastNetwork) {
    if (isNetworkError) {
      lastNetworkToastAt = now;
    }
    toaster.error({
      title: options.title ?? options.context ?? 'Request failed',
      description: message
    });
  }
  pushNotification({
    title: options.title ?? options.context ?? 'Request failed',
    description: message,
    kind: 'error',
    source: options.source ?? metadata?.source ?? options.context,
    operation: options.operation ?? metadata?.operation ?? null,
    code: metadata?.code ?? null,
    requestId: metadata?.requestId ?? null,
    traceId: metadata?.traceId ?? null,
    retryable: options.retryable ?? metadata?.retryable ?? null,
    remediation: options.remediation ?? metadata?.remediation ?? null,
    occurredAt: metadata?.timestampMs ?? null,
    reportedBy: metadata?.reportedBy ?? null
  });
  return message;
}
