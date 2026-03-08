import type { ControlMeta, ControlValue, StreamInfo, StreamManifest } from '$lib/api/httpClient';
import type { StreamsApi } from '$lib/api/streamsApi';
import type { ResourceSample } from '$lib/api/telemetry';

export type ControlSocket = {
  ready: () => boolean;
  send: (payload: unknown) => void;
} | null;

type UnknownRecord = Record<string, unknown>;

const asRecord = (value: unknown): UnknownRecord | null =>
  value && typeof value === 'object' ? (value as UnknownRecord) : null;

type ControlState = {
  get stream(): StreamInfo | null;
  get streamId(): string;
  get manifestState(): StreamManifest | null;
  get controls(): ControlMeta[];
  set controls(value: ControlMeta[]);
  get controlsQuery(): string;
  set controlsQuery(value: string);
  get controlState(): Record<number, number | boolean | null>;
  set controlState(value: Record<number, number | boolean | null>);
  get controlAppliedState(): Record<number, number | boolean | null>;
  set controlAppliedState(value: Record<number, number | boolean | null>);
  get controlBusy(): Record<number, boolean>;
  set controlBusy(value: Record<number, boolean>);
  get controlApplyTimers(): Map<number, number>;
  get controlApplySeqById(): Map<number, number>;
  get controlSocket(): ControlSocket;
};

type ControlDeps = {
  streamsApi: typeof StreamsApi;
  toaster: {
    success: (payload: { title: string; description?: string }) => void;
    error: (payload: { title: string; description?: string }) => void;
  };
  reportError: (args: { title: string; error: unknown; fallback: string }) => void;
  controlApplyDebounceMs: number;
};

