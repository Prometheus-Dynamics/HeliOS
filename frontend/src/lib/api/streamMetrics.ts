import { buildWsUrlFromHttpBase, canUseWebSockets, connectWebSocketWithFallback, type ManagedWebSocket } from '$lib/api/core/ws';
import type { StreamMetrics } from '$lib/api/client';

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
  onOpen?: () => void;
};

type MetricsSubscriber = {
  token: symbol;
  handlers: StreamMetricsHandlers;
  intervalMs: number;
  lastDeliveredAtMs: number | null;
};

type SharedMetricsConnection = {
  streamId: string;
  subscribers: Map<symbol, MetricsSubscriber>;
  connection: ManagedWebSocket | null;
  connected: boolean;
  currentIntervalMs: number;
  latestEvent: StreamMetricsEvent | null;
  generation: number;
};

const DEFAULT_STREAM_METRICS_INTERVAL_MS = 1_000;
const MIN_STREAM_METRICS_INTERVAL_MS = 250;
const MAX_STREAM_METRICS_INTERVAL_MS = 10_000;

const sharedMetricsConnections = new Map<string, SharedMetricsConnection>();

export function buildStreamMetricsUrl(streamId: string, intervalMs?: number): string {
  const intervalParam = normalizeMetricsInterval(intervalMs);
  const baseUrl = buildWsUrlFromHttpBase(['v1', 'ws', 'streams', encodeURIComponent(streamId), 'metrics']);
  try {
    const parsed = new URL(baseUrl);
    parsed.search = '';
    if (intervalParam > 0) {
      parsed.searchParams.set('interval_ms', intervalParam.toString());
    }
    return parsed.toString();
  } catch {
    const query = intervalParam > 0 ? `?interval_ms=${intervalParam}` : '';
    return `${baseUrl}${query}`;
  }
}

export function openStreamMetricsSocket(
  streamId: string,
  handlers: StreamMetricsHandlers,
  options: { intervalMs?: number } = {}
): () => void {
  const normalizedStreamId = String(streamId ?? '').trim();
  if (!normalizedStreamId) {
    handlers.onError?.({ stream_id: null, error: 'Missing stream id for metrics socket' });
    return () => {};
  }
  if (!canUseWebSockets()) {
    handlers.onError?.({ stream_id: normalizedStreamId, error: 'WebSocket not supported in this environment' });
    return () => {};
  }

  const subscriber: MetricsSubscriber = {
    token: Symbol(`stream-metrics:${normalizedStreamId}`),
    handlers,
    intervalMs: normalizeMetricsInterval(options.intervalMs),
    lastDeliveredAtMs: null
  };

  const shared = getOrCreateSharedMetricsConnection(normalizedStreamId);
  shared.subscribers.set(subscriber.token, subscriber);
  ensureSharedMetricsConnection(shared);

  if (shared.connected) {
    queueMicrotask(() => {
      if (!shared.subscribers.has(subscriber.token)) return;
      subscriber.handlers.onOpen?.();
      replayLatestMetricsToSubscriber(shared, subscriber);
    });
  }

  return () => {
    const current = sharedMetricsConnections.get(normalizedStreamId);
    if (current !== shared) return;
    current.subscribers.delete(subscriber.token);
    if (current.subscribers.size === 0) {
      sharedMetricsConnections.delete(normalizedStreamId);
      current.connected = false;
      current.connection?.close();
      current.connection = null;
      return;
    }
    ensureSharedMetricsConnection(current);
  };
}

function getOrCreateSharedMetricsConnection(streamId: string): SharedMetricsConnection {
  const existing = sharedMetricsConnections.get(streamId);
  if (existing) return existing;

  const shared: SharedMetricsConnection = {
    streamId,
    subscribers: new Map(),
    connection: null,
    connected: false,
    currentIntervalMs: 0,
    latestEvent: null,
    generation: 0
  };
  sharedMetricsConnections.set(streamId, shared);
  return shared;
}

