import { writable, type Readable } from 'svelte/store';
import { apiUrl } from '$lib/api/client';
import { apiFetch } from '$lib/api/core/http';
import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';
import { emptyImuStatus, mapImuStatus, refreshImuStatus, updateImuConfig } from '$lib/api/systemsPage';
import { createBackoffTimer } from '$lib/utils/backoff';
import type { PeripheralEntry, SensorOrientation } from '$lib/types/devices';
import type { ImuStatus } from '$lib/types/systems';
import type { DeviceSensorKind } from '$lib/api/deviceSensorsStream';

export type PowerStatus = {
  watts: number | null;
  volts: number | null;
  amps: number | null;
  updatedAt: string | null;
  sources: Array<{
    label: string;
    bus?: number | null;
    address?: string | null;
    watts?: number | null;
    volts?: number | null;
    amps?: number | null;
    shuntVolts?: number | null;
  }>;
  errors: string[];
};

export type SensorTelemetryState = {
  orientation: SensorOrientation;
  imuStatus: ImuStatus;
  imuError: string | null;
  powerStatus: PowerStatus;
  powerError: string | null;
  powerLoading: boolean;
  powerLoadedOnce: boolean;
  sampleRate: string;
  streamEnabled: boolean;
  orientationLock: boolean;
};

export type SensorTelemetryController = {
  state: Readable<SensorTelemetryState>;
  setPeripheral: (peripheral: PeripheralEntry | null) => void;
  setStreamEnabled: (value: boolean) => void;
  setOrientationLock: (value: boolean) => void;
  updateSampleRate: (value: string) => Promise<boolean>;
  refreshImu: () => Promise<boolean>;
  refreshPower: () => Promise<boolean>;
  handleImuStream: (payload: unknown) => void;
  handlePowerStream: (payload: unknown) => void;
  handleStreamError: (kinds: DeviceSensorKind[], message: string) => void;
  startImuPolling: () => void;
  stopImuPolling: () => void;
  startPowerPolling: () => void;
  stopPowerPolling: () => void;
  destroy: () => void;
};

