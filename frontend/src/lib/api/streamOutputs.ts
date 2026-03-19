import { buildWsUrlFromHttpBase, canUseWebSockets, connectWebSocketWithFallback, sendJson, type ManagedWebSocket } from '$lib/api/core/ws';

type UnknownRecord = Record<string, unknown>;

export type StreamOutputsListEvent = {
  outputs: Array<{ name: string; ty?: unknown; previewable: boolean }>;
  timestamp_ms?: number;
  request_id?: string | null;
};

export type StreamOutputSampleEvent = {
  port: string;
  value?: unknown;
  error?: string | null;
  timestamp_ms?: number;
};

export type StreamOutputsHandlers = {
  onOutputs?: (event: StreamOutputsListEvent) => void;
  onSample?: (event: StreamOutputSampleEvent) => void;
  onError?: (error: { error: string; request_id?: string | null }) => void;
  onClose?: (info?: { expected: boolean; code: number; reason: string }) => void;
  onOpen?: () => void;
};

export type StreamOutputsSocket = {
  ready: () => boolean;
  subscribe: (ports: string[], options?: { intervalMs?: number }) => void;
  close: () => void;
};

type OutputsSubscriber = {
  token: symbol;
  handlers: StreamOutputsHandlers;
  ports: string[];
  intervalMs: number;
  portsIntervalMs: number;
  lastDeliveredAtByPort: Map<string, number>;
};

type SharedOutputsConnection = {
  streamId: string;
  subscribers: Map<symbol, OutputsSubscriber>;
  connection: ManagedWebSocket | null;
  connected: boolean;
  currentPortsIntervalMs: number;
  latestOutputs: StreamOutputsListEvent | null;
  latestSamplesByPort: Map<string, StreamOutputSampleEvent>;
  generation: number;
};

const DEFAULT_OUTPUT_SAMPLE_INTERVAL_MS = 250;
const MIN_OUTPUT_SAMPLE_INTERVAL_MS = 100;
const MAX_OUTPUT_SAMPLE_INTERVAL_MS = 5_000;

const DEFAULT_OUTPUT_PORTS_INTERVAL_MS = 2_000;
const MIN_OUTPUT_PORTS_INTERVAL_MS = 500;
const MAX_OUTPUT_PORTS_INTERVAL_MS = 30_000;

const sharedOutputsConnections = new Map<string, SharedOutputsConnection>();

const asRecord = (value: unknown): UnknownRecord | null =>
  value && typeof value === 'object' ? (value as UnknownRecord) : null;

export function buildStreamOutputsUrl(streamId: string, options: { portsIntervalMs?: number } = {}): string {
  const baseUrl = buildWsUrlFromHttpBase(['v1', 'ws', 'streams', encodeURIComponent(streamId), 'outputs']);
  const portsIntervalMs = normalizePortsInterval(options.portsIntervalMs);
  try {
    const parsed = new URL(baseUrl);
    parsed.search = '';
    if (portsIntervalMs > 0) {
      parsed.searchParams.set('ports_interval_ms', portsIntervalMs.toString());
    }
    return parsed.toString();
  } catch {
    const query = portsIntervalMs > 0 ? `?ports_interval_ms=${portsIntervalMs}` : '';
    return `${baseUrl}${query}`;
  }
}

