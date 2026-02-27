import { browser } from '$app/environment';
import { buildWsUrl } from '$lib/api/wsClient';
import { getHttpClientBase } from '$lib/api/httpClient';
import type { ImuStatus } from '$lib/types/systems';
import { mapImuStatus } from './mappers';
import type { ImuStatusResponse } from './mappers';

type ImuStreamHandlers = {
  onStatus: (status: ImuStatus) => void;
  onError?: (message: string) => void;
  onOpen?: () => void;
  onClose?: () => void;
};

export function connectImuStream(handlers: ImuStreamHandlers): () => void {
  if (!browser) return () => {};
  const socket = new WebSocket(buildImuSocketUrl());
  // Clamp status decode/push cadence so high-rate IMU streams can't monopolize the UI thread.
  let latestStatusPayload: string | null = null;
  let flushTimer: ReturnType<typeof setTimeout> | null = null;
  const STATUS_FLUSH_MS = 50;

  const flushStatus = () => {
    flushTimer = null;
    const payload = latestStatusPayload;
    latestStatusPayload = null;
    if (!payload) return;
    const status = parseImuStreamPayload(payload);
    if (status) {
      handlers.onStatus(status);
    }
  };

  const scheduleFlush = () => {
    if (flushTimer) return;
    flushTimer = setTimeout(flushStatus, STATUS_FLUSH_MS);
  };

  const handleMessage = (event: MessageEvent) => {
    if (typeof event.data !== 'string') return;
    // Surface stream-level errors immediately.
    if (event.data.includes('"imu_stream_unavailable"')) {
      try {
        const parsed = JSON.parse(event.data);
        if (parsed?.status === 'imu_stream_unavailable') {
          handlers.onError?.(typeof parsed.reason === 'string' ? parsed.reason : 'IMU stream unavailable');
          return;
        }
      } catch {
        // Fall through to normal status buffering.
      }
    }

    latestStatusPayload = event.data;
    scheduleFlush();
  };

  const clearBufferedStatus = () => {
    latestStatusPayload = null;
    if (flushTimer) {
      clearTimeout(flushTimer);
      flushTimer = null;
    }
  };

  socket.addEventListener('open', () => handlers.onOpen?.());
  socket.addEventListener('message', handleMessage);
  socket.addEventListener('error', () => {
    clearBufferedStatus();
    handlers.onError?.('IMU stream connection failed');
    socket.close();
  });
  socket.addEventListener('close', () => {
    clearBufferedStatus();
    handlers.onClose?.();
  });

  return () => {
    clearBufferedStatus();
    socket.removeEventListener('message', handleMessage);
    if (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING) {
      socket.close();
    }
  };
}

function parseImuStreamPayload(payload: unknown): ImuStatus | null {
  if (typeof payload !== 'string') return null;
  try {
    const parsed = JSON.parse(payload) as ImuStatusResponse;
    return mapImuStatus(parsed);
  } catch {
    return null;
  }
}

function buildImuSocketUrl(): string {
  const base = getHttpClientBase();
  const origin = browser ? globalThis.location?.origin?.trim() || '' : '';
  return buildWsUrl({
    base,
    path: ['v1', 'ws', 'imu'],
    allowRelative: true,
    fallbackOrigin: origin
  });
}
