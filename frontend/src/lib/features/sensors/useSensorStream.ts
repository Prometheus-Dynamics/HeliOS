import { writable, type Readable } from 'svelte/store';
import { connectDeviceSensorsStream, type DeviceSensorKind, type FirmwareUpdatePayload } from '$lib/api/deviceSensorsStream';

type SensorStreamState = {
  connected: boolean;
  enabled: boolean;
  kinds: DeviceSensorKind[];
  intervalMs: number;
  lastMessageAt: number | null;
  error: string | null;
};

type SensorStreamPayloads = {
  imu: Readable<unknown | null>;
  power: Readable<unknown | null>;
  firmware: Readable<FirmwareUpdatePayload | null>;
};

export type SensorStreamStore = {
  state: Readable<SensorStreamState>;
  payloads: SensorStreamPayloads;
  start: () => void;
  stop: () => void;
  setEnabled: (enabled: boolean) => void;
  setKinds: (kinds: DeviceSensorKind[]) => void;
  setIntervalMs: (intervalMs: number) => void;
  destroy: () => void;
};

type SensorStreamOptions = {
  kinds: DeviceSensorKind[];
  intervalMs?: number;
  enabled?: boolean;
  onImu?: (payload: unknown) => void;
  onPower?: (payload: unknown) => void;
  onFirmware?: (payload: FirmwareUpdatePayload) => void;
  onOpen?: () => void;
  onClose?: () => void;
  onError?: (message: string) => void;
};

export function createSensorStreamStore(options: SensorStreamOptions): SensorStreamStore {
  const initialInterval = options.intervalMs ?? 100;
  const state = writable<SensorStreamState>({
    connected: false,
    enabled: options.enabled ?? true,
    kinds: options.kinds,
    intervalMs: initialInterval,
    lastMessageAt: null,
    error: null
  });

  const imuPayload = writable<unknown | null>(null);
  const powerPayload = writable<unknown | null>(null);
  const firmwarePayload = writable<FirmwareUpdatePayload | null>(null);

  let closeStream: (() => void) | null = null;

  function start(): void {
    stop();
    let snapshot: SensorStreamState | null = null;
    state.update((current) => {
      snapshot = current;
      return { ...current, error: null };
    });
    const current = snapshot as SensorStreamState | null;
    if (!current || !current.enabled || current.kinds.length === 0) {
      return;
    }
    closeStream = connectDeviceSensorsStream(current.kinds, current.intervalMs, {
      onImu: (payload) => {
        state.update((value) => ({ ...value, lastMessageAt: Date.now() }));
        imuPayload.set(payload);
        options.onImu?.(payload);
      },
      onPower: (payload) => {
        state.update((value) => ({ ...value, lastMessageAt: Date.now() }));
        powerPayload.set(payload);
        options.onPower?.(payload);
      },
      onFirmware: (payload) => {
        state.update((value) => ({ ...value, lastMessageAt: Date.now() }));
        firmwarePayload.set(payload);
        options.onFirmware?.(payload);
      },
      onOpen: () => {
        state.update((value) => ({ ...value, connected: true, error: null }));
        options.onOpen?.();
      },
      onClose: () => {
        state.update((value) => ({ ...value, connected: false }));
        options.onClose?.();
      },
      onError: (message) => {
        state.update((value) => ({ ...value, connected: false, error: message }));
        options.onError?.(message);
      }
    });
  }

  function stop(): void {
    if (closeStream) {
      closeStream();
      closeStream = null;
    }
    state.update((current) => ({ ...current, connected: false }));
  }

  function setEnabled(enabled: boolean): void {
    state.update((current) => ({ ...current, enabled }));
    if (enabled) start();
    else stop();
  }

  function setKinds(kinds: DeviceSensorKind[]): void {
    state.update((current) => ({ ...current, kinds }));
    start();
  }

  function setIntervalMs(intervalMs: number): void {
    state.update((current) => ({ ...current, intervalMs }));
    start();
  }

  function destroy(): void {
    stop();
  }

  if (options.enabled ?? true) {
    start();
  }

  return {
    state,
    payloads: {
      imu: imuPayload,
      power: powerPayload,
      firmware: firmwarePayload
    },
    start,
    stop,
    setEnabled,
    setKinds,
    setIntervalMs,
    destroy
  };
}