export function openStreamOutputsSocket(
  streamId: string,
  handlers: StreamOutputsHandlers,
  options: { portsIntervalMs?: number } = {}
): StreamOutputsSocket {
  const normalizedStreamId = String(streamId ?? '').trim();
  if (!normalizedStreamId) {
    handlers.onError?.({ error: 'Missing stream id for outputs socket' });
    return { ready: () => false, subscribe: () => {}, close: () => {} };
  }
  if (!canUseWebSockets()) {
    handlers.onError?.({ error: 'WebSocket not supported in this environment' });
    return { ready: () => false, subscribe: () => {}, close: () => {} };
  }

  const subscriber: OutputsSubscriber = {
    token: Symbol(`stream-outputs:${normalizedStreamId}`),
    handlers,
    ports: [],
    intervalMs: DEFAULT_OUTPUT_SAMPLE_INTERVAL_MS,
    portsIntervalMs: normalizePortsInterval(options.portsIntervalMs),
    lastDeliveredAtByPort: new Map()
  };

  const shared = getOrCreateSharedOutputsConnection(normalizedStreamId);
  shared.subscribers.set(subscriber.token, subscriber);
  ensureSharedOutputsConnection(shared);

  if (shared.connected) {
    queueMicrotask(() => {
      if (!shared.subscribers.has(subscriber.token)) return;
      subscriber.handlers.onOpen?.();
      replayOutputsToSubscriber(shared, subscriber);
    });
  }

  return {
    ready: () => shared.connected && (shared.connection?.ready() ?? false),
    subscribe: (ports: string[], subscribeOptions: { intervalMs?: number } = {}) => {
      subscriber.ports = normalizePorts(ports);
      subscriber.intervalMs = normalizeSampleInterval(subscribeOptions.intervalMs);
      subscriber.lastDeliveredAtByPort.clear();
      if (typeof options.portsIntervalMs === 'number' && Number.isFinite(options.portsIntervalMs)) {
        subscriber.portsIntervalMs = normalizePortsInterval(options.portsIntervalMs);
      }

      const current = sharedOutputsConnections.get(normalizedStreamId);
      if (current !== shared) return;
      ensureSharedOutputsConnection(current);
      syncSharedOutputsSubscription(current);
      replayOutputsToSubscriber(current, subscriber);
    },
    close: () => {
      const current = sharedOutputsConnections.get(normalizedStreamId);
      if (current !== shared) return;
      current.subscribers.delete(subscriber.token);
      if (current.subscribers.size === 0) {
        sharedOutputsConnections.delete(normalizedStreamId);
        current.connected = false;
        current.connection?.close();
        current.connection = null;
        return;
      }
      ensureSharedOutputsConnection(current);
      syncSharedOutputsSubscription(current);
    }
  };
}

function getOrCreateSharedOutputsConnection(streamId: string): SharedOutputsConnection {
  const existing = sharedOutputsConnections.get(streamId);
  if (existing) return existing;

  const shared: SharedOutputsConnection = {
    streamId,
    subscribers: new Map(),
    connection: null,
    connected: false,
    currentPortsIntervalMs: 0,
    latestOutputs: null,
    latestSamplesByPort: new Map(),
    generation: 0
  };
  sharedOutputsConnections.set(streamId, shared);
  return shared;
}

function ensureSharedOutputsConnection(shared: SharedOutputsConnection): void {
  const desiredPortsIntervalMs = aggregatePortsInterval(shared);
  const shouldReuse = shared.connection && shared.currentPortsIntervalMs === desiredPortsIntervalMs;
  if (shouldReuse) {
    return;
  }

  const previous = shared.connection;
  const generation = shared.generation + 1;
  const connection = connectWebSocketWithFallback(
    buildStreamOutputsUrl(shared.streamId, { portsIntervalMs: desiredPortsIntervalMs }),
    {
      onOpen: () => {
        if (shared.generation !== generation) return;
        shared.connected = true;
        for (const subscriber of shared.subscribers.values()) {
          subscriber.handlers.onOpen?.();
          replayOutputsToSubscriber(shared, subscriber);
        }
        syncSharedOutputsSubscription(shared);
      },
      onMessage: (event) => {
        if (shared.generation !== generation) return;
        const payload = parsePayload(event.data);
        if (!payload) {
          emitOutputsError(shared, { error: 'Received invalid stream outputs payload' });
          return;
        }
        if (payload.type === 'outputs') {
          shared.latestOutputs = payload.event;
          for (const subscriber of shared.subscribers.values()) {
            subscriber.handlers.onOutputs?.(payload.event);
          }
          return;
        }
        if (payload.type === 'sample') {
          shared.latestSamplesByPort.set(payload.event.port, payload.event);
          for (const subscriber of shared.subscribers.values()) {
            deliverSampleToSubscriber(subscriber, payload.event);
          }
          return;
        }
        if (payload.type === 'ack') {
          return;
        }
        emitOutputsError(shared, payload.error);
      },
      onError: (message) => {
        if (shared.generation !== generation) return;
        emitOutputsError(shared, { error: message || 'Outputs stream error' });
      },
      onClose: (event) => {
        if (shared.generation !== generation) return;
        shared.connected = false;
        shared.connection = null;
        for (const subscriber of shared.subscribers.values()) {
          subscriber.handlers.onClose?.({ expected: false, code: event.code, reason: event.reason });
        }
        if (shared.subscribers.size === 0) {
          sharedOutputsConnections.delete(shared.streamId);
        }
      }
    },
    { errorMessage: 'Outputs stream error' }
  );

  if (!connection) {
    if (previous) {
      shared.connection = previous;
      shared.connected = previous.ready();
      return;
    }
    shared.connected = false;
    shared.connection = null;
    emitOutputsError(shared, { error: 'Unable to open stream outputs socket' });
    return;
  }

  shared.generation = generation;
  shared.connection = connection;
  shared.currentPortsIntervalMs = desiredPortsIntervalMs;
  shared.connected = connection.ready();
  previous?.close();
}

