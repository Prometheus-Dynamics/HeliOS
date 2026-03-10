import { buildWsUrlFromHttpBase, canUseWebSockets, connectWebSocketWithFallback } from '$lib/api/core/ws';

export type DevicesUpdateReason =
  | 'usb'
  | 'streams'
  | 'api'
  | 'pipelines'
  | 'localization'
  | 'media'
  | 'imu'
  | 'device'
  | 'settings';

export type DevicesUpdatesServerEvent =
  | { type: 'ready'; interval_ms: number }
  | { type: 'update'; timestamp_ms: number; reasons: DevicesUpdateReason[] }
  | { type: 'error'; message: string };

type DevicesUpdatesHandlers = {
  onReady?: (intervalMs: number) => void;
  onUpdate?: (reasons: DevicesUpdateReason[], timestampMs: number) => void;
  onError?: (message: string) => void;
  onOpen?: () => void;
  onClose?: () => void;
};

type SharedDevicesUpdatesConnection = {
  connection: { close: () => void };
  handlers: Map<symbol, DevicesUpdatesHandlers>;
  isOpen: boolean;
  readyIntervalMs: number | null;
};

const sharedDevicesUpdatesConnections = new Map<number, SharedDevicesUpdatesConnection>();

export function connectDevicesUpdatesStream(
  handlers: DevicesUpdatesHandlers,
  options: { intervalMs?: number } = {}
): () => void {
  if (!canUseWebSockets()) {
    handlers.onError?.('WebSocket not supported in this environment');
    return () => {};
  }

  const intervalMs = normalizeIntervalMs(options.intervalMs ?? 2000);
  const shared = getOrCreateSharedConnection(intervalMs);
  if (!shared) {
    handlers.onError?.('Unable to open devices updates socket');
    return () => {};
  }

  const token = Symbol('devices-updates-handler');
  shared.handlers.set(token, handlers);

  if (shared.isOpen) {
    queueMicrotask(() => {
      if (shared.handlers.has(token)) handlers.onOpen?.();
    });
  }
  if (shared.readyIntervalMs != null) {
    queueMicrotask(() => {
      if (shared.handlers.has(token)) handlers.onReady?.(shared.readyIntervalMs ?? intervalMs);
    });
  }

  return () => {
    shared.handlers.delete(token);
    if (shared.handlers.size === 0) {
      if (sharedDevicesUpdatesConnections.get(intervalMs) === shared) {
        sharedDevicesUpdatesConnections.delete(intervalMs);
      }
      shared.connection.close();
    }
  };
}

function buildDevicesSocketUrl(intervalMs: number): string {
  const interval = normalizeIntervalMs(intervalMs);
  const baseUrl = buildWsUrlFromHttpBase(['v1', 'ws', 'devices']);
  try {
    const parsed = new URL(baseUrl);
    parsed.searchParams.set('interval_ms', interval.toString());
    return parsed.toString();
  } catch {
    const params = new URLSearchParams({ interval_ms: String(interval) });
    return `${baseUrl}?${params.toString()}`;
  }
}

function getOrCreateSharedConnection(intervalMs: number): SharedDevicesUpdatesConnection | null {
  const existing = sharedDevicesUpdatesConnections.get(intervalMs);
  if (existing) return existing;

  const shared = {
    handlers: new Map<symbol, DevicesUpdatesHandlers>(),
    isOpen: false,
    readyIntervalMs: null
  } as SharedDevicesUpdatesConnection;
  const url = buildDevicesSocketUrl(intervalMs);

  const connection = connectWebSocketWithFallback(
    url,
    {
      onOpen: () => {
        shared.isOpen = true;
        for (const handler of shared.handlers.values()) {
          handler.onOpen?.();
        }
      },
      onMessage: (event) => {
        if (typeof event.data !== 'string') return;
        let parsed: DevicesUpdatesServerEvent | null = null;
        try {
          parsed = JSON.parse(event.data) as DevicesUpdatesServerEvent;
        } catch {
          return;
        }
        if (!parsed || typeof parsed !== 'object') return;
        if (parsed.type === 'ready') {
          shared.readyIntervalMs = typeof parsed.interval_ms === 'number' ? parsed.interval_ms : intervalMs;
          for (const handler of shared.handlers.values()) {
            handler.onReady?.(shared.readyIntervalMs);
          }
          return;
        }
        if (parsed.type === 'update') {
          const reasons = Array.isArray(parsed.reasons) ? parsed.reasons : [];
          const timestampMs = typeof parsed.timestamp_ms === 'number' ? parsed.timestamp_ms : Date.now();
          for (const handler of shared.handlers.values()) {
            handler.onUpdate?.(reasons, timestampMs);
          }
          return;
        }
        if (parsed.type === 'error') {
          const message = parsed.message || 'Devices updates stream error';
          for (const handler of shared.handlers.values()) {
            handler.onError?.(message);
          }
        }
      },
      onError: (message) => {
        const errorMessage = message || 'Devices updates stream connection failed';
        for (const handler of shared.handlers.values()) {
          handler.onError?.(errorMessage);
        }
      },
      onClose: () => {
        shared.isOpen = false;
        shared.readyIntervalMs = null;
        if (sharedDevicesUpdatesConnections.get(intervalMs) === shared) {
          sharedDevicesUpdatesConnections.delete(intervalMs);
        }
        for (const handler of shared.handlers.values()) {
          handler.onClose?.();
        }
      }
    },
    { errorMessage: 'Devices updates stream connection failed' }
  );

  if (!connection) {
    return null;
  }

  shared.connection = connection;
  sharedDevicesUpdatesConnections.set(intervalMs, shared);
  return shared;
}

function normalizeIntervalMs(intervalMs: number): number {
  return Math.max(250, Math.min(10000, Math.floor(intervalMs)));
}