export function createCameraControlController(state: ControlState, deps: ControlDeps) {
  function timeoutApplyHint(sample: ResourceSample | null | undefined): string {
    const engine = sample?.engine ?? null;
    if (engine && !engine.connected) {
      if (typeof engine.last_disconnect_ms === 'number' && engine.last_disconnect_ms > 0) {
        const ageMs = Math.max(0, Date.now() - engine.last_disconnect_ms);
        const ageSec = Math.max(1, Math.round(ageMs / 1000));
        return `Engine restarted ${ageSec}s ago while applying settings. Check decoder/libcamera settings or engine logs.`;
      }
      return 'Engine disconnected while applying settings. Check decoder/libcamera settings or engine logs.';
    }
    return 'Apply request timed out. If the engine restarted, check decoder/libcamera settings or engine logs.';
  }

  function downloadManifest(): void {
    const payload = state.stream?.manifest ?? state.manifestState;
    if (!payload) {
      deps.toaster.error({ title: 'Manifest unavailable', description: 'No manifest is available to download yet.' });
      return;
    }
    const json = JSON.stringify(payload, null, 2);
    const blob = new Blob([json], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    const id = String(state.stream?.id ?? state.streamId ?? 'stream').trim() || 'stream';
    link.href = url;
    link.download = `manifest-${id}.json`;
    document.body.appendChild(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
  }

  function seedControlState(entries: ControlMeta[]): Record<number, number | boolean | null> {
    const map: Record<number, number | boolean | null> = {};
    for (const ctrl of entries) {
      const current = extractValue(asRecord(ctrl)?.value ?? ctrl.default);
      map[ctrl.id] = current;
    }
    return map;
  }

  function extractValue(value: unknown): number | boolean | null {
    if (!value) return null;
    if (value === 'None') return null;
    const record = asRecord(value);
    if (!record) return null;
    if (typeof record.Bool === 'boolean') return record.Bool;
    if (typeof record.Int === 'number') return record.Int;
    if (typeof record.Uint === 'number') return record.Uint;
    if (typeof record.Float === 'number') return record.Float;

    const kind = typeof record.kind === 'string' ? record.kind : null;
    const payload = record.value;
    if (!kind) return null;
    if (kind === 'none') return null;
    if (kind === 'bool') return Boolean(payload);
    if (kind === 'int' || kind === 'int32' || kind === 'int64') return Number(payload);
    if (kind === 'uint' || kind === 'uint32' || kind === 'uint16' || kind === 'byte') return Math.max(0, Number(payload));
    if (kind === 'float') return Number(payload);
    return null;
  }

  function controlStep(ctrl: ControlMeta): number | null {
    const step = ctrl.step ? extractValue(ctrl.step) : null;
    return typeof step === 'number' && Number.isFinite(step) && step > 0 ? step : null;
  }

  function menuOptions(ctrl: ControlMeta): Array<{ value: number; label: string }> {
    const list: unknown[] = Array.isArray(ctrl.menu) ? ctrl.menu : [];
    const options: Array<{ value: number; label: string }> = [];
    list.forEach((entry, idx) => {
      if (typeof entry === 'number') {
        options.push({ value: entry, label: String(entry) });
      } else if (typeof entry === 'string') {
        options.push({ value: idx, label: entry });
      } else {
        const entryRecord = asRecord(entry);
        if (entryRecord) {
          const val = Number(entryRecord.value ?? entryRecord.id ?? idx);
          const label = String(entryRecord.label ?? entryRecord.name ?? val);
          options.push({ value: Number.isFinite(val) ? val : idx, label });
        } else {
          options.push({ value: idx, label: `Option ${idx + 1}` });
        }
      }
    });
    return options.length ? options : [{ value: 0, label: '0' }];
  }

  function menuValueDisplay(ctrl: ControlMeta, value: number | boolean | null): string {
    const numeric = Math.trunc(Number(value ?? 0));
    if (!Number.isFinite(numeric)) return 'n/a';
    const options = menuOptions(ctrl);
    const found = options.find((opt) => opt.value === numeric);
    if (!found) return String(numeric);
    const label = String(found.label ?? '').trim();
    const num = String(numeric);
    if (!label.length || label === num) return `[${num}]`;
    return `[${num}] - ${label}`;
  }

  function displayValue(ctrl: ControlMeta, value: number | boolean | null): string {
    if (value == null) return '—';
    if (ctrl.kind === 'Bool') return value ? 'On' : 'Off';
    if (ctrl.kind === 'Menu' || ctrl.kind === 'IntMenu') return menuValueDisplay(ctrl, value);
    if (typeof value === 'number') {
      if (!Number.isFinite(value)) return '—';
      if (ctrl.kind === 'Float') return value.toFixed(3).replace(/\.?0+$/, '');
      return String(Math.trunc(value));
    }
    return String(value);
  }

  function accessLabel(access: ControlMeta['access']): string {
    switch (access) {
      case 'ReadWrite':
        return 'Read|Write';
      case 'ReadOnly':
        return 'Read';
      default:
        return String(access ?? '');
    }
  }

  function accessBadgeClass(access: ControlMeta['access']): string {
    switch (access) {
      case 'ReadWrite':
        return 'border-primary-500/50 bg-primary-500/10 text-primary-100';
      case 'ReadOnly':
        return 'border-surface-700/70 bg-surface-900/40 text-surface-400';
      default:
        return 'border-surface-700/70 bg-surface-900/40 text-surface-400';
    }
  }

  function filteredControls(): ControlMeta[] {
    const q = state.controlsQuery.trim().toLowerCase();
    const list = state.controls.filter((ctrl) => {
      if (!q) return true;
      return String(ctrl.name ?? '').toLowerCase().includes(q);
    });
    list.sort((a, b) => {
      const accA = a.access === 'ReadWrite' ? 0 : 1;
      const accB = b.access === 'ReadWrite' ? 0 : 1;
      if (accA !== accB) return accA - accB;
      return String(a.name).localeCompare(String(b.name));
    });
    return list;
  }

  function isControlChanged(ctrl: ControlMeta): boolean {
    return state.controlState[ctrl.id] !== state.controlAppliedState[ctrl.id];
  }

  function buildControlValue(kind: ControlMeta['kind'], next: number | boolean | null): Record<string, unknown> {
    if (next == null) return { kind: 'none' };
    switch (kind) {
      case 'Bool':
        return { kind: 'bool', value: Boolean(next) };
      case 'Int':
        return { kind: 'int', value: Math.trunc(Number(next ?? 0)) };
      case 'Uint': {
        const v = Math.max(0, Number(next ?? 0));
        return { kind: 'uint', value: Math.trunc(v) };
      }
      case 'Float':
        return { kind: 'float', value: Number(next ?? 0) };
      case 'Menu':
        return { kind: 'uint', value: Math.max(0, Math.trunc(Number(next ?? 0))) };
      case 'IntMenu':
        return { kind: 'int', value: Math.trunc(Number(next ?? 0)) };
      default:
        return { kind: 'none' };
    }
  }

  function controlMin(ctrl: ControlMeta): number | null {
    const value = extractValue(ctrl.min);
    return typeof value === 'number' ? value : null;
  }

  function controlMax(ctrl: ControlMeta): number | null {
    const value = extractValue(ctrl.max);
    return typeof value === 'number' ? value : null;
  }

  function clampNumber(value: number, min: number | null, max: number | null): number {
    let next = value;
    if (min != null) next = Math.max(min, next);
    if (max != null) next = Math.min(max, next);
    return next;
  }

  function clampControlValue(ctrl: ControlMeta, next: number | boolean | null): number | boolean | null {
    if (next == null) return null;
    if (ctrl.kind === 'Bool') return Boolean(next);

    if (ctrl.kind === 'Menu' || ctrl.kind === 'IntMenu') {
      const options = menuOptions(ctrl);
      const numeric = Math.trunc(Number(next ?? 0));
      if (!Number.isFinite(numeric)) return null;
      const allowed = new Set(options.map((o) => o.value));
      if (allowed.has(numeric)) return numeric;
      const min = Math.min(...options.map((o) => o.value));
      const max = Math.max(...options.map((o) => o.value));
      return clampNumber(numeric, min, max);
    }

    const numeric = Number(next);
    if (!Number.isFinite(numeric)) return null;
    const min = controlMin(ctrl);
    const max = controlMax(ctrl);
    const clamped = clampNumber(numeric, min, max);
    return ctrl.kind === 'Float' ? clamped : Math.trunc(clamped);
  }

  function scheduleControlApply(ctrl: ControlMeta, next: number | boolean | null): void {
    const normalized = clampControlValue(ctrl, next);
    const existing = state.controlApplyTimers.get(ctrl.id);
    if (existing != null) {
      clearTimeout(existing);
    }
    const timer = window.setTimeout(() => {
      state.controlApplyTimers.delete(ctrl.id);
      void applyControl(ctrl, normalized, { silent: true });
    }, deps.controlApplyDebounceMs);
    state.controlApplyTimers.set(ctrl.id, timer);
  }

  async function applyControl(
    ctrl: ControlMeta,
    next: number | boolean | null,
    options: { silent?: boolean } = {}
  ): Promise<void> {
    if (ctrl.access === 'ReadOnly') return;
    const pendingTimer = state.controlApplyTimers.get(ctrl.id);
    if (pendingTimer != null) {
      clearTimeout(pendingTimer);
      state.controlApplyTimers.delete(ctrl.id);
    }
    const normalized = clampControlValue(ctrl, next);
    const seq = (state.controlApplySeqById.get(ctrl.id) ?? 0) + 1;
    state.controlApplySeqById.set(ctrl.id, seq);
    state.controlState = { ...state.controlState, [ctrl.id]: normalized };
    if (state.controlSocket?.ready()) {
      const requestBody = buildControlValue(ctrl.kind, normalized);
      state.controlSocket.send({
        type: 'set_control',
        control_id: ctrl.id,
        value: requestBody
      });
      state.controlAppliedState = { ...state.controlAppliedState, [ctrl.id]: normalized };
      if (!options.silent) {
        deps.toaster.success({ title: 'Control updated', description: ctrl.name });
      }
      return;
    }
    state.controlBusy = { ...state.controlBusy, [ctrl.id]: true };
    try {
      const requestBody = buildControlValue(ctrl.kind, normalized) as unknown as ControlValue;
      await deps.streamsApi.setControl({ id: state.stream?.id ?? state.streamId, controlId: ctrl.id, requestBody });
      if (state.controlApplySeqById.get(ctrl.id) === seq) {
        state.controlAppliedState = { ...state.controlAppliedState, [ctrl.id]: normalized };
      }
      if (!options.silent) {
        deps.toaster.success({ title: 'Control updated', description: ctrl.name });
      }
    } catch (err) {
      if (state.controlApplySeqById.get(ctrl.id) === seq) {
        console.error('Failed to set control', err);
        deps.reportError({
          title: 'Control update failed',
          error: err,
          fallback: 'Unable to update the control right now.'
        });
      }
    } finally {
      if (state.controlApplySeqById.get(ctrl.id) === seq) {
        state.controlBusy = { ...state.controlBusy, [ctrl.id]: false };
      }
    }
  }

  return {
    timeoutApplyHint,
    downloadManifest,
    seedControlState,
    extractValue,
    controlStep,
    displayValue,
    accessLabel,
    accessBadgeClass,
    filteredControls,
    isControlChanged,
    buildControlValue,
    menuOptions,
    menuValueDisplay,
    controlMin,
    controlMax,
    clampNumber,
    clampControlValue,
    scheduleControlApply,
    applyControl
  };
}
