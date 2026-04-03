import { writable, type Readable } from 'svelte/store';
import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';
import { apiUrl } from '$lib/api/client';
import { apiFetchResponse } from '$lib/api/core/http';
import type { PeripheralEntry } from '$lib/types/devices';
import type { FirmwareUpdatePayload } from '$lib/api/deviceSensorsStream';

export type FirmwareStatus = PeripheralEntry['firmware'];

export type FirmwareProgressPhase = 'idle' | 'queued' | 'flashing' | 'applying' | 'complete' | 'failed';

export type FirmwareControllerState = {
  status: FirmwareStatus | null;
  selection: string;
  selectionMissing: boolean;
  applyDisabled: boolean;
  loading: boolean;
  busy: boolean;
  error: string | null;
  pendingSelection: string | null;
  streamWarningShown: boolean;
  progressPhase: FirmwareProgressPhase;
  progressPct: number;
  progressLabel: string | null;
  progressDetail: string | null;
  progressStartedAtMs: number | null;
  progressPhaseStartedAtMs: number | null;
  progressUpdatedAtMs: number | null;
};

export type FirmwareController = {
  state: Readable<FirmwareControllerState>;
  setPeripheral: (peripheral: PeripheralEntry | null) => void;
  setSelection: (value: string) => void;
  handleStreamUpdate: (payload: FirmwareUpdatePayload, peripheral: PeripheralEntry | null) => void;
  handleStreamError: () => void;
  applyFirmware: (peripheral: PeripheralEntry | null) => Promise<boolean>;
  clear: () => void;
  destroy: () => void;
};

const DEFAULT_STATE: FirmwareControllerState = {
  status: null,
  selection: '',
  selectionMissing: false,
  applyDisabled: true,
  loading: false,
  busy: false,
  error: null,
  pendingSelection: null,
  streamWarningShown: false,
  progressPhase: 'idle',
  progressPct: 0,
  progressLabel: null,
  progressDetail: null,
  progressStartedAtMs: null,
  progressPhaseStartedAtMs: null,
  progressUpdatedAtMs: null
};