function syncSharedOutputsSubscription(shared: SharedOutputsConnection): void {
  if (!shared.connected || !shared.connection?.ready()) {
    return;
  }
  const payload = aggregateSharedSubscription(shared);
  sendJson(shared.connection, {
    type: 'subscribe',
    ports: payload.ports,
    interval_ms: payload.intervalMs
  });
}

function aggregateSharedSubscription(shared: SharedOutputsConnection): { ports: string[]; intervalMs: number } {
  const ports = new Map<string, number>();
  for (const subscriber of shared.subscribers.values()) {
    for (const port of subscriber.ports) {
      const current = ports.get(port);
      if (current == null || subscriber.intervalMs < current) {
        ports.set(port, subscriber.intervalMs);
      }
    }
  }

  let intervalMs = DEFAULT_OUTPUT_SAMPLE_INTERVAL_MS;
  for (const value of ports.values()) {
    intervalMs = Math.min(intervalMs, value);
  }

  return {
    ports: Array.from(ports.keys()).sort(),
    intervalMs: normalizeSampleInterval(intervalMs)
  };
}

function aggregatePortsInterval(shared: SharedOutputsConnection): number {
  let intervalMs = DEFAULT_OUTPUT_PORTS_INTERVAL_MS;
  for (const subscriber of shared.subscribers.values()) {
    intervalMs = Math.min(intervalMs, subscriber.portsIntervalMs);
  }
  return normalizePortsInterval(intervalMs);
}

function replayOutputsToSubscriber(shared: SharedOutputsConnection, subscriber: OutputsSubscriber): void {
  if (shared.latestOutputs) {
    subscriber.handlers.onOutputs?.(shared.latestOutputs);
  }
  for (const port of subscriber.ports) {
    const sample = shared.latestSamplesByPort.get(port);
    if (sample) {
      deliverSampleToSubscriber(subscriber, sample, true);
    }
  }
}

function deliverSampleToSubscriber(subscriber: OutputsSubscriber, event: StreamOutputSampleEvent, replay = false): void {
  if (!subscriber.ports.includes(event.port)) {
    return;
  }
  const timestampMs = normalizeEventTimestamp(event.timestamp_ms);
  const previous = subscriber.lastDeliveredAtByPort.get(event.port) ?? null;
  if (!replay && previous != null && timestampMs - previous < subscriber.intervalMs) {
    return;
  }
  subscriber.lastDeliveredAtByPort.set(event.port, timestampMs);
  subscriber.handlers.onSample?.(event);
}

