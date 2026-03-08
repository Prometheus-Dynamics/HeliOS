import { browser } from '$app/environment';
import { getHttpClientBase } from '$lib/api/httpClient';
import { buildWsUrl } from '$lib/api/wsClient';

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

function isFirmwareUpdatePayload(value: unknown): value is FirmwareUpdatePayload {
  if (!value || typeof value !== 'object') return false;
  const record = value as Record<string, unknown>;
  return (
    typeof record.device_id === 'string' &&
    typeof record.firmware === 'string' &&
    typeof record.status === 'string'
  );
}

export function connectDeviceSensorsStream(
  kinds: DeviceSensorKind[],
  intervalMs: number,
  handlers: DeviceSensorsStreamHandlers
): () => void {
  if (!browser) return () => {};

  const socket = new WebSocket(buildSensorsSocketUrl());

  const sendSubscribe = () => {
    const message = {
      op: 'subscribe',
      kinds,
      interval_ms: intervalMs
    };
    try {
      socket.send(JSON.stringify(message));
    } catch {
      // ignore
    }
  };

  const handleMessage = (event: MessageEvent) => {
    const data = event.data;
    if (typeof data !== 'string') return;
    try {
      const parsed = JSON.parse(data) as Record<string, unknown>;
      if (parsed?.status === 'sensors_stream_unavailable') {
        handlers.onError?.(typeof parsed.reason === 'string' ? parsed.reason : 'Sensors stream unavailable');
        return;
      }
      if (parsed?.imu != null) {
        handlers.onImu?.(parsed.imu);
      }
      if (parsed?.power != null) {
        handlers.onPower?.(parsed.power);
      }
      if (isFirmwareUpdatePayload(parsed?.firmware)) {
        handlers.onFirmware?.(parsed.firmware);
      }
    } catch {
      // ignore
    }
  };

  socket.addEventListener('open', () => {
    handlers.onOpen?.();
    sendSubscribe();
  });
  socket.addEventListener('message', handleMessage);
  socket.addEventListener('error', () => {
    handlers.onError?.('Sensors stream connection failed');
    socket.close();
  });
  socket.addEventListener('close', () => handlers.onClose?.());

  return () => {
    socket.removeEventListener('message', handleMessage);
    try {
      if (socket.readyState === WebSocket.OPEN) {
        socket.send(JSON.stringify({ op: 'unsubscribe' }));
      }
    } catch {
      // ignore
    }
    if (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING) {
      socket.close();
    }
  };
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
