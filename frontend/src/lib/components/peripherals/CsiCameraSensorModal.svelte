<script lang="ts">
  import type { PeripheralEntry } from '$lib/types/devices';

  import SensorModalShell from './SensorModalShell.svelte';
  import PeripheralStats from './PeripheralStats.svelte';

  type Props = {
    peripheral: PeripheralEntry;
    onClose: () => void;
  };

  let { peripheral, onClose }: Props = $props();

  const title = $derived(peripheral.name?.trim() || 'CSI camera sensor');
  const typeLabel = $derived(peripheral.type?.trim() || 'Camera');
  const hardwareId = $derived(peripheral.hardwareId?.trim() || null);
  const identityLabel = $derived(peripheral.identity?.display?.trim() || null);
  const statusLabel = $derived(peripheral.status?.trim() || 'Unknown');
  const pathLabel = $derived(peripheral.driverCameraId?.trim() || null);
</script>

<SensorModalShell {peripheral} {onClose}>
  {#snippet viewer()}
    <div class="space-y-4">
      <div class="rounded-xl border border-surface-800 bg-surface-950/70 p-4 shadow-inner shadow-black/30">
        <p class="text-micro uppercase tracking-[0.3em] text-surface-500">CSI / I2C camera sensor</p>
        <h2 class="mt-2 text-lg font-semibold text-surface-50">{title}</h2>
        <p class="mt-2 text-sm text-surface-300">
          This is the raw image sensor detected on the I2C bus. To use it for streaming, register a capture session from the Cameras panel.
        </p>
      </div>

      <div class="rounded-xl border border-surface-800 bg-surface-900/70 p-4 shadow-inner shadow-black/20">
        <p class="text-[0.7rem] uppercase tracking-[0.3em] text-surface-500">Details</p>
        <dl class="mt-3 grid gap-2 text-sm text-surface-200 sm:grid-cols-2">
          <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
            <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Type</dt>
            <dd class="text-surface-100">{typeLabel}</dd>
          </div>
          <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
            <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Status</dt>
            <dd class="text-surface-100">{statusLabel}</dd>
          </div>
          {#if hardwareId}
            <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2 sm:col-span-2">
              <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Hardware</dt>
              <dd class="truncate text-surface-100">{hardwareId}</dd>
            </div>
          {/if}
          {#if identityLabel}
            <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2 sm:col-span-2">
              <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Identity</dt>
              <dd class="truncate text-surface-100">{identityLabel}</dd>
            </div>
          {/if}
          {#if pathLabel}
            <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2 sm:col-span-2">
              <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Path</dt>
              <dd class="truncate font-mono text-xs text-surface-200">{pathLabel}</dd>
            </div>
          {/if}
        </dl>
      </div>

      <PeripheralStats {peripheral} showCalibration={false} />
    </div>
  {/snippet}

  {#snippet config()}
    <div class="space-y-2">
      <h3 class="text-lg font-semibold text-surface-100">Properties</h3>
      <dl class="mt-3 grid gap-2 text-sm text-surface-200">
        <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
          <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Name</dt>
          <dd class="text-surface-100">{title}</dd>
        </div>
        <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
          <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Type</dt>
          <dd class="text-surface-100">{typeLabel}</dd>
        </div>
        {#if hardwareId}
          <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
            <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Hardware ID</dt>
            <dd class="truncate text-surface-100">{hardwareId}</dd>
          </div>
        {/if}
        {#if identityLabel}
          <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
            <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Identity</dt>
            <dd class="truncate text-surface-100">{identityLabel}</dd>
          </div>
        {/if}
        {#if pathLabel}
          <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
            <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Path</dt>
            <dd class="truncate font-mono text-xs text-surface-200">{pathLabel}</dd>
          </div>
        {/if}
        <div class="flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-2">
          <dt class="text-micro uppercase tracking-[0.25em] text-surface-500">Status</dt>
          <dd class="text-surface-100">{statusLabel}</dd>
        </div>
      </dl>
    </div>
  {/snippet}

  {#snippet footer()}
    <div class="flex flex-wrap items-center gap-3"></div>
  {/snippet}
</SensorModalShell>