export function createFirmwareController(): FirmwareController {
  const state = writable<FirmwareControllerState>({ ...DEFAULT_STATE });
  let applyTimeout: ReturnType<typeof setTimeout> | null = null;
  let progressTimer: ReturnType<typeof setInterval> | null = null;

  const getSnapshot = (): FirmwareControllerState => {
    let snapshot = DEFAULT_STATE;
    state.subscribe((value) => (snapshot = value))();
    return snapshot;
  };

  const updateState = (patch: Partial<FirmwareControllerState>) => {
    state.update((current) => recalcState({ ...current, ...patch }));
  };

  const replaceState = (next: FirmwareControllerState) => {
    state.set(recalcState(next));
  };

  function recalcState(next: FirmwareControllerState): FirmwareControllerState {
    const selectionMissing = computeSelectionMissing(next.selection, next.status);
    const applyDisabled = computeApplyDisabled(next);
    return { ...next, selectionMissing, applyDisabled };
  }

  function clearApplyTimeout() {
    if (applyTimeout) {
      clearTimeout(applyTimeout);
      applyTimeout = null;
    }
  }

  function clearProgressTimer() {
    if (progressTimer) {
      clearInterval(progressTimer);
      progressTimer = null;
    }
  }

  function startProgressTimer() {
    if (progressTimer) return;
    progressTimer = setInterval(() => {
      const snapshot = getSnapshot();
      if (!snapshot.busy) {
        clearProgressTimer();
        return;
      }
      const phase = snapshot.progressPhase;
      if (phase === 'idle' || phase === 'complete' || phase === 'failed') return;
      const now = Date.now();
      const phaseStart = snapshot.progressPhaseStartedAtMs ?? snapshot.progressStartedAtMs ?? now;
      const nextPct = computePhaseProgress(phase, now - phaseStart, snapshot.progressPct);
      if (nextPct !== snapshot.progressPct) {
        updateState({ progressPct: nextPct });
      }
    }, 500);
  }

  function resetProgress() {
    clearProgressTimer();
    updateState({
      progressPhase: 'idle',
      progressPct: 0,
      progressLabel: null,
      progressDetail: null,
      progressStartedAtMs: null,
      progressPhaseStartedAtMs: null,
      progressUpdatedAtMs: null
    });
  }

  function syncProgressFromStatus(
    statusText: string | null | undefined,
    hint: { progressPct?: number | null; detail?: string | null } = {},
    now = Date.now()
  ) {
    const snapshot = getSnapshot();
    const phase = resolveProgressPhase(statusText ?? '');
    if (phase === 'idle') {
      if (!snapshot.busy) {
        resetProgress();
      }
      return;
    }

    const hintedPct =
      typeof hint.progressPct === 'number' && Number.isFinite(hint.progressPct)
        ? Math.max(0, Math.min(100, Math.round(hint.progressPct)))
        : null;
    const phaseChanged = phase !== snapshot.progressPhase;
    const progressStartedAtMs = snapshot.progressStartedAtMs ?? now;
    const progressPhaseStartedAtMs = phaseChanged ? now : snapshot.progressPhaseStartedAtMs ?? now;
    let progressPct = snapshot.progressPct;
    if (phase === 'complete' || phase === 'failed') {
      progressPct = 100;
    } else if (hintedPct != null) {
      progressPct = Math.max(progressPct, hintedPct);
    } else {
      progressPct = computePhaseProgress(phase, now - progressPhaseStartedAtMs, progressPct);
    }

    updateState({
      progressPhase: phase,
      progressLabel: statusText?.trim() || snapshot.progressLabel,
      progressDetail: hint.detail?.trim() || null,
      progressPct,
      progressStartedAtMs,
      progressPhaseStartedAtMs,
      progressUpdatedAtMs: now
    });

    if (phase === 'complete' || phase === 'failed') {
      clearProgressTimer();
    } else {
      startProgressTimer();
    }
  }

  function armApplyTimeout() {
    clearApplyTimeout();
    applyTimeout = setTimeout(() => {
      const snapshot = getSnapshot();
      if (!snapshot.busy) return;
      updateState({
        status: snapshot.status ? { ...snapshot.status, status: snapshot.status.status ?? 'Update queued' } : snapshot.status,
        progressDetail: 'No status update yet. Refresh to confirm when it finishes.'
      });
    }, 90_000);
  }

  function clear() {
    clearApplyTimeout();
    clearProgressTimer();
    replaceState({ ...DEFAULT_STATE });
  }

  function setPeripheral(peripheral: PeripheralEntry | null) {
    const snapshot = getSnapshot();
    const nextFirmware = peripheral?.firmware ?? null;
    const desired = nextFirmware?.desired?.trim().toLowerCase() ?? '';
    const active = nextFirmware?.active?.trim().toLowerCase() ?? '';
    const pending = snapshot.pendingSelection?.trim().toLowerCase() ?? '';
    let pendingSelection = snapshot.pendingSelection;
    if (pending && (pending === desired || pending === active)) {
      pendingSelection = null;
    }

    const selection = pendingSelection ? pendingSelection : canonicalFirmware(nextFirmware);
    let status = nextFirmware;
    if (pendingSelection) {
      status = {
        ...nextFirmware,
        desired: pendingSelection,
        last_error: null,
        status: nextFirmware?.status ?? 'Update queued'
      };
    }

    const statusLabel = (nextFirmware?.status ?? '').trim().toLowerCase();
    const phase = resolveProgressPhase(statusLabel);
    const applying = phase === 'queued' || phase === 'flashing' || phase === 'applying';

    replaceState({
      status,
      selection,
      selectionMissing: false,
      applyDisabled: false,
      loading: false,
      busy: Boolean(pendingSelection) || applying,
      error: null,
      pendingSelection,
      streamWarningShown: false,
      progressPhase: applying ? phase : 'idle',
      progressPct: applying ? Math.max(5, DEFAULT_STATE.progressPct) : 0,
      progressLabel: nextFirmware?.status ?? null,
      progressDetail: null,
      progressStartedAtMs: applying ? Date.now() : null,
      progressPhaseStartedAtMs: applying ? Date.now() : null,
      progressUpdatedAtMs: applying ? Date.now() : null
    });
    if (applying) {
      armApplyTimeout();
      startProgressTimer();
    } else {
      clearApplyTimeout();
      clearProgressTimer();
    }
  }

  function setSelection(value: string) {
    updateState({ selection: value, pendingSelection: null });
  }

  function formatFirmwareEventStatus(raw: string): string | null {
    const value = raw.trim().toLowerCase();
    if (!value) return null;
    switch (value) {
      case 'queued':
        return 'Update queued';
      case 'flashing':
        return 'Flashing firmware';
      case 'applying':
        return 'Applying firmware';
      case 'complete':
        return 'Firmware applied';
      case 'failed':
        return 'Firmware update failed';
      default:
        return raw.trim();
    }
  }

  function handleStreamUpdate(payload: FirmwareUpdatePayload, peripheral: PeripheralEntry | null) {
    const snapshot = getSnapshot();
    if (!peripheral || !snapshot.status) return;
    const deviceId = (payload?.device_id ?? '').trim();
    if (!deviceId || deviceId !== (peripheral.driverCameraId ?? '').trim()) return;

    const statusKey = (payload?.status ?? '').trim().toLowerCase();
    const nextStatus = formatFirmwareEventStatus(statusKey);
    const desired = (payload?.firmware ?? '').trim().toLowerCase();
    const active = (payload?.active ?? '').trim().toLowerCase();
    const next = { ...snapshot.status };

    if (nextStatus) next.status = nextStatus;
    if (desired) next.desired = desired;
    if (active) next.active = active;
    if (payload?.error) {
      next.last_error = payload.error;
    } else if (statusKey === 'complete' || statusKey === 'queued' || statusKey === 'flashing' || statusKey === 'applying') {
      next.last_error = null;
    }

    const pending = snapshot.pendingSelection?.trim().toLowerCase();
    let pendingSelection = snapshot.pendingSelection;
    if (pending) {
      if ((desired && pending === desired) || (active && pending === active)) {
        pendingSelection = null;
      }
    }

    const nextBusy = statusKey === 'queued' || statusKey === 'flashing' || statusKey === 'applying';
    if (nextBusy) {
      armApplyTimeout();
      startProgressTimer();
    } else if (statusKey === 'complete' || statusKey === 'failed') {
      clearApplyTimeout();
      clearProgressTimer();
    }

    updateState({
      status: next,
      busy: nextBusy ? true : statusKey === 'complete' || statusKey === 'failed' ? false : snapshot.busy,
      error:
        payload?.error ??
        (statusKey === 'complete' || statusKey === 'queued' || statusKey === 'flashing' || statusKey === 'applying'
          ? null
          : snapshot.error),
      pendingSelection
    });

    syncProgressFromStatus(nextStatus ?? next.status ?? null, {
      progressPct: payload?.progress_pct ?? null,
      detail: payload?.detail ?? null
    });
  }

  function handleStreamError() {
    const snapshot = getSnapshot();
    if (snapshot.status && !snapshot.status.status) {
      updateState({
        status: { ...snapshot.status, status: 'Update queued' },
        progressDetail: 'Firmware status stream disconnected. Update may still be running.'
      });
    }

    if (!snapshot.streamWarningShown) {
      updateState({ streamWarningShown: true });
    }
  }

  async function applyFirmware(peripheral: PeripheralEntry | null): Promise<boolean> {
    const snapshot = getSnapshot();
    if (!peripheral || !snapshot.status) return false;

    const deviceId = (peripheral.driverCameraId ?? '').trim();
    const firmware = (snapshot.selection ?? '').trim().toLowerCase();
    if (!deviceId || !firmware) return false;
    if (snapshot.busy) return false;

    const prevStatus = snapshot.status?.status ?? null;
    updateState({
      busy: true,
      error: null,
      pendingSelection: firmware,
      streamWarningShown: false,
      status: snapshot.status ? { ...snapshot.status, status: 'Applying firmware...' } : snapshot.status,
      progressPhase: 'queued',
      progressPct: 5,
      progressLabel: 'Applying firmware…',
      progressDetail: null,
      progressStartedAtMs: Date.now(),
      progressPhaseStartedAtMs: Date.now(),
      progressUpdatedAtMs: Date.now()
    });
    startProgressTimer();

    try {
      const url = apiUrl('/peripherals/sensors/firmware');
      const response = await apiFetchResponse(url, {
        method: 'POST',
        headers: { Accept: 'application/json', 'Content-Type': 'application/json' },
        body: JSON.stringify({ device_id: deviceId, firmware })
      });
      if (!response.ok) {
        const text = await response.text().catch(() => '');
        let message = text;
        if (text) {
          try {
            const parsed = JSON.parse(text) as { error?: string; message?: string };
            message = parsed.error ?? parsed.message ?? text;
          } catch {
            message = text;
          }
        }
        throw new Error(message || `Firmware update failed (${response.status})`);
      }

      const statusText = snapshot.status?.status?.trim() ?? '';
      updateState({
        status: snapshot.status
          ? {
              ...snapshot.status,
              desired: firmware,
              last_error: null,
              status: statusText.length ? statusText : 'Update queued'
            }
          : snapshot.status
      });

      armApplyTimeout();
      syncProgressFromStatus(statusText.length ? statusText : 'Update queued');
      return true;
    } catch (error) {
      const message = buildErrorMessage({ error, fallback: 'Unable to apply firmware right now.' });
      updateState({
        error: message,
        busy: false,
        status: snapshot.status ? { ...snapshot.status, status: prevStatus } : snapshot.status,
        progressPhase: 'idle',
        progressPct: 0,
        progressLabel: null,
        progressDetail: null,
        progressStartedAtMs: null,
        progressPhaseStartedAtMs: null,
        progressUpdatedAtMs: null
      });
      clearApplyTimeout();
      clearProgressTimer();
      reportError({
        title: 'Failed to apply firmware',
        error,
        fallback: message
      });
      return false;
    }
  }

  function destroy() {
    clearApplyTimeout();
    clearProgressTimer();
  }

  return {
    state,
    setPeripheral,
    setSelection,
    handleStreamUpdate,
    handleStreamError,
    applyFirmware,
    clear,
    destroy
  };
}

