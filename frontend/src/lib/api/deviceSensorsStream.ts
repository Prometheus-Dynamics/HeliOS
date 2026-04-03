import { browser } from '$app/environment';
import { getHttpClientBase } from '$lib/api/client';
import { buildWsUrl, connectWebSocketWithFallback, sendJson, type ManagedWebSocket } from '$lib/api/core/ws';

export type DeviceSensorKind = 'imu' | 'power' | 'firmware';

export type FirmwareUpdatePayload = {
  device_id: string;
  firmware: string;
  status: string;
  active?: string | null;
  error?: string | null;
  progress_pct?: number | null;
  detail?: string | null;
  timestamp_ms?: number;
};

export type DeviceSensorsStreamHandlers = {
  onImu?: (payload: unknown) => void;
  onPower?: (payload: unknown) => void;
  onFirmware?: (payload: FirmwareUpdatePayload) => void;
  onError?: (message: string) => void;
  onOpen?: () => void;
  onClose?: () => void;
};

type DeviceSensorsSubscriber = {
  token: symbol;
  kinds: DeviceSensorKind[];
  intervalMs: number;
  handlers: DeviceSensorsStreamHandlers;
};

type SharedDeviceSensorsSocket = {
  connection: ManagedWebSocket;
  subscribers: Map<symbol, DeviceSensorsSubscriber>;
  connected: boolean;
  activeKindsKey: string;
  activeIntervalMs: number;
};

let sharedDeviceSensorsSocket: SharedDeviceSensorsSocket | null = null;

function isFirmwareUpdatePayload(value: unknown): value is FirmwareUpdatePayload {
  if (!value || typeof value !== 'object') return false;
  const record = value as Record<string, unknown>;
  return typeof record.device_id === 'string' && typeof record.firmware === 'string' && typeof record.status === 'string';
}

export function connectDeviceSensorsStream(
  kinds: DeviceSensorKind[],
  intervalMs: number,
  handlers: DeviceSensorsStreamHandlers
): () => void {
  if (!browser) return () => {};

  const subscriber: DeviceSensorsSubscriber = {
    token: Symbol('device-sensors-subscriber'),
    kinds: normalizeKinds(kinds),
    intervalMs: normalizeIntervalMs(intervalMs),
    handlers
  };
  if (subscriber.kinds.length === 0) return () => {};

  const shared = getOrCreateSharedDeviceSensorsSocket();
  if (!shared) {
    handlers.onError?.('Unable to open sensors socket');
    return () => {};
  }

  shared.subscribers.set(subscriber.token, subscriber);
  syncSharedSubscription(shared);

  if (shared.connected) {
    queueMicrotask(() => {
      if (shared.subscribers.has(subscriber.token)) {
        subscriber.handlers.onOpen?.();
      }
    });
  }

  return () => {
    if (!shared.subscribers.delete(subscriber.token)) return;
    if (shared.subscribers.size === 0) {
      closeSharedDeviceSensorsSocket(shared);
      return;
    }
    syncSharedSubscription(shared);
  };
}

