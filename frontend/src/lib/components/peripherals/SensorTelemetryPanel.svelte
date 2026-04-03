<script lang="ts">
  import type { PeripheralEntry } from '$lib/types/devices';
  import type { Snippet } from 'svelte';
  import MiniSparkline from './MiniSparkline.svelte';

  type Props = {
    peripheral: PeripheralEntry;
    sampleRate: string;
    streamEnabled: boolean;
    orientationLock: boolean;
    rollSeries: number[];
    pitchSeries: number[];
    yawSeries: number[];
    onSampleRateChange?: (value: string) => void;
    onStreamEnabledChange?: (value: boolean) => void;
    onOrientationLockChange?: (value: boolean) => void;
    firmware?: Snippet;
  };

  const {
    peripheral,
    sampleRate,
    streamEnabled,
    orientationLock,
    rollSeries,
    pitchSeries,
    yawSeries,
    onSampleRateChange,
    onStreamEnabledChange,
    onOrientationLockChange,
    firmware
  }: Props = $props();
</script>

<div class="space-y-3">
  <h3 class="text-lg font-semibold text-surface-100">Telemetry</h3>
  <label class="grid gap-1 text-sm text-surface-200">
    <span class="text-surface-300">Sample rate</span>
    <select
      class="input border border-surface-700 bg-surface-950"
      value={sampleRate}
      onchange={(event) => onSampleRateChange?.(event.currentTarget.value)}
    >
      <option value="10ms">100 Hz (10ms)</option>
      <option value="20ms">50 Hz (20ms)</option>
      <option value="40ms">25 Hz (40ms)</option>
      <option value="100ms">10 Hz (100ms)</option>
      <option value="200ms">5 Hz (200ms)</option>
      <option value={peripheral.interval ?? '100ms'}>Match telemetry ({peripheral.interval ?? 'unknown'})</option>
    </select>
  </label>

  <label class="grid grid-cols-[auto_1fr] items-center gap-3 text-sm text-surface-200">
    <input
      type="checkbox"
      checked={streamEnabled}
      onchange={(event) => onStreamEnabledChange?.(event.currentTarget.checked)}
    />
    <span class="text-surface-200">Enable live telemetry stream</span>
  </label>

  <label class="grid grid-cols-[auto_1fr] items-center gap-3 text-sm text-surface-200">
    <input
      type="checkbox"
      checked={orientationLock}
      onchange={(event) => onOrientationLockChange?.(event.currentTarget.checked)}
    />
    <span class="text-surface-200">Lock orientation reference frame</span>
  </label>

  {#if firmware}
    {@render firmware()}
  {/if}

  <div class="mt-4 space-y-3">
    <p class="text-[0.7rem] uppercase tracking-[0.3em] text-surface-500">Orientation graphs</p>
    <div class="grid gap-3">
      <MiniSparkline label="Roll" series={rollSeries} unit="°" color="var(--axis-roll)" precision={1} heightClass="h-28" />
      <MiniSparkline label="Pitch" series={pitchSeries} unit="°" color="var(--axis-pitch)" precision={1} heightClass="h-28" />
      <MiniSparkline label="Yaw" series={yawSeries} unit="°" color="var(--axis-yaw)" precision={1} heightClass="h-28" />
    </div>
  </div>
</div>
