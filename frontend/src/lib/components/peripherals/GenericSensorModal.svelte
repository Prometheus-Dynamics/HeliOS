<script lang="ts">
  import type { PeripheralEntry } from '$lib/types/devices';

  import SensorModalShell from './SensorModalShell.svelte';
  import PeripheralStats from './PeripheralStats.svelte';

  type GenericSensorMetric = {
    index?: number | string | null;
    kind?: string | null;
    label?: string | null;
    value: number;
    units?: string | null;
  };

  type GenericSensorChannel = {
    name?: string | null;
    metrics?: GenericSensorMetric[] | null;
  };

  type GenericSensorDevfreq = {
    current_freq_hz?: number | null;
    target_freq_hz?: number | null;
    min_freq_hz?: number | null;
    max_freq_hz?: number | null;
    load_percent?: number | null;
    governor?: string | null;
    busy_time_us?: number | null;
    total_time_us?: number | null;
  };

  type GenericSensorTelemetry = {
    status?: string | null;
    message?: string | null;
    collected_at_ms?: number | null;
    device_path?: string | null;
    usb_device_path?: string | null;
    class_path?: string | null;
    debugfs_path?: string | null;
    devfreq?: GenericSensorDevfreq | null;
    hwmon?: GenericSensorChannel[] | null;
  };

  type Props = {
    peripheral: PeripheralEntry;
    onClose: () => void;
    onCalibrate?: (() => void) | null;
  };

  let { peripheral, onClose, onCalibrate = null }: Props = $props();

  const telemetry = $derived((peripheral.telemetry ?? null) as GenericSensorTelemetry | null);
  const telemetryStatus = $derived(telemetry?.status ?? telemetry?.message ?? null);
  const hwmonSensors = $derived(telemetry?.hwmon ?? []);

  function formatTelemetryTimestamp(ms: number | null | undefined): string {
    if (!ms || !Number.isFinite(ms)) return 'Unknown';
    const date = new Date(ms);
    if (Number.isNaN(date.getTime())) return 'Unknown';
    return date.toLocaleString();
  }

  function formatHz(value: number | null | undefined): string {
    if (value == null || !Number.isFinite(value)) return '—';
    const units = ['Hz', 'kHz', 'MHz', 'GHz'];
    let freq = value;
    let unitIdx = 0;
    while (freq >= 1000 && unitIdx < units.length - 1) {
      freq /= 1000;
      unitIdx += 1;
    }
    return `${freq.toFixed(freq >= 10 || unitIdx === 0 ? 0 : 1)} ${units[unitIdx]}`;
  }

  function formatPercent(value: number | null | undefined): string {
    if (value == null || !Number.isFinite(value)) return '—';
    return `${value.toFixed(Math.abs(value) >= 100 ? 0 : 1)}%`;
  }

  function formatMicroseconds(value: number | null | undefined): string {
    if (value == null || !Number.isFinite(value)) return '—';
    if (value < 1000) return `${value.toFixed(0)} µs`;
    const ms = value / 1000;
    return `${ms.toFixed(ms >= 10 ? 0 : 1)} ms`;
  }

  function formatMetricValue(value: number, units: string | null | undefined): string {
    const precision = Math.abs(value) >= 100 ? 0 : 2;
    const suffix = units && units.trim().length ? ` ${units}` : '';
    return `${value.toFixed(precision)}${suffix}`;
  }
</script>