export function createSensorTelemetryController(options: { pollMs?: number } = {}): SensorTelemetryController {
  const { pollMs = 100 } = options;

  const state = writable<SensorTelemetryState>({
    orientation: { roll: 0, pitch: 0, yaw: 0 },
    imuStatus: emptyImuStatus(),
    imuError: null,
    powerStatus: emptyPowerStatus(),
    powerError: null,
    powerLoading: false,
    powerLoadedOnce: false,
    sampleRate: '100ms',
    streamEnabled: true,
    orientationLock: false
  });

  let imuPolling = false;
  let powerPolling = false;
  const imuPollTimer = createBackoffTimer({ baseMs: pollMs, maxMs: pollMs * 4 });
  const powerPollTimer = createBackoffTimer({ baseMs: pollMs, maxMs: pollMs * 4 });

  const getSnapshot = (): SensorTelemetryState => {
    let snapshot = emptyState();
    state.subscribe((value) => (snapshot = value))();
    return snapshot;
  };

  function emptyState(): SensorTelemetryState {
    return {
      orientation: { roll: 0, pitch: 0, yaw: 0 },
      imuStatus: emptyImuStatus(),
      imuError: null,
      powerStatus: emptyPowerStatus(),
      powerError: null,
      powerLoading: false,
      powerLoadedOnce: false,
      sampleRate: '100ms',
      streamEnabled: true,
      orientationLock: false
    };
  }

  function setPeripheral(peripheral: PeripheralEntry | null) {
    stopImuPolling();
    stopPowerPolling();
    state.update((current) => ({
      ...current,
      orientation: peripheral?.orientation ?? { roll: 0, pitch: 0, yaw: 0 },
      imuStatus: emptyImuStatus(),
      imuError: null,
      powerStatus: emptyPowerStatus(),
      powerError: null,
      powerLoading: false,
      powerLoadedOnce: false,
      sampleRate: peripheral?.interval ?? '100ms',
      streamEnabled: true,
      orientationLock: false
    }));
  }

  function setStreamEnabled(value: boolean) {
    state.update((current) => ({ ...current, streamEnabled: value }));
  }

  function setOrientationLock(value: boolean) {
    state.update((current) => ({ ...current, orientationLock: value }));
  }

  function handleImuStream(payload: unknown) {
    const status = mapImuStatus(payload as Parameters<typeof mapImuStatus>[0]);
    state.update((current) => ({
      ...current,
      imuStatus: status,
      imuError: status.lastError ?? null,
      orientation: status.orientation ?? current.orientation
    }));
  }

  function handlePowerStream(payload: unknown) {
    const next = normalizePowerStatus(payload);
    state.update((current) => ({
      ...current,
      powerStatus: next,
      powerLoadedOnce: true,
      powerError: null
    }));
  }

  function handleStreamError(kinds: DeviceSensorKind[], message: string) {
    state.update((current) => ({
      ...current,
      imuError: kinds.includes('imu') ? message : current.imuError,
      powerError: kinds.includes('power') ? message : current.powerError
    }));
  }

  async function refreshImu(): Promise<boolean> {
    try {
      const next = await refreshImuStatus();
      state.update((current) => ({
        ...current,
        imuStatus: next,
        imuError: next.lastError ?? null,
        orientation: next.orientation ?? current.orientation
      }));
      return true;
    } catch (error) {
      const message = buildErrorMessage({ error, fallback: 'Unable to load IMU telemetry.' });
      state.update((current) => ({ ...current, imuError: message }));
      return false;
    }
  }

  async function refreshPower(): Promise<boolean> {
    const snapshot = getSnapshot();
    if (snapshot.powerLoading) return false;
    state.update((current) => ({ ...current, powerLoading: true, powerError: null }));
    try {
      const url = apiUrl('/device/power');
      const json = await apiFetch<unknown>(url, { headers: { Accept: 'application/json' } });
      state.update((current) => ({
        ...current,
        powerStatus: normalizePowerStatus(json),
        powerLoadedOnce: true
      }));
      return true;
    } catch (error) {
      const message = buildErrorMessage({ error, fallback: 'Unable to load power telemetry.' });
      state.update((current) => ({ ...current, powerError: message }));
      return false;
    } finally {
      state.update((current) => ({ ...current, powerLoading: false }));
    }
  }

  async function updateSampleRate(value: string): Promise<boolean> {
    state.update((current) => ({ ...current, sampleRate: value }));
    const numeric = parseInt(value, 10);
    if (!Number.isFinite(numeric) || numeric <= 0) return false;
    try {
      const status = await updateImuConfig({ updateIntervalMs: numeric });
      state.update((current) => ({
        ...current,
        imuStatus: status,
        orientation: status.orientation ?? current.orientation,
        imuError: status.lastError ?? null
      }));
      return true;
    } catch (error) {
      reportError({
        title: 'Failed to update IMU',
        error,
        fallback: 'Unable to update IMU settings right now.',
        inline: (message) => {
          state.update((current) => ({ ...current, imuError: message }));
        }
      });
      return false;
    }
  }

  function startImuPolling() {
    if (imuPolling) return;
    imuPolling = true;
    imuPollTimer.reset();
    void refreshImu().then((ok) => {
      if (!imuPolling) return;
      if (ok) imuPollTimer.reset();
      else imuPollTimer.bump();
      scheduleImuPoll();
    });
  }

  function stopImuPolling() {
    imuPolling = false;
    imuPollTimer.cancel();
  }

  function startPowerPolling() {
    if (powerPolling) return;
    powerPolling = true;
    powerPollTimer.reset();
    void refreshPower().then((ok) => {
      if (!powerPolling) return;
      if (ok) powerPollTimer.reset();
      else powerPollTimer.bump();
      schedulePowerPoll();
    });
  }

  function stopPowerPolling() {
    powerPolling = false;
    powerPollTimer.cancel();
  }

  function scheduleImuPoll() {
    imuPollTimer.schedule(() => {
      if (!imuPolling) return;
      void refreshImu().then((ok) => {
        if (!imuPolling) return;
        if (ok) imuPollTimer.reset();
        else imuPollTimer.bump();
        scheduleImuPoll();
      });
    });
  }

  function schedulePowerPoll() {
    powerPollTimer.schedule(() => {
      if (!powerPolling) return;
      void refreshPower().then((ok) => {
        if (!powerPolling) return;
        if (ok) powerPollTimer.reset();
        else powerPollTimer.bump();
        schedulePowerPoll();
      });
    });
  }

  function destroy() {
    stopImuPolling();
    stopPowerPolling();
  }

  return {
    state,
    setPeripheral,
    setStreamEnabled,
    setOrientationLock,
    updateSampleRate,
    refreshImu,
    refreshPower,
    handleImuStream,
    handlePowerStream,
    handleStreamError,
    startImuPolling,
    stopImuPolling,
    startPowerPolling,
    stopPowerPolling,
    destroy
  };
}

