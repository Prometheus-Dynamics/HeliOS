<script lang="ts">
  import { onDestroy, onMount } from 'svelte';

  import { toaster } from '$lib';
  import { apiFetch } from '$lib/api/core/http';
  import { resourceTelemetryStore, type ResourceSample } from '$lib/api/telemetry';
  import type { PeripheralEntry } from '$lib/types/devices';
  import SensorModalShell from './SensorModalShell.svelte';
  import FanStatusPanel from './fan/FanStatusPanel.svelte';
  import FanControlPanel from './fan/FanControlPanel.svelte';
  import FanConfigPanel from './fan/FanConfigPanel.svelte';
  import FanAdvancedModal from './fan/FanAdvancedModal.svelte';
  import type { FanConfig, FanCurvePoint, FanStatus } from '../../../routes/settings/types';
  import { deviceSettingsStore, type DeviceSettingsState } from '../../../routes/settings/deviceSettingsStore';
  import { REQUESTED_BY } from '../../../routes/settings/api';
  import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';
  import { createSettingsLoader } from '$lib/components/peripherals/useSensorModalState';

  type Props = {
    peripheral: PeripheralEntry;
    onClose: () => void;
    onRefresh: () => void;
  };

  const { peripheral, onClose, onRefresh }: Props = $props();

  const telemetry = $derived($resourceTelemetryStore as ResourceSample);
  const deviceState = $derived($deviceSettingsStore as DeviceSettingsState);

  let fanStatus = $state<FanStatus | null>(null);

  const FAN_STATUS_REFRESH_MS = 5000;
  const TEMP_MIN_C = 30;
  const TEMP_MAX_C = 95;
  const DEFAULT_CURVE: FanCurvePoint[] = [
    { temp_c: 40, percent: 70 },
    { temp_c: 50, percent: 85 },
    { temp_c: 60, percent: 100 }
  ];

  const DEFAULT_FAN: FanConfig = {
    enabled: true,
    pwm_path: 'auto',
    tacho_path: 'auto',
    min_percent: 0,
    max_percent: 100,
    manual_percent: null,
    poll_interval_ms: 5000,
    invert_pwm: true,
    curve: DEFAULT_CURVE
  };

  let form = $state<FanConfig>(normalizeFan(DEFAULT_FAN));
  let manualInput = $state('');
  let selectedPointIndex = $state(0);
  let fixedPercent = $state(50);

  let busy = $state(false);
  let dirty = $state(false);
  let status = $state<string | null>(null);
  let error = $state<string | null>(null);
  let settingsBusy = $state(false);
  let settingsError = $state<string | null>(null);
  let showAdvanced = $state(false);

  let refreshingStatus = $state(false);
  let statusRefreshError = $state<string | null>(null);
  let statusPollId: ReturnType<typeof setInterval> | null = null;

  const mode = $derived(form.enabled ? (form.manual_percent != null ? 'fixed' : 'curve') : 'disabled');
  const statusMode = $derived(normalizeStatusMode(fanStatus?.mode));
  const displayMode = $derived(dirty ? mode : statusMode ?? mode);
  const statusMessage = $derived.by(() => status ?? (refreshingStatus ? 'Refreshing status…' : null));
  const currentTemp = $derived(fanStatus?.temperature_c ?? telemetry.cpu.temperature_c ?? null);
  const previewTarget = $derived(computeTarget(form, currentTemp));
  const ensureDeviceSettings = createSettingsLoader({
    load: deviceSettingsStore.load,
    setBusy: (value) => {
      settingsBusy = value;
    },
    setError: (value) => {
      settingsError = value;
    },
    fallback: 'Unable to load fan settings.'
  });

  onMount(() => {
    if (!deviceState.initialized) {
      void ensureDeviceSettings();
    }
    void refreshFanStatus({ silent: true });
    scheduleStatusRefresh();
  });

  onDestroy(() => {
    stopStatusRefresh();
  });

  $effect(() => {
    if (dirty || busy) return;
    const incoming = deviceState.data?.fan ? fanFromDevice(deviceState.data.fan) : normalizeFan(DEFAULT_FAN);
    const nextManual = incoming.manual_percent == null ? '' : String(incoming.manual_percent);
    if (!fanEquals(form, incoming) || manualInput !== nextManual) {
      form = incoming;
      manualInput = nextManual;
    }
  });

  function clamp(value: number, min: number, max: number): number {
    if (!Number.isFinite(value)) return min;
    return Math.min(Math.max(value, min), max);
  }

  function round2(value: number): number {
    if (!Number.isFinite(value)) return 0;
    return Math.round(value * 100) / 100;
  }

  function ensureCurveEndpoints(input: FanCurvePoint[], options: { tempMin: number; tempMax: number }): FanCurvePoint[] {
    const { tempMin, tempMax } = options;
    const sorted = input
      .filter((p) => Number.isFinite(p.temp_c) && Number.isFinite(p.percent))
      .map((p) => ({ temp_c: round2(Number(p.temp_c)), percent: clamp(Math.round(Number(p.percent)), 0, 100) }))
      .sort((a, b) => a.temp_c - b.temp_c);

    if (!sorted.length) {
      return [
        { temp_c: tempMin, percent: 0 },
        { temp_c: tempMax, percent: 100 }
      ];
    }

    if (sorted.length === 1) {
      const only = sorted[0]!;
      return [
        { temp_c: tempMin, percent: only.percent },
        { temp_c: tempMax, percent: only.percent }
      ];
    }

    sorted[0] = { ...sorted[0]!, temp_c: tempMin };
    sorted[sorted.length - 1] = { ...sorted.at(-1)!, temp_c: tempMax };

    const deduped: FanCurvePoint[] = [];
    for (const point of sorted) {
      const last = deduped.at(-1);
      if (last && Math.abs(point.temp_c - last.temp_c) < 0.0001) {
        deduped[deduped.length - 1] = point;
        continue;
      }
      deduped.push(point);
    }

    return deduped
      .map((point, idx, arr) => {
        const prevTemp = idx === 0 ? tempMin : arr[idx - 1]!.temp_c + 0.1;
        const nextTemp = idx === arr.length - 1 ? tempMax : arr[idx + 1]!.temp_c - 0.1;
        const temp = idx === 0 ? tempMin : idx === arr.length - 1 ? tempMax : clamp(point.temp_c, prevTemp, nextTemp);
        return { temp_c: round2(temp), percent: clamp(Math.round(point.percent), 0, 100) };
      })
      .sort((a, b) => a.temp_c - b.temp_c);
  }

  function enforceMonotonicCurve(input: FanCurvePoint[]): FanCurvePoint[] {
    let lastPercent = 0;
    return input.map((point) => {
      const next = clamp(Math.round(point.percent), lastPercent, 100);
      lastPercent = next;
      return { ...point, percent: next };
    });
  }

  function normalizeFan(input: FanConfig): FanConfig {
    const min = 0;
    const max = 100;
    const poll = Math.max(500, Math.round(input.poll_interval_ms || DEFAULT_FAN.poll_interval_ms));
    const manual = input.manual_percent == null ? null : clamp(input.manual_percent, 0, 100);
    const invert = Boolean(input.invert_pwm);
    const curveBase = (input.curve ?? []).length ? input.curve : [...DEFAULT_CURVE];
    const curve = enforceMonotonicCurve(
      ensureCurveEndpoints(curveBase, { tempMin: TEMP_MIN_C, tempMax: TEMP_MAX_C }).map((point) => ({
        temp_c: point.temp_c,
        percent: clamp(point.percent, min, max)
      }))
    );
    return {
      enabled: Boolean(input.enabled),
      pwm_path: input.pwm_path?.trim() || DEFAULT_FAN.pwm_path,
      tacho_path: input.tacho_path?.trim() || DEFAULT_FAN.tacho_path,
      min_percent: min,
      max_percent: max,
      manual_percent: manual,
      poll_interval_ms: poll,
      invert_pwm: invert,
      curve
    };
  }

  function fanFromDevice(input: FanConfig): FanConfig {
    const curveBase = (input.curve ?? []).map((point) => ({
      temp_c: point.temp_c,
      percent: point.percent
    }));
    return normalizeFan({
      ...input,
      curve: curveBase
    });
  }

  function fanToDevice(input: FanConfig): FanConfig {
    return normalizeFan(input);
  }

  function computeTarget(config: FanConfig, temperature: number | null): number {
    if (!config.enabled) return 0;
    if (config.manual_percent != null) return clamp(config.manual_percent, 0, 100);
    const points = (config.curve ?? []).slice().sort((a, b) => a.temp_c - b.temp_c);
    if (!points.length) return 0;
    if (temperature == null || !Number.isFinite(temperature)) {
      return clamp(points[0]?.percent ?? 0, 0, 100);
    }
    const temp = temperature;
    if (temp <= points[0]!.temp_c) return clamp(points[0]!.percent, 0, 100);
    if (temp >= points.at(-1)!.temp_c) return clamp(points.at(-1)!.percent, 0, 100);
    for (let i = 0; i < points.length - 1; i += 1) {
      const a = points[i]!;
      const b = points[i + 1]!;
      if (temp < a.temp_c || temp > b.temp_c) continue;
      if (Math.abs(b.temp_c - a.temp_c) < 1e-6) continue;
      const ratio = clamp((temp - a.temp_c) / (b.temp_c - a.temp_c), 0, 1);
      const interpolated = a.percent + ratio * (b.percent - a.percent);
      return clamp(Math.round(interpolated), 0, 100);
    }
    return clamp(points.at(-1)!.percent, 0, 100);
  }

  function setEnabled(enabled: boolean): void {
    form = { ...form, enabled, manual_percent: enabled ? form.manual_percent : null };
    if (!enabled) manualInput = '';
    dirty = true;
  }

  function setManual(value: string): void {
    manualInput = value;
    const trimmed = value.trim();
    if (!trimmed.length) {
      form = { ...form, manual_percent: null };
      dirty = true;
      return;
    }
    const parsed = Number(trimmed);
    form = { ...form, manual_percent: clamp(Number.isFinite(parsed) ? parsed : 0, 0, 100) };
    dirty = true;
  }

  function handleManualInput(value: string): void {
    setManual(value);
    fixedPercent = clamp(Number(value), 0, 100);
  }

  function handleFixedPercentChange(value: number): void {
    fixedPercent = clamp(Number.isFinite(value) ? value : 0, 0, 100);
  }

  function setControlMode(next: 'curve' | 'fixed'): void {
    if (next === 'fixed') {
      const target = clamp(form.manual_percent ?? fixedPercent, 0, 100);
      fixedPercent = target;
      manualInput = String(target);
      form = { ...form, enabled: true, manual_percent: target };
      dirty = true;
      return;
    }
    manualInput = '';
    form = { ...form, enabled: true, manual_percent: null };
    dirty = true;
  }

  function updateCurve(nextCurve: FanCurvePoint[]): void {
    const normalized = normalizeFan({
      ...form,
      curve: enforceMonotonicCurve(ensureCurveEndpoints(nextCurve, { tempMin: TEMP_MIN_C, tempMax: TEMP_MAX_C }))
    });
    form = normalized;
    selectedPointIndex = clamp(selectedPointIndex, 0, Math.max(0, normalized.curve.length - 1));
    dirty = true;
  }

  function updatePoint(index: number, key: 'temp_c' | 'percent', value: number): void {
    const safeIndex = clamp(index, 0, Math.max(0, form.curve.length - 1));
    const minTemp = (form.curve[safeIndex - 1]?.temp_c ?? TEMP_MIN_C) + 0.1;
    const maxTemp = (form.curve[safeIndex + 1]?.temp_c ?? TEMP_MAX_C) - 0.1;
    const nextCurve = form.curve.map((point, idx) => {
      if (idx !== safeIndex) return point;
      if (key === 'temp_c') {
        if (safeIndex === 0) return { ...point, temp_c: TEMP_MIN_C };
        if (safeIndex === form.curve.length - 1) return { ...point, temp_c: TEMP_MAX_C };
        return { ...point, temp_c: round2(clamp(value, minTemp, maxTemp)) };
      }
      return { ...point, percent: clamp(Math.round(value), form.min_percent, form.max_percent) };
    });
    updateCurve(nextCurve);
  }

  function addPoint(): void {
    const curve = form.curve.length ? [...form.curve] : [...DEFAULT_CURVE];
    const idx = clamp(selectedPointIndex, 0, Math.max(0, curve.length - 1));
    const a = curve[idx] ?? { temp_c: 45, percent: 0 };
    const b = curve[idx + 1] ?? null;
    const nextTemp = b ? (a.temp_c + b.temp_c) / 2 : a.temp_c + 5;
    const nextPercent = clamp(a.percent, form.min_percent, form.max_percent);
    const next = [...curve.slice(0, idx + 1), { temp_c: round2(nextTemp), percent: nextPercent }, ...curve.slice(idx + 1)];
    updateCurve(next);
    selectedPointIndex = clamp(idx + 1, 0, Math.max(0, next.length - 1));
  }

  function removePoint(index: number): void {
    if (index === 0 || index === form.curve.length - 1) return;
    if (form.curve.length <= 1) return;
    const next = form.curve.filter((_, idx) => idx !== index);
    updateCurve(next);
    selectedPointIndex = clamp(selectedPointIndex, 0, Math.max(0, next.length - 1));
  }

  async function refreshFanStatus(options: { silent?: boolean } = {}): Promise<void> {
    const { silent = false } = options;
    if (!silent) refreshingStatus = true;
    try {
      fanStatus = await apiFetch<FanStatus>('/peripherals/fan');
      statusRefreshError = null;
    } catch (err) {
      statusRefreshError = buildErrorMessage({ error: err, fallback: 'Unable to refresh fan status.' });
    } finally {
      if (!silent) refreshingStatus = false;
    }
  }

  function scheduleStatusRefresh(): void {
    if (statusPollId) return;
    statusPollId = setInterval(() => {
      void refreshFanStatus({ silent: true });
    }, FAN_STATUS_REFRESH_MS);
  }

  function stopStatusRefresh(): void {
    if (statusPollId) {
      clearInterval(statusPollId);
      statusPollId = null;
    }
  }

  async function saveFan(): Promise<void> {
    status = null;
    error = null;
    busy = true;
    const payload = fanToDevice(form);
    try {
      await deviceSettingsStore.patch({ requested_by: REQUESTED_BY, fan: payload });
      status = `Saved at ${new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
      dirty = false;
      onRefresh();
      void refreshFanStatus({ silent: true });
      toaster.success({ title: 'Fan settings saved', description: 'Cooling configuration updated.' });
    } catch (err) {
      reportError({
        title: 'Save failed',
        error: err,
        fallback: 'Unable to save fan settings.',
        inline: (message) => {
          error = message;
        }
      });
    } finally {
      busy = false;
    }
  }

  async function resetFanConfig(): Promise<void> {
    settingsBusy = true;
    settingsError = null;
    try {
      await deviceSettingsStore.load({ force: true });
      const incoming = deviceState.data?.fan ? fanFromDevice(deviceState.data.fan) : normalizeFan(DEFAULT_FAN);
      form = incoming;
      manualInput = incoming.manual_percent == null ? '' : String(incoming.manual_percent);
      if (incoming.manual_percent != null) {
        fixedPercent = clamp(incoming.manual_percent, 0, 100);
      }
      dirty = false;
    } catch (err) {
      settingsError = buildErrorMessage({ error: err, fallback: 'Unable to reset fan settings.' });
    } finally {
      settingsBusy = false;
    }
  }

  function describeMode(status: FanStatus | null): string {
    if (!status) return 'Pending';
    if (!status.present) return 'Not present';
    const raw = status.mode?.toLowerCase() ?? '';
    if (raw.includes('disable')) return 'Disabled';
    if (raw.includes('manual') || raw.includes('fixed')) return 'Fixed';
    if (raw.includes('curve')) return 'Curve';
    return status.mode ?? 'Unknown';
  }

  function normalizeStatusMode(raw: string | null | undefined): 'fixed' | 'curve' | 'disabled' | null {
    const value = raw?.toLowerCase() ?? '';
    if (!value) return null;
    if (value.includes('disable')) return 'disabled';
    if (value.includes('manual') || value.includes('fixed')) return 'fixed';
    if (value.includes('curve')) return 'curve';
    return null;
  }

  function fanEquals(a: FanConfig, b: FanConfig): boolean {
    const key = (curve: FanCurvePoint[]) => curve.map((p) => `${p.temp_c}:${p.percent}`).join('|');
    return (
      a.enabled === b.enabled &&
      a.pwm_path === b.pwm_path &&
      (a.tacho_path || '') === (b.tacho_path || '') &&
      a.min_percent === b.min_percent &&
      a.max_percent === b.max_percent &&
      a.manual_percent === b.manual_percent &&
      a.poll_interval_ms === b.poll_interval_ms &&
      Boolean(a.invert_pwm) === Boolean(b.invert_pwm) &&
      key(a.curve) === key(b.curve)
    );
  }
</script>

<SensorModalShell {peripheral} {onClose}>
  {#snippet viewer()}
    <div class="space-y-4">
      <FanStatusPanel
        {fanStatus}
        {displayMode}
        {currentTemp}
        {previewTarget}
        {statusRefreshError}
        {describeMode}
      />
      <FanControlPanel
        {form}
        {mode}
        {displayMode}
        {currentTemp}
        {selectedPointIndex}
        {manualInput}
        {fixedPercent}
        tempMin={TEMP_MIN_C}
        tempMax={TEMP_MAX_C}
        onSelectPoint={(index) => (selectedPointIndex = index)}
        onCurveChange={(curve) => updateCurve(curve)}
        onControlModeChange={(next) => setControlMode(next)}
        onManualInput={(value) => handleManualInput(value)}
        onFixedPercentChange={(value) => handleFixedPercentChange(value)}
      />
    </div>
  {/snippet}

  {#snippet config()}
    <FanConfigPanel
      {form}
      {selectedPointIndex}
      {settingsBusy}
      {busy}
      {dirty}
      {settingsError}
      status={statusMessage}
      {error}
      onSelectPoint={(index) => (selectedPointIndex = index)}
      onAddPoint={() => addPoint()}
      onUpdatePoint={(index, key, value) => updatePoint(index, key, value)}
      onRemovePoint={(index) => removePoint(index)}
      onSave={() => void saveFan()}
      onOpenAdvanced={() => (showAdvanced = true)}
      onRetryLoad={() => void ensureDeviceSettings()}
    />
  {/snippet}

  {#snippet footer()}
    <div class="flex flex-wrap items-center gap-2 text-xs text-surface-500">
      <span>Target {fanStatus?.target_percent ?? '—'}%</span>
      <span class="text-surface-700">·</span>
      <span>Preview {previewTarget}%</span>
    </div>
  {/snippet}
</SensorModalShell>

{#if showAdvanced}
  <FanAdvancedModal
    bind:form
    {settingsBusy}
    {settingsError}
    onClose={() => (showAdvanced = false)}
    onReset={() => void resetFanConfig()}
    onRetryLoad={() => void ensureDeviceSettings()}
    onSetEnabled={(enabled) => setEnabled(enabled)}
    onDirty={() => (dirty = true)}
  />
{/if}