function resolveProgressPhase(status: string): FirmwareProgressPhase {
  const value = normalizeStatusKey(status);
  if (!value) return 'idle';
  if (value === 'complete') return 'complete';
  if (value === 'failed') return 'failed';
  if (value === 'flashing') return 'flashing';
  if (value === 'applying') return 'applying';
  if (value === 'queued') return 'queued';
  return 'idle';
}

function normalizeStatusKey(raw: string): string {
  const value = (raw ?? '').trim().toLowerCase();
  if (!value) return '';
  if (value.includes('complete') || value.includes('applied')) return 'complete';
  if (value.includes('fail') || value.includes('error')) return 'failed';
  if (value.includes('flash')) return 'flashing';
  if (value.includes('apply')) return 'applying';
  if (value.includes('queue')) return 'queued';
  return value;
}

function computePhaseProgress(phase: FirmwareProgressPhase, elapsedMs: number, current: number): number {
  const ranges: Record<FirmwareProgressPhase, { min: number; max: number; durationMs: number }> = {
    idle: { min: 0, max: 0, durationMs: 0 },
    queued: { min: 5, max: 20, durationMs: 15_000 },
    flashing: { min: 20, max: 90, durationMs: 90_000 },
    applying: { min: 90, max: 97, durationMs: 20_000 },
    complete: { min: 100, max: 100, durationMs: 0 },
    failed: { min: 100, max: 100, durationMs: 0 }
  };
  const range = ranges[phase];
  if (!range || range.durationMs <= 0) return range?.max ?? current;
  const clamped = Math.max(0, Math.min(1, elapsedMs / range.durationMs));
  const next = range.min + (range.max - range.min) * clamped;
  const rounded = Math.round(next * 10) / 10;
  return Math.max(current, rounded);
}

function canonicalFirmware(status: FirmwareStatus | string | null | undefined): string {
  if (typeof status === 'string') {
    return status.trim().toLowerCase();
  }
  if (!status) return '';
  const desired = status.desired?.trim().toLowerCase();
  if (desired) return desired;
  const active = status.active?.trim().toLowerCase();
  if (active) return active;
  const fallback = status.options?.[0]?.variant ?? '';
  return fallback.trim().toLowerCase();
}

function computeSelectionMissing(selection: string, status: FirmwareStatus | null): boolean {
  if (!selection) return false;
  const options = status?.options ?? [];
  return !options.some((option) => option.variant?.trim().toLowerCase() === selection);
}

function computeApplyDisabled(state: FirmwareControllerState): boolean {
  if (state.busy || state.loading) return true;
  if (!state.status) return true;
  const selection = state.selection.trim().toLowerCase();
  if (!selection) return true;
  const active = state.status.active?.trim().toLowerCase();
  const hasError = Boolean(state.status.last_error) || Boolean(state.error);
  const isBootloader = (state.status.mode ?? '').trim().toLowerCase() === 'bootloader';
  if (active && selection === active && !hasError && !isBootloader) {
    return true;
  }
  return false;
}