<SensorModalShell {peripheral} {onClose}>
  {#snippet viewer()}
    {#if telemetry}
      <div class="space-y-3">
        <div class="rounded border border-surface-800/70 bg-surface-950/50 p-3">
          <div class="flex flex-wrap items-center justify-between gap-3">
            <div>
              <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Telemetry status</p>
              <p class="text-sm text-surface-200">{telemetryStatus ?? 'Reported without status'}</p>
            </div>
            <div class="text-right text-[0.7rem] text-surface-500">
              <p>Collected {formatTelemetryTimestamp(telemetry.collected_at_ms)}</p>
              {#if telemetry.collected_at_ms}
                <p class="text-micro uppercase tracking-[0.25em] text-surface-600">Epoch {telemetry.collected_at_ms}</p>
              {/if}
            </div>
          </div>
          <div class="mt-3 grid gap-2 text-xs text-surface-300 sm:grid-cols-2">
            {#if telemetry.device_path}
              <p class="truncate text-surface-400">Device · {telemetry.device_path}</p>
            {/if}
            {#if telemetry.usb_device_path}
              <p class="truncate text-surface-400">USB · {telemetry.usb_device_path}</p>
            {/if}
            {#if telemetry.class_path}
              <p class="truncate text-surface-400">Class · {telemetry.class_path}</p>
            {/if}
            {#if telemetry.debugfs_path}
              <p class="truncate text-surface-400">DebugFS · {telemetry.debugfs_path}</p>
            {/if}
            {#if !telemetry.device_path && !telemetry.usb_device_path && !telemetry.class_path && !telemetry.debugfs_path}
              <p class="text-surface-500">No device paths reported.</p>
            {/if}
          </div>
        </div>

        {#if telemetry.devfreq}
          <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
            <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Frequency</p>
            <dl class="mt-2 grid gap-2 text-sm text-surface-200 sm:grid-cols-2">
              <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-900/60 px-2 py-1">
                <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Current</dt>
                <dd>{formatHz(telemetry.devfreq.current_freq_hz)}</dd>
              </div>
              <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-900/60 px-2 py-1">
                <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Target</dt>
                <dd>{formatHz(telemetry.devfreq.target_freq_hz)}</dd>
              </div>
              <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-900/60 px-2 py-1">
                <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Min</dt>
                <dd>{formatHz(telemetry.devfreq.min_freq_hz)}</dd>
              </div>
              <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-900/60 px-2 py-1">
                <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Max</dt>
                <dd>{formatHz(telemetry.devfreq.max_freq_hz)}</dd>
              </div>
              <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-900/60 px-2 py-1">
                <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Load</dt>
                <dd>{formatPercent(telemetry.devfreq.load_percent)}</dd>
              </div>
              <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-900/60 px-2 py-1">
                <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Governor</dt>
                <dd>{telemetry.devfreq.governor ?? '—'}</dd>
              </div>
              <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-900/60 px-2 py-1">
                <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Busy</dt>
                <dd>{formatMicroseconds(telemetry.devfreq.busy_time_us)}</dd>
              </div>
              <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-900/60 px-2 py-1">
                <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Total</dt>
                <dd>{formatMicroseconds(telemetry.devfreq.total_time_us)}</dd>
              </div>
            </dl>
          </div>
        {/if}

        <div class="rounded border border-surface-800/70 bg-surface-950/40 p-3">
          <div class="flex items-center justify-between gap-2">
            <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Sensors</p>
            <p class="text-micro uppercase tracking-[0.25em] text-surface-600">
              {hwmonSensors.length} channel{hwmonSensors.length === 1 ? '' : 's'}
            </p>
          </div>
          {#if hwmonSensors.length}
            <div class="mt-2 space-y-2">
              {#each hwmonSensors as sensor, idx (sensor.name ?? `sensor-${idx}`)}
                <div class="rounded border border-surface-800/70 bg-surface-900/50 p-2">
                  <div class="flex items-center justify-between text-sm text-surface-200">
                    <p class="font-semibold">{sensor.name ?? `Sensor ${idx + 1}`}</p>
                    {#if sensor.metrics?.length}
                      <span class="text-micro uppercase tracking-[0.25em] text-surface-500">
                        {sensor.metrics.length} reading{sensor.metrics.length === 1 ? '' : 's'}
                      </span>
                    {/if}
                  </div>
                  {#if sensor.metrics?.length}
                    <dl class="mt-2 grid gap-2 text-sm text-surface-200 sm:grid-cols-2">
                      {#each sensor.metrics as metric (metric.index ?? metric.kind)}
                        <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-900/60 px-2 py-1">
                          <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">
                            {metric.label ?? metric.kind}
                          </dt>
                          <dd class="text-surface-100">{formatMetricValue(metric.value, metric.units)}</dd>
                        </div>
                      {/each}
                    </dl>
                  {:else}
                    <p class="text-xs text-surface-500">No metrics reported for this channel.</p>
                  {/if}
                </div>
              {/each}
            </div>
          {:else}
            <p class="mt-2 text-sm text-surface-500">No hwmon telemetry reported.</p>
          {/if}
        </div>

        <PeripheralStats {peripheral} calibrationLabel="Interval" calibrationValue="5ms" />
      </div>
    {:else}
      <div class="grid min-h-[16rem] place-content-center gap-2 rounded-xl border border-dashed border-surface-700/70 bg-surface-900 p-6 text-center shadow-inner shadow-black/20">
        <p class="text-sm font-semibold text-surface-300">No dedicated preview</p>
        <p class="text-xs text-surface-500">Live telemetry controls are unavailable for this peripheral.</p>
      </div>
      <PeripheralStats {peripheral} calibrationLabel="Interval" calibrationValue="5ms" />
    {/if}
  {/snippet}

  {#snippet config()}
    <h3 class="text-lg font-semibold text-surface-100">Configuration</h3>
    <p class="text-sm text-surface-400">This peripheral does not expose configurable telemetry from the dashboard.</p>
  {/snippet}

  {#snippet footer()}
    <div class="flex flex-wrap items-center gap-3">
      {#if onCalibrate}
        <button class="btn btn-xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={onCalibrate}>
          Calibrate
        </button>
      {/if}
    </div>
  {/snippet}
</SensorModalShell>
