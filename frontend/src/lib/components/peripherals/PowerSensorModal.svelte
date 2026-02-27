<script lang="ts">
  import type { PeripheralEntry } from '$lib/types/devices';

  import MiniSparkline from './MiniSparkline.svelte';
  import SensorModalShell from './SensorModalShell.svelte';
  import PeripheralStats from './PeripheralStats.svelte';

  type PowerSource = {
    label: string;
    bus?: number | null;
    address?: string | null;
    watts?: number | null;
    volts?: number | null;
    amps?: number | null;
    shuntVolts?: number | null;
  };

  type PowerStatus = {
    watts: number | null;
    volts: number | null;
    amps: number | null;
    updatedAt: string | null;
    sources: PowerSource[];
    errors: string[];
  };

  type Props = {
    peripheral: PeripheralEntry;
    status: PowerStatus;
    powerError?: string | null;
    onClose: () => void;
    onCalibrate?: (() => void) | null;
    onRefresh?: () => void;
  };

  let { peripheral, status, powerError = null, onClose, onCalibrate = null, onRefresh }: Props = $props();

  const hasSample = $derived(
    Boolean(status.updatedAt) ||
      (typeof status.watts === 'number' && Number.isFinite(status.watts)) ||
      (typeof status.volts === 'number' && Number.isFinite(status.volts)) ||
      (typeof status.amps === 'number' && Number.isFinite(status.amps))
  );

  let lastSampleKey = $state<string | null>(null);
  let graphsAutoScale = $state(true);
  let wattsHistory = $state<number[]>([]);
  let voltsHistory = $state<number[]>([]);
  let ampsHistory = $state<number[]>([]);
  let timeHistoryMs = $state<number[]>([]);
  const HISTORY_MAX = 180;

  function finiteNumber(value: number | null | undefined): number | null {
    return typeof value === 'number' && Number.isFinite(value) ? value : null;
  }

  $effect(() => {
    const sampleKey = [
      status.updatedAt ?? '',
      status.watts ?? '',
      status.volts ?? '',
      status.amps ?? '',
      status.sources?.length ?? 0
    ].join('|');
    if (sampleKey === lastSampleKey) return;
    lastSampleKey = sampleKey;

    const timestampMs = status.updatedAt ? Date.parse(status.updatedAt) : Date.now();
    timeHistoryMs = [...timeHistoryMs, Number.isFinite(timestampMs) ? timestampMs : Date.now()].slice(-HISTORY_MAX);

    const voltsSample = finiteNumber(status.volts);
    const ampsSample = finiteNumber(status.amps);
    const wattsSample = finiteNumber(status.watts) ?? (voltsSample != null && ampsSample != null ? voltsSample * ampsSample : null);

    wattsHistory = [...wattsHistory, wattsSample ?? wattsHistory.at(-1) ?? 0].slice(-HISTORY_MAX);
    voltsHistory = [...voltsHistory, voltsSample ?? voltsHistory.at(-1) ?? 0].slice(-HISTORY_MAX);
    ampsHistory = [...ampsHistory, ampsSample ?? ampsHistory.at(-1) ?? 0].slice(-HISTORY_MAX);
  });

  function formatNumber(value: number | null | undefined, precision = 2): string {
    if (value == null || Number.isNaN(value)) return '--';
    return value.toFixed(precision);
  }

  function formatTimestamp(value?: string | null): string {
    if (!value) return 'Awaiting sample';
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return 'Awaiting sample';
    return date.toLocaleString();
  }
</script>

<SensorModalShell {peripheral} {onClose}>
  {#snippet viewer()}
    <div class="space-y-4">
      <div class="rounded-xl border border-surface-800 bg-surface-950/70 p-4 shadow-inner shadow-black/30">
        <div class="flex flex-wrap items-start justify-between gap-3">
          <div class="space-y-1">
            <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Power sensor</p>
            {#if status.errors?.length}
              <p class="text-sm text-error-300">{status.errors[0]}</p>
            {:else}
              <p class="text-base font-semibold text-surface-100">{hasSample ? 'Active' : 'Idle · awaiting samples'}</p>
            {/if}
            {#if powerError}
              <p class="text-xs text-error-300">{powerError}</p>
            {/if}
          </div>
          <div class="text-right text-xs text-surface-500">
            <p>Interval {peripheral.interval ?? '—'}</p>
            <p class="text-micro uppercase tracking-[0.25em] text-surface-600">Updated {formatTimestamp(status.updatedAt)}</p>
          </div>
        </div>
      </div>

      <div class="rounded-xl border border-surface-800 bg-surface-900/70 p-4 shadow-inner shadow-black/20">
        <p class="text-[0.7rem] uppercase tracking-[0.3em] text-surface-500">Readings</p>
        <dl class="mt-3 space-y-2 text-sm text-surface-100">
          <div class="flex items-center justify-between rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
            <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Watts</dt>
            <dd class="font-semibold text-primary-100">{status.watts != null ? `${formatNumber(status.watts, 1)} W` : '--'}</dd>
          </div>
          <div class="flex items-center justify-between rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
            <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Volts</dt>
            <dd class="font-semibold text-secondary-100">{status.volts != null ? `${formatNumber(status.volts, 2)} V` : '--'}</dd>
          </div>
          <div class="flex items-center justify-between rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
            <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Amps</dt>
            <dd class="font-semibold text-amber-100">{status.amps != null ? `${formatNumber(status.amps, 3)} A` : '--'}</dd>
          </div>
        </dl>
      </div>

      <div class="flex items-center justify-between text-xs text-surface-500">
        <button class="btn btn-xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={onRefresh}>
          Refresh
        </button>
      </div>

      <PeripheralStats {peripheral} calibrationLabel="Interval" calibrationValue="5ms" />
    </div>
  {/snippet}

  {#snippet config()}
    <div class="space-y-3">
      <div>
        <h3 class="text-lg font-semibold text-surface-100">Graphs</h3>
        <p class="text-sm text-surface-400">Last {HISTORY_MAX} samples.</p>
      </div>

      {#if hasSample}
        <label class="flex items-center gap-2 text-xs text-surface-300">
          <input class="checkbox checkbox-xs" type="checkbox" bind:checked={graphsAutoScale} />
          Autoscale
        </label>
        <div class="grid gap-3">
          <MiniSparkline
            label="Watts"
            series={wattsHistory}
            timestampsMs={timeHistoryMs}
            autoScale={graphsAutoScale}
            unit="W"
            colorClass="text-rose-300"
            precision={2}
            heightClass="h-32"
          />
          <MiniSparkline
            label="Volts"
            series={voltsHistory}
            timestampsMs={timeHistoryMs}
            autoScale={graphsAutoScale}
            unit="V"
            colorClass="text-primary-300"
            precision={3}
            heightClass="h-32"
          />
          <MiniSparkline
            label="Amps"
            series={ampsHistory}
            timestampsMs={timeHistoryMs}
            autoScale={graphsAutoScale}
            unit="A"
            colorClass="text-secondary-300"
            precision={3}
            heightClass="h-32"
          />
        </div>
      {:else}
        <div class="rounded border border-surface-800/70 bg-surface-900/50 p-3 text-sm text-surface-400">
          No power samples yet.
        </div>
      {/if}
    </div>
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