function getOrCreateSharedDeviceSensorsSocket(): SharedDeviceSensorsSocket | null {
  if (sharedDeviceSensorsSocket) return sharedDeviceSensorsSocket;

  const shared: SharedDeviceSensorsSocket = {
    connection: null as unknown as ManagedWebSocket,
    subscribers: new Map(),
    connected: false,
    activeKindsKey: '',
    activeIntervalMs: 100
  };

  const connection = connectWebSocketWithFallback(
    buildSensorsSocketUrl(),
    {
      onOpen: () => {
        shared.connected = true;
        for (const subscriber of shared.subscribers.values()) {
          subscriber.handlers.onOpen?.();
        }
        syncSharedSubscription(shared);
      },
      onMessage: (event) => {
        const data = event.data;
        if (typeof data !== 'string') return;

        let parsed: Record<string, unknown>;
        try {
          parsed = JSON.parse(data) as Record<string, unknown>;
        } catch {
          return;
        }

        if (parsed?.status === 'sensors_stream_unavailable') {
          const message = typeof parsed.reason === 'string' ? parsed.reason : 'Sensors stream unavailable';
          for (const subscriber of shared.subscribers.values()) {
            subscriber.handlers.onError?.(message);
          }
          return;
        }

        for (const subscriber of shared.subscribers.values()) {
          if (parsed.imu != null && subscriber.kinds.includes('imu')) {
            subscriber.handlers.onImu?.(parsed.imu);
          }
          if (parsed.power != null && subscriber.kinds.includes('power')) {
            subscriber.handlers.onPower?.(parsed.power);
          }
          if (subscriber.kinds.includes('firmware') && isFirmwareUpdatePayload(parsed.firmware)) {
            subscriber.handlers.onFirmware?.(parsed.firmware);
          }
        }
      },
      onError: (message) => {
        const errorMessage = message || 'Sensors stream connection failed';
        for (const subscriber of shared.subscribers.values()) {
          subscriber.handlers.onError?.(errorMessage);
        }
      },
      onClose: () => {
        shared.connected = false;
        shared.activeKindsKey = '';
        if (sharedDeviceSensorsSocket === shared) {
          sharedDeviceSensorsSocket = null;
        }
        for (const subscriber of shared.subscribers.values()) {
          subscriber.handlers.onClose?.();
        }
      }
    },
    { errorMessage: 'Sensors stream connection failed' }
  );

  if (!connection) {
    return null;
  }

  shared.connection = connection;
  sharedDeviceSensorsSocket = shared;
  return shared;
}

function syncSharedSubscription(shared: SharedDeviceSensorsSocket): void {
  if (!shared.connected || !shared.connection.ready()) return;

  const desiredKinds = new Set<DeviceSensorKind>();
  let desiredIntervalMs = Number.POSITIVE_INFINITY;
  for (const subscriber of shared.subscribers.values()) {
    for (const kind of subscriber.kinds) {
      desiredKinds.add(kind);
    }
    desiredIntervalMs = Math.min(desiredIntervalMs, subscriber.intervalMs);
  }

  if (desiredKinds.size === 0) {
    sendSharedMessage(shared.connection, { op: 'unsubscribe' });
    shared.activeKindsKey = '';
    return;
  }

  const normalizedKinds = normalizeKinds([...desiredKinds]);
  const nextKindsKey = normalizedKinds.join(',');
  const nextIntervalMs = normalizeIntervalMs(Number.isFinite(desiredIntervalMs) ? desiredIntervalMs : 100);
  if (shared.activeKindsKey === nextKindsKey && shared.activeIntervalMs === nextIntervalMs) {
    return;
  }

  sendSharedMessage(shared.connection, { op: 'unsubscribe' });
  sendSharedMessage(shared.connection, {
    op: 'subscribe',
    kinds: normalizedKinds,
    interval_ms: nextIntervalMs
  });
  shared.activeKindsKey = nextKindsKey;
  shared.activeIntervalMs = nextIntervalMs;
}

function sendSharedMessage(connection: ManagedWebSocket, message: Record<string, unknown>): void {
  sendJson(connection, message);
}

function closeSharedDeviceSensorsSocket(shared: SharedDeviceSensorsSocket): void {
  if (sharedDeviceSensorsSocket === shared) {
    sharedDeviceSensorsSocket = null;
  }
  shared.connected = false;
  shared.activeKindsKey = '';
  sendJson(shared.connection, { op: 'unsubscribe' });
  shared.connection.close();
}

function buildSensorsSocketUrl(): string {
  const base = getHttpClientBase();
  const origin = browser ? globalThis.location?.origin?.trim() || '' : '';
  return buildWsUrl({
    base,
    path: ['v1', 'ws', 'sensors'],
    allowRelative: true,
    fallbackOrigin: origin
  });
}

function normalizeKinds(kinds: DeviceSensorKind[]): DeviceSensorKind[] {
  return [...new Set(kinds)].sort();
}

function normalizeIntervalMs(intervalMs: number): number {
  return Math.max(50, Math.min(10_000, Math.floor(intervalMs)));
}
