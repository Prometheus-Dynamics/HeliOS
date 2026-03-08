<script lang="ts">
  import { onDestroy, onMount } from 'svelte';

  import type { PeripheralEntry } from '$lib/types/devices';
  import type { ImuAxes, ImuStatus } from '$lib/types/systems';

  import MiniSparkline from './MiniSparkline.svelte';
  import SensorModalShell from './SensorModalShell.svelte';
  import PeripheralStats from './PeripheralStats.svelte';

  type Props = {
    peripheral: PeripheralEntry;
    imu: ImuStatus;
    imuError?: string | null;
    onClose: () => void;
    onCalibrate?: (() => void) | null;
  };

  let { peripheral, imu, imuError = null, onClose, onCalibrate = null }: Props = $props();

  const gyro = $derived<ImuAxes>(imu?.gyro ?? { x: 0, y: 0, z: 0 });
  const intervalLabel = $derived(formatInterval(imu?.updateIntervalMs));
  const updatedLabel = $derived(formatTimestamp(imu?.updatedAt));
  const statusLabel = $derived(computeStatus());
  const sourceLabel = $derived(imu?.sources?.accelGyro ?? null);

  let lastHistoryAtMs = $state<number>(0);
  let historyX = $state<number[]>([]);
  let historyY = $state<number[]>([]);
  let historyZ = $state<number[]>([]);
  const HISTORY_MAX = 180;

  let sampleTimer: ReturnType<typeof setInterval> | null = null;

  onMount(() => {
    if (sampleTimer) return;
    sampleTimer = setInterval(() => {
      const updatedAt = imu?.updatedAt ?? null;
      if (!imu?.hasSample && !updatedAt) return;
      const now = Date.now();
      if (now - lastHistoryAtMs < 40) return;
      lastHistoryAtMs = now;
      historyX = [...historyX, gyro.x].slice(-HISTORY_MAX);
      historyY = [...historyY, gyro.y].slice(-HISTORY_MAX);
      historyZ = [...historyZ, gyro.z].slice(-HISTORY_MAX);
    }, 40);
  });

  onDestroy(() => {
    if (sampleTimer) {
      clearInterval(sampleTimer);
      sampleTimer = null;
    }
  });

  function computeStatus(): string {
    if (!imu) return 'No samples yet';
    if (imu.lastError && imu.lastError.trim().length) return `Error · ${imu.lastError}`;
    return imu.hasSample ? 'Active' : 'Idle · awaiting samples';
  }

  function formatInterval(ms?: number | null): string {
    if (!ms || ms <= 0) return 'Unknown rate';
    return `${ms}ms`;
  }

  function formatTimestamp(value?: string | null): string {
    if (!value) return 'Awaiting sample';
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return 'Awaiting sample';
    return date.toLocaleString();
  }

  function formatAxis(value: number): string {
    const precision = Math.abs(value) >= 10 ? 1 : 3;
    return value.toFixed(precision);
  }
</script>

<SensorModalShell {peripheral} {onClose}>
  {#snippet viewer()}
    <div class="space-y-4">
      <div class="rounded-xl border border-surface-800 bg-surface-950/70 p-4 shadow-inner shadow-black/30">
        <div class="flex flex-wrap items-start justify-between gap-3">
          <div class="space-y-1">
            <p class="text-micro uppercase tracking-[0.3em] text-surface-500">3-axis gyroscope</p>
            <p class="text-base font-semibold text-surface-100">{statusLabel}</p>
            {#if imuError && !imu.lastError}
              <p class="text-xs text-error-300">{imuError}</p>
            {/if}
            {#if sourceLabel}
              <p class="text-xs text-surface-400">Source · {sourceLabel}</p>
            {/if}
          </div>
          <div class="text-right text-xs text-surface-500">
            <p>Interval {intervalLabel}</p>
            <p class="text-micro uppercase tracking-[0.25em] text-surface-600">Updated {updatedLabel}</p>
          </div>
        </div>
      </div>

      <div class="rounded-xl border border-surface-800 bg-surface-900/70 p-4 shadow-inner shadow-black/20">
        <p class="text-[0.7rem] uppercase tracking-[0.3em] text-surface-500">Rotation (°/s)</p>
        <dl class="mt-3 space-y-2 text-sm text-surface-100">
          <div class="flex items-center justify-between rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
            <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">X</dt>
            <dd class="font-semibold" style="color: var(--axis-x)">{formatAxis(gyro.x)}</dd>
          </div>
          <div class="flex items-center justify-between rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
            <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Y</dt>
            <dd class="font-semibold" style="color: var(--axis-y)">{formatAxis(gyro.y)}</dd>
          </div>
          <div class="flex items-center justify-between rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
            <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Z</dt>
            <dd class="font-semibold" style="color: var(--axis-z)">{formatAxis(gyro.z)}</dd>
          </div>
        </dl>
      </div>

      <PeripheralStats {peripheral} calibrationLabel="Interval" calibrationValue="5ms" />
    </div>
  {/snippet}

  {#snippet config()}
    <div class="space-y-3">
      <div>
        <h3 class="text-lg font-semibold text-surface-100">Axis graphs</h3>
        <p class="text-sm text-surface-400">Last {HISTORY_MAX} samples (autoscaled per axis).</p>
      </div>

      {#if historyX.length > 1}
        <div class="grid gap-3">
          <MiniSparkline label="X" series={historyX} unit="°/s" color="var(--axis-x)" precision={3} heightClass="h-28" />
          <MiniSparkline label="Y" series={historyY} unit="°/s" color="var(--axis-y)" precision={3} heightClass="h-28" />
          <MiniSparkline label="Z" series={historyZ} unit="°/s" color="var(--axis-z)" precision={3} heightClass="h-28" />
        </div>
      {:else}
        <div class="rounded border border-surface-800/70 bg-surface-900/50 p-3 text-sm text-surface-400">
          No gyroscope samples yet.
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
