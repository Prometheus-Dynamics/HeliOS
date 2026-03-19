import type { ImuStatus } from '$lib/types/systems';
import { connectDeviceSensorsStream } from '$lib/api/deviceSensorsStream';
import { mapImuStatus } from './mappers';
import type { ImuStatusResponse } from './mappers';

type ImuStreamHandlers = {
  onStatus: (status: ImuStatus) => void;
  onError?: (message: string) => void;
  onOpen?: () => void;
  onClose?: () => void;
};

export function connectImuStream(handlers: ImuStreamHandlers): () => void {
  let latestStatusPayload: unknown = null;
  let flushTimer: ReturnType<typeof setTimeout> | null = null;
  const STATUS_FLUSH_MS = 50;

  const flushStatus = () => {
    flushTimer = null;
    const payload = latestStatusPayload;
    latestStatusPayload = null;
    const status = parseImuStreamPayload(payload);
    if (status) {
      handlers.onStatus(status);
    }
  };

  const scheduleFlush = () => {
    if (flushTimer) return;
    flushTimer = setTimeout(flushStatus, STATUS_FLUSH_MS);
  };

  const clearBufferedStatus = () => {
    latestStatusPayload = null;
    if (flushTimer) {
      clearTimeout(flushTimer);
      flushTimer = null;
    }
  };

  const close = connectDeviceSensorsStream(['imu'], STATUS_FLUSH_MS, {
    onImu: (payload) => {
      latestStatusPayload = payload;
      scheduleFlush();
    },
    onOpen: () => {
      handlers.onOpen?.();
    },
    onError: (message) => {
      clearBufferedStatus();
      handlers.onError?.(message.includes('Sensors') ? message.replace('Sensors', 'IMU') : message);
    },
    onClose: () => {
      clearBufferedStatus();
      handlers.onClose?.();
    }
  });

  return () => {
    clearBufferedStatus();
    close();
  };
}

function parseImuStreamPayload(payload: unknown): ImuStatus | null {
  if (!payload || typeof payload !== 'object') return null;
  try {
    return mapImuStatus(payload as ImuStatusResponse);
  } catch {
    return null;
  }
}