function emitOutputsError(shared: SharedOutputsConnection, error: { error: string; request_id?: string | null }): void {
  for (const subscriber of shared.subscribers.values()) {
    subscriber.handlers.onError?.(error);
  }
}

function normalizePorts(ports: string[]): string[] {
  return (Array.isArray(ports) ? ports : [])
    .map((value) => String(value ?? '').trim())
    .filter((value) => value.length > 0)
    .sort()
    .filter((value, index, list) => list.indexOf(value) === index);
}

function normalizeSampleInterval(intervalMs?: number): number {
  if (typeof intervalMs !== 'number' || !Number.isFinite(intervalMs)) {
    return DEFAULT_OUTPUT_SAMPLE_INTERVAL_MS;
  }
  return Math.max(MIN_OUTPUT_SAMPLE_INTERVAL_MS, Math.min(MAX_OUTPUT_SAMPLE_INTERVAL_MS, Math.floor(intervalMs)));
}

function normalizePortsInterval(intervalMs?: number): number {
  if (typeof intervalMs !== 'number' || !Number.isFinite(intervalMs)) {
    return DEFAULT_OUTPUT_PORTS_INTERVAL_MS;
  }
  return Math.max(MIN_OUTPUT_PORTS_INTERVAL_MS, Math.min(MAX_OUTPUT_PORTS_INTERVAL_MS, Math.floor(intervalMs)));
}

function normalizeEventTimestamp(timestampMs?: number): number {
  return typeof timestampMs === 'number' && Number.isFinite(timestampMs) ? timestampMs : Date.now();
}

function parsePayload(data: unknown):
  | { type: 'outputs'; event: StreamOutputsListEvent }
  | { type: 'sample'; event: StreamOutputSampleEvent }
  | { type: 'ack'; request_id?: string | null }
  | { type: 'error'; error: { error: string; request_id?: string | null } }
  | null {
  if (typeof data !== 'string') return null;
  try {
    const parsed = JSON.parse(data);
    const parsedRecord = asRecord(parsed);
    if (!parsedRecord) return null;
    if ('outputs' in parsedRecord) {
      return { type: 'outputs', event: normalizeOutputs(parsed) };
    }
    if ('port' in parsedRecord && ('value' in parsedRecord || 'error' in parsedRecord)) {
      return { type: 'sample', event: normalizeSample(parsed) };
    }
    if (parsedRecord.type === 'ack') {
      return {
        type: 'ack',
        request_id: typeof parsedRecord.request_id === 'string' ? parsedRecord.request_id : null
      };
    }
    if (parsedRecord.type === 'error' && 'error' in parsedRecord) {
      return {
        type: 'error',
        error: {
          error: String(parsedRecord.error ?? 'Unknown error'),
          request_id: typeof parsedRecord.request_id === 'string' ? parsedRecord.request_id : null
        }
      };
    }
  } catch {
    return null;
  }
  return null;
}

function normalizeOutputs(value: unknown): StreamOutputsListEvent {
  const record = asRecord(value);
  const outputList = Array.isArray(record?.outputs) ? record.outputs : [];
  return {
    outputs: outputList
      .map((entry) => {
        const output = asRecord(entry);
        if (!output) return null;
        const name = typeof output.name === 'string' ? output.name.trim() : '';
        if (!name) return null;
        return { name, ty: output.ty, previewable: Boolean(output.previewable) };
      })
      .filter((entry): entry is { name: string; ty: unknown; previewable: boolean } => entry !== null),
    timestamp_ms: typeof record?.timestamp_ms === 'number' ? record.timestamp_ms : undefined,
    request_id: typeof record?.request_id === 'string' ? record.request_id : null
  };
}

function normalizeSample(value: unknown): StreamOutputSampleEvent {
  const record = asRecord(value);
  return {
    port: String(record?.port ?? ''),
    value: record && 'value' in record ? record.value : undefined,
    error: typeof record?.error === 'string' ? record.error : null,
    timestamp_ms: typeof record?.timestamp_ms === 'number' ? record.timestamp_ms : undefined
  };
}
