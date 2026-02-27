<script lang="ts">
  import type { PeripheralEntry } from '$lib/types/devices';

  type Props = {
    peripheral: PeripheralEntry | null;
    showInterval?: boolean;
    showCalibration?: boolean;
    calibrationLabel?: string | null;
    calibrationValue?: string | null;
  };

  const {
    peripheral,
    showInterval = true,
    showCalibration = true,
    calibrationLabel = null,
    calibrationValue = null
  }: Props = $props();
</script>

<dl class="grid grid-cols-[repeat(auto-fit,minmax(12rem,1fr))] gap-3 text-sm text-surface-300">
  <div class="flex flex-col gap-1">
    <dt class="text-micro uppercase tracking-[0.3em] text-surface-500">Status</dt>
    <dd class="text-base text-surface-200">{peripheral?.status ?? '—'}</dd>
  </div>
  <div class="flex flex-col gap-1">
    <dt class="text-micro uppercase tracking-[0.3em] text-surface-500">Driver</dt>
    <dd class="text-base text-surface-200">{peripheral?.driverNamespace ?? '—'}</dd>
  </div>
  <div class="flex flex-col gap-1">
    <dt class="text-micro uppercase tracking-[0.3em] text-surface-500">Type</dt>
    <dd class="text-base text-surface-200">{peripheral?.type ?? '—'}</dd>
  </div>
  <div class="flex flex-col gap-1">
    <dt class="text-micro uppercase tracking-[0.3em] text-surface-500">Hardware ID</dt>
    <dd class="text-base text-surface-200">{peripheral?.hardwareId ?? '—'}</dd>
  </div>
  {#if showInterval}
    <div class="flex flex-col gap-1">
      <dt class="text-micro uppercase tracking-[0.3em] text-surface-500">Sample interval</dt>
      <dd class="text-base text-surface-200">{peripheral?.interval ?? '—'}</dd>
    </div>
  {/if}
  {#if showCalibration}
    <div class="flex flex-col gap-1">
      <dt class="text-micro uppercase tracking-[0.3em] text-surface-500">
        {calibrationLabel ?? 'Last calibration'}
      </dt>
      <dd class="text-base text-surface-200">{calibrationValue ?? peripheral?.lastCalibration ?? 'Never'}</dd>
    </div>
  {/if}
</dl>
