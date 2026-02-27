import { buildWsUrlFromHttpBase, canUseWebSockets, connectWebSocketWithFallback } from '$lib/api/wsClient';
import type { StreamMetrics } from '$lib/ts-bindings/http/client';

export type StreamMetricsEvent = {
  stream_id: string;
  metrics: StreamMetrics;
  timestamp_ms?: number;
};

export type StreamMetricsError = {
  stream_id?: string | null;
  error: string;
  code?: string | null;
  detail?: string | null;
  timestamp_ms?: number | null;
  source?: string | null;
  operation?: string | null;
};

type StreamMetricsHandlers = {
  onMetrics?: (event: StreamMetricsEvent) => void;
  onError?: (error: StreamMetricsError) => void;
  onClose?: (info?: { expected: boolean; code: number; reason: string }) => void;
};

export function buildStreamMetricsUrl(streamId: string, intervalMs?: number): string {
  const intervalParam = intervalMs && Number.isFinite(intervalMs) ? Math.max(0, Math.floor(intervalMs)) : null;
  const baseUrl = buildWsUrlFromHttpBase(['v1', 'ws', 'streams', encodeURIComponent(streamId), 'metrics']);
  try {
    const parsed = new URL(baseUrl);
    parsed.search = '';
    if (intervalParam && intervalParam > 0) {
      parsed.searchParams.set('interval_ms', intervalParam.toString());
    }
    return parsed.toString();
  } catch {
    const query = intervalParam && intervalParam > 0 ? `?interval_ms=${intervalParam}` : '';
    return `${baseUrl}${query}`;
  }
}

export function openStreamMetricsSocket(
  streamId: string,
  handlers: StreamMetricsHandlers,
  options: { intervalMs?: number } = {}
): () => void {
  if (!canUseWebSockets()) {
    handlers.onError?.({ stream_id: streamId, error: 'WebSocket not supported in this environment' });
    return () => {};
  }

  const url = buildStreamMetricsUrl(streamId, options.intervalMs);
  let closingRequested = false;
  const connection = connectWebSocketWithFallback(
    url,
    {
      onMessage: (event) => {
        const payload = parsePayload(event.data);
        if (payload?.type === 'metrics' && payload.event) {
          handlers.onMetrics?.(payload.event);
          return;
        }
        if (payload?.type === 'error' && payload.error) {
          handlers.onError?.(payload.error);
          return;
        }
      },
      onError: (message) => {
        handlers.onError?.({ stream_id: streamId, error: message || 'Metrics stream error' });
      },
      onClose: (event) => {
        handlers.onClose?.({ expected: closingRequested, code: event.code, reason: event.reason });
      }
    },
    { errorMessage: 'Metrics stream error' }
  );

  if (!connection) {
    handlers.onError?.({ stream_id: streamId, error: 'Unable to open stream metrics socket' });
    return () => {};
  }

  return () => {
    try {
      closingRequested = true;
      connection.close();
    } catch {
      // ignore
    }
  };
}

function parsePayload(data: unknown):
  | { type: 'metrics'; event: StreamMetricsEvent }
  | { type: 'error'; error: StreamMetricsError }
  | null {
  if (typeof data !== 'string') return null;
  try {
    const parsed = JSON.parse(data);
    if (parsed && typeof parsed === 'object') {
      if ('metrics' in parsed) {
        return { type: 'metrics', event: normalizeMetricsEvent(parsed) };
      }
      if ('error' in parsed) {
        return { type: 'error', error: normalizeError(parsed) };
      }
    }
  } catch (err) {
    console.warn('Failed to parse stream metrics payload', err);
  }
  return null;
}

function normalizeMetricsEvent(value: unknown): StreamMetricsEvent {
  const record = asRecord(value);
  return {
    stream_id: String(record['stream_id'] ?? ''),
    metrics: (record['metrics'] ?? {}) as StreamMetrics,
    timestamp_ms: typeof record['timestamp_ms'] === 'number' ? record['timestamp_ms'] : undefined
  };
}

function normalizeError(value: unknown): StreamMetricsError {
  const record = asRecord(value);
  return {
    stream_id: typeof record['stream_id'] === 'string' ? record['stream_id'] : null,
    error: String(record['error'] ?? 'Metrics stream error'),
    code: typeof record['code'] === 'string' ? record['code'] : null,
    detail: typeof record['detail'] === 'string' ? record['detail'] : null,
    timestamp_ms: typeof record['timestamp_ms'] === 'number' ? record['timestamp_ms'] : null,
    source: typeof record['source'] === 'string' ? record['source'] : null,
    operation: typeof record['operation'] === 'string' ? record['operation'] : null
  };
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' ? (value as Record<string, unknown>) : {};
}
