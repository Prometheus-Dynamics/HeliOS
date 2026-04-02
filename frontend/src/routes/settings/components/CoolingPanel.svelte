<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { resourceTelemetryStore, type ResourceSample } from '$lib/api/telemetry';
  import { deviceSettingsStore, type DeviceSettingsState } from '../deviceSettingsStore';
  import type { FanConfig, FanCurvePoint, FanStatus } from '../types';
  import { REQUESTED_BY } from '../api';
  import { buildErrorMessage } from '$lib/ui/errorPolicy';
  import { cancelDebounce, scheduleDebounce, type DebounceHandle } from '$lib/utils/debounce';

  const deviceState = $derived($deviceSettingsStore as DeviceSettingsState);
  const telemetry = $derived($resourceTelemetryStore as ResourceSample);
  const FAN_STATUS_REFRESH_MS = 5000;

  const DEFAULT_CURVE: FanCurvePoint[] = [
    { temp_c: 40, percent: 70 },
    { temp_c: 50, percent: 85 },
    { temp_c: 60, percent: 100 }
  ];

  const DEFAULT_FAN: FanConfig = {
    enabled: true,
    pwm_path: 'auto',
    tacho_path: 'auto',
    min_percent: 20,
    max_percent: 100,
    manual_percent: null,
    poll_interval_ms: 5000,
    invert_pwm: true,
    curve: DEFAULT_CURVE
  };

  let form = $state<FanConfig>({ ...DEFAULT_FAN });
  let status = $state<string | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let manualInput = $state('');
  let dirty = $state(false);
  let autoSaveTimer: DebounceHandle = null;
  let refreshingStatus = $state(false);
  let statusRefreshError = $state<string | null>(null);
  let statusPollId: ReturnType<typeof setInterval> | null = null;
  let pendingStatusTimer: DebounceHandle = null;

  const fanStatus = $derived(deviceState.data?.fan_status ?? null);
  const fanSnapshot = $derived(
    deviceState.data?.fan
      ? normalizeFan({
          ...deviceState.data.fan,
          curve: [...(deviceState.data.fan.curve ?? [])]
        })
      : null
  );
  const cpuTemp = $derived(fanStatus?.temperature_c ?? telemetry.cpu.temperature_c ?? null);
  const mode = $derived(form.enabled ? (form.manual_percent != null ? 'manual' : 'curve') : 'disabled');
  const statusMessage = $derived.by(() => status ?? (busy ? 'Saving…' : refreshingStatus ? 'Refreshing…' : 'Autosave active'));

  $effect(() => {
    if (dirty || busy) return;
    const nextForm = deviceState.data?.fan ? normalizeFan(deviceState.data.fan) : { ...DEFAULT_FAN };
    const nextManual = nextForm.manual_percent == null ? '' : String(nextForm.manual_percent);
    if (!fanEquals(form, nextForm) || manualInput !== nextManual) {
      form = nextForm;
      manualInput = nextManual;
    }
  });

  function clamp(value: number, min: number, max: number): number {
    if (!Number.isFinite(value)) return min;
    return Math.min(Math.max(value, min), max);
  }

  function normalizeFan(input: FanConfig): FanConfig {
    const min = clamp(input.min_percent ?? DEFAULT_FAN.min_percent, 0, 100);
    const max = clamp(input.max_percent ?? DEFAULT_FAN.max_percent, min, 100);
    const poll = Math.max(500, Math.round(input.poll_interval_ms || DEFAULT_FAN.poll_interval_ms));
    const manual = input.manual_percent == null ? null : clamp(input.manual_percent, 0, 100);
    const invert = Boolean(input.invert_pwm);
    const curve: FanCurvePoint[] = (input.curve ?? []).length
      ? input.curve
          .filter((point) => Number.isFinite(point.temp_c) && Number.isFinite(point.percent))
          .map((point) => ({
            temp_c: Number(point.temp_c),
            percent: clamp(Number(point.percent), min, max)
          }))
          .sort((a, b) => a.temp_c - b.temp_c)
      : [...DEFAULT_CURVE];
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

  function addPoint(): void {
    const last = form.curve[form.curve.length - 1] ?? { temp_c: 40, percent: form.min_percent };
    const next: FanCurvePoint = { temp_c: last.temp_c + 5, percent: clamp(last.percent, form.min_percent, form.max_percent) };
    form = { ...form, curve: [...form.curve, next] };
    markDirty();
  }

  function removePoint(index: number): void {
    form = { ...form, curve: form.curve.filter((_, idx) => idx !== index) };
    markDirty();
  }

  function updatePoint(index: number, key: 'temp_c' | 'percent', value: number): void {
    const updated = form.curve.map((point, idx) =>
      idx === index ? { ...point, [key]: key === 'percent' ? clamp(value, form.min_percent, form.max_percent) : value } : point
    );
    form = { ...form, curve: updated };
    markDirty();
  }

  function setManual(value: string): void {
    manualInput = value;
    const trimmed = value.trim();
    if (!trimmed.length) {
      form = { ...form, manual_percent: null };
      markDirty();
      return;
    }
    const parsed = Number(trimmed);
    form = { ...form, manual_percent: clamp(Number.isFinite(parsed) ? parsed : 0, 0, 100) };
    markDirty();
  }

  function setMode(next: 'disabled' | 'manual' | 'curve'): void {
    if (next === 'disabled') {
      form = { ...form, enabled: false, manual_percent: null };
      manualInput = '';
      markDirty();
      return;
    }
    if (next === 'manual') {
      const target = form.manual_percent ?? 50;
      form = { ...form, enabled: true, manual_percent: target };
      manualInput = String(target);
      markDirty();
      return;
    }
    form = { ...form, enabled: true, manual_percent: null };
    manualInput = '';
    markDirty();
  }

  function scheduleAutoSave(): void {
    autoSaveTimer = scheduleDebounce(autoSaveTimer, () => {
      autoSaveTimer = null;
      if (busy) {
        scheduleAutoSave();
        return;
      }
      if (!dirty) return;
      void saveFan({ silent: true });
    }, 600);
  }

  function markDirty(): void {
    dirty = true;
    scheduleAutoSave();
  }

  async function refreshFanStatus(options: { silent?: boolean; delayMs?: number } = {}): Promise<void> {
    const { silent = false, delayMs = 0 } = options;
    if (delayMs > 0) {
      await new Promise((resolve) => setTimeout(resolve, delayMs));
    }
    if (!silent) {
      refreshingStatus = true;
    }
    try {
      await deviceSettingsStore.load({ quiet: silent });
      statusRefreshError = null;
    } catch (err) {
      statusRefreshError = buildErrorMessage({ error: err, fallback: 'Unable to refresh fan status.' });
    } finally {
      if (!silent) {
        refreshingStatus = false;
      }
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

  function schedulePostSaveCheck(): void {
    pendingStatusTimer = scheduleDebounce(pendingStatusTimer, () => {
      pendingStatusTimer = null;
      void refreshFanStatus({ silent: true });
    }, 1200);
  }

  onMount(() => {
    void refreshFanStatus({ silent: true });
    scheduleStatusRefresh();
  });

  onDestroy(() => {
    stopStatusRefresh();
    pendingStatusTimer = cancelDebounce(pendingStatusTimer);
    autoSaveTimer = cancelDebounce(autoSaveTimer);
  });

  async function saveFan(options: { silent?: boolean } = {}): Promise<void> {
    const { silent = false } = options;
    busy = true;
    if (!silent) {
      status = null;
      error = null;
    }
    const payload = normalizeFan(form);
    try {
      await deviceSettingsStore.patch({
        requested_by: REQUESTED_BY,
        fan: payload
      });
      status = `Updated at ${new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`;
      dirty = false;
      statusRefreshError = null;
      void refreshFanStatus({ silent: true });
      schedulePostSaveCheck();
    } catch (err) {
      error = buildErrorMessage({ error: err, fallback: 'Unable to save fan settings.' });
    } finally {
      busy = false;
    }
  }

  function describeMode(status: FanStatus | null): string {
    if (!status) return 'pending';
    const mode = status.mode?.toLowerCase() ?? '';
    if (mode.includes('disable')) return 'Disabled';
    if (mode.includes('manual') || mode.includes('fixed')) return 'Manual override';
    if (mode.includes('curve')) return 'Curve';
    return 'Curve';
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

<section class="space-y-4">
  {#if fanStatus?.last_error}
    <div class="rounded border border-error-500/50 bg-error-500/10 px-3 py-2 text-xs text-error-200">
      Fan controller reported an error: {fanStatus.last_error}. Check the PWM path ({fanStatus.path_in_use ?? form.pwm_path}) and permissions.
    </div>
  {/if}

  {#if statusRefreshError}
    <div class="rounded border border-warning-500/40 bg-warning-500/10 px-3 py-2 text-xs text-warning-100">
      Failed to refresh fan status: {statusRefreshError}
    </div>
  {/if}

  <div class="grid gap-4 lg:grid-cols-[minmax(0,0.95fr)_minmax(0,1.05fr)]">
    <article class="space-y-3 rounded border border-surface-800/60 bg-surface-950/30 p-4">
      <header class="flex items-start justify-between gap-2">
        <div>
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Fan status</p>
          <p class="text-sm text-surface-400">Live CM5 cooling loop.</p>
        </div>
        <div class="flex items-center gap-2 text-micro uppercase tracking-[0.25em] text-surface-500">
          {#if fanStatus?.updated_at_ms}
            <span class="text-surface-500">
              {new Date(fanStatus.updated_at_ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
            </span>
          {/if}
        </div>
      </header>
      <div class="grid gap-3 md:grid-cols-3">
        <div class="rounded border border-surface-800 bg-surface-950/30 p-3">
          <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Mode</p>
          <p class="mt-1 text-lg font-semibold text-surface-50">{describeMode(fanStatus)}</p>
          <p class="text-xs text-surface-500">{form.manual_percent == null ? 'Curve responds to CPU temp' : 'Manual duty until cleared'}</p>
        </div>
        <div class="rounded border border-surface-800 bg-surface-950/30 p-3">
          <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Target</p>
          <p class="mt-1 text-lg font-semibold text-surface-50">
            {fanStatus ? `${fanStatus.target_percent ?? 0}%` : '…'}
          </p>
          <p class="text-xs text-surface-500">CPU {cpuTemp == null ? 'pending' : `${cpuTemp.toFixed(1)}°C`}</p>
        </div>
        <div class="rounded border border-surface-800 bg-surface-950/30 p-3">
          <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Path</p>
          <p class="mt-1 text-sm text-surface-50">{fanStatus?.path_in_use ?? form.pwm_path}</p>
          <p class="text-xs text-surface-500">{fanStatus?.last_error ?? 'PWM via hwmon'}</p>
        </div>
        <div class="rounded border border-surface-800 bg-surface-950/30 p-3">
          <p class="text-micro uppercase tracking-[0.3em] text-surface-500">RPM</p>
          <p class="mt-1 text-lg font-semibold text-surface-50">
            {fanStatus?.rpm != null ? fanStatus.rpm : '—'}
          </p>
          <p class="text-xs text-surface-500">Tacho {form.tacho_path || 'unset'}</p>
        </div>
      </div>
    </article>

    <article class="space-y-4 rounded border border-surface-800/60 bg-surface-950/30 p-4">
      <header class="flex items-start justify-between gap-2">
        <div>
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Control</p>
          <p class="text-sm text-surface-400">Enable PWM + apply overrides.</p>
        </div>
        <div class="flex gap-2 text-micro uppercase tracking-[0.25em]">
          <button
            type="button"
            class={`rounded px-2 py-1 ${mode === 'disabled' ? 'bg-error-500/15 text-error-100' : 'bg-surface-800 text-surface-400'}`}
            onclick={() => setMode('disabled')}
          >
            Disabled
          </button>
          <button
            type="button"
            class={`rounded px-2 py-1 ${mode === 'manual' ? 'bg-primary-500/20 text-primary-50' : 'bg-surface-800 text-surface-400'}`}
            onclick={() => setMode('manual')}
          >
            Manual
          </button>
          <button
            type="button"
            class={`rounded px-2 py-1 ${mode === 'curve' ? 'bg-success-500/15 text-success-100' : 'bg-surface-800 text-surface-400'}`}
            onclick={() => setMode('curve')}
          >
            Curve
          </button>
        </div>
      </header>
      <div class="grid gap-3 md:grid-cols-2">
        <label class="space-y-1 text-sm">
          <span class="text-xs uppercase tracking-[0.3em] text-surface-500">PWM path</span>
          <input class="input w-full" bind:value={form.pwm_path} oninput={() => markDirty()} />
          <p class="text-xs text-surface-500">Use auto to select the first hwmon pwm1 entry.</p>
        </label>
        <label class="flex items-center gap-3 text-sm">
          <input type="checkbox" class="h-4 w-4 accent-primary-400" bind:checked={form.enabled} onchange={() => setMode(form.enabled ? 'curve' : 'disabled')} />
          <div>
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Enable fan loop</p>
            <p class="text-xs text-surface-500">Stops writes when disabled.</p>
          </div>
        </label>
        <label class="flex items-center gap-3 text-sm">
          <input
            type="checkbox"
            class="h-4 w-4 accent-primary-400"
            checked={form.invert_pwm}
            onchange={(event) => {
              form = { ...form, invert_pwm: (event.target as HTMLInputElement).checked };
              markDirty();
            }}
          />
          <div>
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Invert PWM</p>
            <p class="text-xs text-surface-500">100% duty = 0% fan speed.</p>
          </div>
        </label>
      </div>
      <div class="grid gap-3 md:grid-cols-3">
        <label class="space-y-1 text-sm">
          <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Min %</span>
          <input class="input w-full" type="number" min="0" max="100" bind:value={form.min_percent} disabled={mode !== 'curve'} oninput={() => markDirty()} />
        </label>
        <label class="space-y-1 text-sm">
          <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Max %</span>
          <input class="input w-full" type="number" min="0" max="100" bind:value={form.max_percent} disabled={mode !== 'curve'} oninput={() => markDirty()} />
        </label>
        <label class="space-y-1 text-sm">
          <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Poll interval (ms)</span>
          <input class="input w-full" type="number" min="500" step="100" bind:value={form.poll_interval_ms} oninput={() => markDirty()} />
        </label>
        <label class="space-y-1 text-sm md:col-span-3">
          <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Tacho path</span>
          <input class="input w-full" bind:value={form.tacho_path} oninput={() => markDirty()} />
          <p class="text-xs text-surface-500">Use auto to select the first fan*_input RPM reader.</p>
        </label>
      </div>

      {#if mode === 'manual'}
        <div class="space-y-2">
          <div class="flex items-center justify-between gap-2">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Manual override</p>
            <button class="btn btn-xs preset-tonal" type="button" onclick={() => setManual('')}>Clear</button>
          </div>
          <div class="grid gap-2 md:grid-cols-[1fr_auto] md:items-center">
            <input
              class="input w-full"
              type="number"
              min="0"
              max="100"
              placeholder="Auto curve"
              value={manualInput}
              oninput={(event) => setManual((event.target as HTMLInputElement).value)}
            />
            <input
              class="range range-primary"
              type="range"
              min="0"
              max="100"
              step="1"
              value={manualInput || '0'}
              oninput={(event) => setManual((event.target as HTMLInputElement).value)}
            />
          </div>
          <p class="text-xs text-surface-500">Leave blank to follow the curve.</p>
        </div>
      {/if}

      {#if error}
        <p class="text-xs text-error-400">{error}</p>
      {/if}
      <div class="flex flex-wrap items-center justify-between gap-3 text-xs text-surface-500">
        <span>{statusMessage}</span>
      </div>
    </article>
  </div>

  {#if mode === 'curve'}
    <article class="space-y-3 rounded border border-surface-800/60 bg-surface-950/30 p-4">
      <header class="flex items-start justify-between gap-3">
        <div>
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Curve</p>
          <p class="text-sm text-surface-400">Map CPU temp to PWM duty.</p>
        </div>
        <button class="btn btn-xs preset-tonal" type="button" onclick={() => addPoint()}>Add point</button>
      </header>
      <div class="overflow-auto">
        <table class="min-w-full text-sm">
          <thead>
            <tr class="border-b border-surface-800 text-micro uppercase tracking-[0.25em] text-surface-500">
              <th class="px-2 py-1 text-left">Temp (°C)</th>
              <th class="px-2 py-1 text-left">Duty (%)</th>
              <th class="px-2 py-1 text-left"></th>
            </tr>
          </thead>
          <tbody class="divide-y divide-surface-800">
            {#each form.curve as point, idx (idx)}
              <tr>
                <td class="px-2 py-2">
                  <input
                    class="input w-full"
                    type="number"
                    min="0"
                    max="120"
                    step="0.5"
                    value={point.temp_c}
                    oninput={(event) => updatePoint(idx, 'temp_c', Number((event.target as HTMLInputElement).value))}
                  />
                </td>
                <td class="px-2 py-2">
                  <input
                    class="input w-full"
                    type="number"
                    min={form.min_percent}
                    max={form.max_percent}
                    value={point.percent}
                    oninput={(event) => updatePoint(idx, 'percent', Number((event.target as HTMLInputElement).value))}
                  />
                </td>
                <td class="px-2 py-2 text-right">
                  <button class="btn btn-xs preset-tonal" type="button" onclick={() => removePoint(idx)} disabled={form.curve.length <= 1}>
                    Remove
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <p class="text-xs text-surface-500">
        Points are sorted on save; the controller linearly interpolates between temperatures and clamps to the min/max duty range.
      </p>
      {#if fanSnapshot}
        <p class="text-xs text-surface-500">Applied revision · min {fanSnapshot.min_percent}% · max {fanSnapshot.max_percent}% · {fanSnapshot.curve.length} points</p>
      {/if}
    </article>
  {/if}
</section>
