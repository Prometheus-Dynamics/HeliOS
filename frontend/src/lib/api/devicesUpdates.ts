import { buildWsUrlFromHttpBase, canUseWebSockets, connectWebSocketWithFallback } from '$lib/api/wsClient';

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

export function connectDevicesUpdatesStream(
  handlers: DevicesUpdatesHandlers,
  options: { intervalMs?: number } = {}
): () => void {
  if (!canUseWebSockets()) {
    handlers.onError?.('WebSocket not supported in this environment');
    return () => {};
  }

  const url = buildDevicesSocketUrl(options.intervalMs ?? 2000);
  const connection = connectWebSocketWithFallback(
    url,
    {
      onOpen: () => {
        handlers.onOpen?.();
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
          handlers.onReady?.(typeof parsed.interval_ms === 'number' ? parsed.interval_ms : 2000);
          return;
        }
        if (parsed.type === 'update') {
          const reasons = Array.isArray(parsed.reasons) ? parsed.reasons : [];
          const timestampMs = typeof parsed.timestamp_ms === 'number' ? parsed.timestamp_ms : Date.now();
          handlers.onUpdate?.(reasons, timestampMs);
          return;
        }
        if (parsed.type === 'error') {
          handlers.onError?.(parsed.message || 'Devices updates stream error');
        }
      },
      onError: (message) => {
        handlers.onError?.(message || 'Devices updates stream connection failed');
      },
      onClose: () => {
        handlers.onClose?.();
      }
    },
    { errorMessage: 'Devices updates stream connection failed' }
  );

  if (!connection) {
    handlers.onError?.('Unable to open devices updates socket');
    return () => {};
  }

  return () => {
    connection.close();
  };
}

function buildDevicesSocketUrl(intervalMs: number): string {
  const interval = Math.max(250, Math.min(10000, Math.floor(intervalMs)));
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