function ensureSharedMetricsConnection(shared: SharedMetricsConnection): void {
  const desiredIntervalMs = aggregateMetricsInterval(shared);
  const shouldReuse = shared.connection && shared.currentIntervalMs === desiredIntervalMs;
  if (shouldReuse) {
    return;
  }

  const previous = shared.connection;
  const generation = shared.generation + 1;
  const url = buildStreamMetricsUrl(shared.streamId, desiredIntervalMs);
  const connection = connectWebSocketWithFallback(
    url,
    {
      onOpen: () => {
        if (shared.generation !== generation) return;
        shared.connected = true;
        for (const subscriber of shared.subscribers.values()) {
          subscriber.handlers.onOpen?.();
          replayLatestMetricsToSubscriber(shared, subscriber);
        }
      },
      onMessage: (event) => {
        if (shared.generation !== generation) return;
        const payload = parsePayload(event.data);
        if (payload?.type === 'metrics' && payload.event) {
          shared.latestEvent = payload.event;
          for (const subscriber of shared.subscribers.values()) {
            deliverMetricsToSubscriber(subscriber, payload.event);
          }
          return;
        }
        if (payload?.type === 'error' && payload.error) {
          emitMetricsError(shared, payload.error);
          return;
        }
        emitMetricsError(shared, {
          stream_id: shared.streamId,
          error: 'Received invalid stream metrics payload'
        });
      },
      onError: (message) => {
        if (shared.generation !== generation) return;
        emitMetricsError(shared, { stream_id: shared.streamId, error: message || 'Metrics stream error' });
      },
      onClose: (event) => {
        if (shared.generation !== generation) return;
        shared.connected = false;
        shared.connection = null;
        for (const subscriber of shared.subscribers.values()) {
          subscriber.handlers.onClose?.({ expected: false, code: event.code, reason: event.reason });
        }
        if (shared.subscribers.size === 0) {
          sharedMetricsConnections.delete(shared.streamId);
        }
      }
    },
    { errorMessage: 'Metrics stream error' }
  );

  if (!connection) {
    if (previous) {
      shared.connection = previous;
      shared.connected = previous.ready();
      return;
    }
    shared.connected = false;
    shared.connection = null;
    emitMetricsError(shared, { stream_id: shared.streamId, error: 'Unable to open stream metrics socket' });
    return;
  }

  shared.generation = generation;
  shared.connection = connection;
  shared.currentIntervalMs = desiredIntervalMs;
  shared.connected = connection.ready();
  previous?.close();
}

function aggregateMetricsInterval(shared: SharedMetricsConnection): number {
  let intervalMs = DEFAULT_STREAM_METRICS_INTERVAL_MS;
  for (const subscriber of shared.subscribers.values()) {
    intervalMs = Math.min(intervalMs, subscriber.intervalMs);
  }
  return normalizeMetricsInterval(intervalMs);
}

function replayLatestMetricsToSubscriber(shared: SharedMetricsConnection, subscriber: MetricsSubscriber): void {
  if (!shared.latestEvent) return;
  deliverMetricsToSubscriber(subscriber, shared.latestEvent, true);
}

function deliverMetricsToSubscriber(subscriber: MetricsSubscriber, event: StreamMetricsEvent, replay = false): void {
  const timestampMs = normalizeEventTimestamp(event.timestamp_ms);
  if (!replay && subscriber.lastDeliveredAtMs != null && timestampMs - subscriber.lastDeliveredAtMs < subscriber.intervalMs) {
    return;
  }
  subscriber.lastDeliveredAtMs = timestampMs;
  subscriber.handlers.onMetrics?.(event);
}

function emitMetricsError(shared: SharedMetricsConnection, error: StreamMetricsError): void {
  for (const subscriber of shared.subscribers.values()) {
    subscriber.handlers.onError?.(error);
  }
}

function normalizeMetricsInterval(intervalMs?: number): number {
  if (typeof intervalMs !== 'number' || !Number.isFinite(intervalMs)) {
    return DEFAULT_STREAM_METRICS_INTERVAL_MS;
  }
  return Math.max(MIN_STREAM_METRICS_INTERVAL_MS, Math.min(MAX_STREAM_METRICS_INTERVAL_MS, Math.floor(intervalMs)));
}

function normalizeEventTimestamp(timestampMs?: number): number {
  return typeof timestampMs === 'number' && Number.isFinite(timestampMs) ? timestampMs : Date.now();
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
  } catch {
    return null;
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
