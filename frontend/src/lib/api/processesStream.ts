import { buildWsUrlFromHttpBase, canUseWebSockets, connectWebSocketWithFallback, type ManagedWebSocket } from '$lib/api/core/ws';

export type ProcessSample = {
  pid: number;
  name: string;
  cpu_percent: number;
  memory_bytes: number;
  virtual_memory_bytes: number;
  status?: string | null;
  cmd?: string[];
};

export type ProcessesSnapshot = {
  timestamp_ms: number;
  total_memory_bytes: number;
  used_memory_bytes: number;
  processes: ProcessSample[];
};

export type ProcessesServerEvent =
  | { type: 'ready'; interval_ms: number }
  | { type: 'snapshot'; snapshot: ProcessesSnapshot }
  | { type: 'error'; message: string };

type ProcessesHandlers = {
  onReady?: (intervalMs: number) => void;
  onSnapshot?: (snapshot: ProcessesSnapshot) => void;
  onError?: (message: string) => void;
  onOpen?: () => void;
  onClose?: () => void;
};

type SharedProcessesConnection = {
  connection: ManagedWebSocket;
  subscribers: Map<symbol, ProcessesHandlers>;
  connected: boolean;
  readyIntervalMs: number | null;
};

const sharedProcessesConnections = new Map<string, SharedProcessesConnection>();

export function connectProcessesStream(
  handlers: ProcessesHandlers,
  options: { intervalMs?: number; limit?: number } = {}
): () => void {
  if (!canUseWebSockets()) {
    handlers.onError?.('WebSocket not supported in this environment');
    return () => {};
  }

  const intervalMs = normalizeIntervalMs(options.intervalMs ?? 1000);
  const limit = normalizeLimit(options.limit ?? 200);
  const key = `${intervalMs}:${limit}`;
  const shared = getOrCreateSharedConnection(key, intervalMs, limit);
  if (!shared) {
    handlers.onError?.('Unable to open processes socket');
    return () => {};
  }

  const token = Symbol('processes-stream-handler');
  shared.subscribers.set(token, handlers);

  if (shared.connected) {
    queueMicrotask(() => {
      if (shared.subscribers.has(token)) handlers.onOpen?.();
    });
  }
  if (shared.readyIntervalMs != null) {
    queueMicrotask(() => {
      if (shared.subscribers.has(token)) handlers.onReady?.(shared.readyIntervalMs ?? intervalMs);
    });
  }

  return () => {
    shared.subscribers.delete(token);
    if (shared.subscribers.size === 0) {
      if (sharedProcessesConnections.get(key) === shared) {
        sharedProcessesConnections.delete(key);
      }
      shared.connection.close();
    }
  };
}

function getOrCreateSharedConnection(key: string, intervalMs: number, limit: number): SharedProcessesConnection | null {
  const existing = sharedProcessesConnections.get(key);
  if (existing) return existing;

  const shared: SharedProcessesConnection = {
    connection: null as unknown as ManagedWebSocket,
    subscribers: new Map(),
    connected: false,
    readyIntervalMs: null
  };

  const connection = connectWebSocketWithFallback(
    buildProcessesSocketUrl(intervalMs, limit),
    {
      onOpen: () => {
        shared.connected = true;
        for (const subscriber of shared.subscribers.values()) {
          subscriber.onOpen?.();
        }
      },
      onMessage: (event) => {
        if (typeof event.data !== 'string') return;
        let parsed: ProcessesServerEvent | null = null;
        try {
          parsed = JSON.parse(event.data) as ProcessesServerEvent;
        } catch {
          return;
        }
        if (!parsed || typeof parsed !== 'object') return;
        if (parsed.type === 'ready') {
          shared.readyIntervalMs = typeof parsed.interval_ms === 'number' ? parsed.interval_ms : intervalMs;
          for (const subscriber of shared.subscribers.values()) {
            subscriber.onReady?.(shared.readyIntervalMs);
          }
          return;
        }
        if (parsed.type === 'snapshot' && parsed.snapshot) {
          for (const subscriber of shared.subscribers.values()) {
            subscriber.onSnapshot?.(parsed.snapshot);
          }
          return;
        }
        if (parsed.type === 'error') {
          const message = parsed.message || 'Processes stream error';
          for (const subscriber of shared.subscribers.values()) {
            subscriber.onError?.(message);
          }
        }
      },
      onError: (message) => {
        const errorMessage = message || 'Processes stream connection failed';
        for (const subscriber of shared.subscribers.values()) {
          subscriber.onError?.(errorMessage);
        }
      },
      onClose: () => {
        shared.connected = false;
        shared.readyIntervalMs = null;
        if (sharedProcessesConnections.get(key) === shared) {
          sharedProcessesConnections.delete(key);
        }
        for (const subscriber of shared.subscribers.values()) {
          subscriber.onClose?.();
        }
      }
    },
    { errorMessage: 'Processes stream connection failed' }
  );

  if (!connection) {
    return null;
  }

  shared.connection = connection;
  sharedProcessesConnections.set(key, shared);
  return shared;
}

function buildProcessesSocketUrl(intervalMs: number, limit: number): string {
  const interval = normalizeIntervalMs(intervalMs);
  const cappedLimit = normalizeLimit(limit);
  const baseUrl = buildWsUrlFromHttpBase(['v1', 'ws', 'processes']);
  try {
    const parsed = new URL(baseUrl);
    parsed.searchParams.set('interval_ms', interval.toString());
    parsed.searchParams.set('limit', cappedLimit.toString());
    return parsed.toString();
  } catch {
    const params = new URLSearchParams({ interval_ms: String(interval), limit: String(cappedLimit) });
    return `${baseUrl}?${params.toString()}`;
  }
}

function normalizeIntervalMs(intervalMs: number): number {
  return Math.max(250, Math.min(10000, Math.floor(intervalMs)));
}

function normalizeLimit(limit: number): number {
  return Math.max(10, Math.min(2000, Math.floor(limit)));
}