function emptyPowerStatus(): PowerStatus {
  return { watts: null, volts: null, amps: null, updatedAt: null, sources: [], errors: [] };
}

const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : null;

function normalizePowerStatus(value: unknown): PowerStatus {
  const record = asRecord(value);
  if (!record) {
    return emptyPowerStatus();
  }
  const toNumber = (v: unknown) => {
    if (typeof v === 'number' && Number.isFinite(v)) return v;
    if (typeof v === 'string' && v.trim().length) {
      const parsed = Number(v.trim());
      return Number.isFinite(parsed) ? parsed : null;
    }
    return null;
  };
  const toString = (v: unknown) => (typeof v === 'string' && v.trim().length ? v.trim() : null);
  const sources: PowerStatus['sources'] = Array.isArray(record.sources)
    ? record.sources
        .map((entry) => {
          const source = asRecord(entry);
          const bus = toNumber(source?.bus);
          return {
            label: toString(source?.label) ?? 'Source',
            bus: bus != null ? Math.trunc(bus) : null,
            address: toString(source?.address),
            watts: toNumber(source?.watts),
            volts: toNumber(source?.volts),
            amps: toNumber(source?.amps),
            shuntVolts: toNumber(source?.shunt_volts ?? source?.shuntVolts)
          };
        })
        .filter((entry) => entry.label)
    : [];

  const firstSourceVolts = sources.find((entry) => entry.volts != null)?.volts ?? null;
  const sourceAmps = sources.map((entry) => entry.amps).filter((entry): entry is number => entry != null);
  const sourceWatts = sources
    .map((entry) => entry.watts ?? (entry.volts != null && entry.amps != null ? entry.volts * entry.amps : null))
    .filter((entry): entry is number => entry != null);
  const sourceAmpsSum = sourceAmps.length ? sourceAmps.reduce((sum, next) => sum + next, 0) : null;
  const sourceWattsSum = sourceWatts.length ? sourceWatts.reduce((sum, next) => sum + next, 0) : null;

  const volts = toNumber(record.volts) ?? firstSourceVolts;
  const amps = toNumber(record.amps) ?? sourceAmpsSum;
  const rawWatts = toNumber(record.watts);
  const derivedWatts = volts != null && amps != null ? volts * amps : null;
  const watts =
    rawWatts == null
      ? (sourceWattsSum ?? derivedWatts)
      : (Math.abs(rawWatts) <= 1e-9 && derivedWatts != null && Math.abs(derivedWatts) > 1e-6 ? derivedWatts : rawWatts);

  return {
    watts,
    volts,
    amps,
    updatedAt: toString(record.updated_at ?? record.updatedAt),
    sources,
    errors: Array.isArray(record.errors)
      ? record.errors.map((entry) => toString(entry)).filter((entry): entry is string => entry != null)
      : []
  };
}
