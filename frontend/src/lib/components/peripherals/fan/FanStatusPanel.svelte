<script lang="ts">
  import type { FanStatus } from '$lib/api/client';

  type Props = {
    fanStatus: FanStatus | null;
    displayMode: 'fixed' | 'curve' | 'disabled' | null;
    currentTemp: number | null;
    previewTarget: number;
    statusRefreshError: string | null;
    describeMode: (status: FanStatus | null) => string;
  };

  const { fanStatus, displayMode, currentTemp, previewTarget, statusRefreshError, describeMode }: Props = $props();
</script>

<div class="space-y-4">
  <div class="grid gap-3 md:grid-cols-3">
    <div class="rounded border border-surface-800 bg-surface-950/30 p-3">
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Mode</p>
      <p class="mt-1 text-lg font-semibold text-surface-50">{describeMode(fanStatus)}</p>
      <p class="text-xs text-surface-500">
        {displayMode === 'fixed' ? 'Fixed duty' : displayMode === 'disabled' ? 'Writes paused' : 'Follows curve'}
      </p>
    </div>
    <div class="rounded border border-surface-800 bg-surface-950/30 p-3">
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">CPU temp</p>
      <p class="mt-1 text-lg font-semibold text-surface-50">{currentTemp == null ? '—' : `${currentTemp.toFixed(1)}°C`}</p>
      <p class="text-xs text-surface-500">Target {fanStatus?.target_percent ?? previewTarget}%</p>
    </div>
    <div class="rounded border border-surface-800 bg-surface-950/30 p-3">
      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">RPM</p>
      <p class="mt-1 text-lg font-semibold text-surface-50">{fanStatus?.rpm ?? '—'}</p>
      <p class="text-xs text-surface-500">Target {fanStatus?.target_percent ?? '—'}%</p>
    </div>
  </div>

  {#if fanStatus?.last_error}
    <div class="rounded border border-error-500/50 bg-error-500/10 px-3 py-2 text-xs text-error-200">
      Fan controller reported an error: {fanStatus.last_error}.
    </div>
  {/if}

  {#if statusRefreshError}
    <div class="rounded border border-warning-500/40 bg-warning-500/10 px-3 py-2 text-xs text-warning-100">
      Failed to refresh fan status: {statusRefreshError}
    </div>
  {/if}
</div>
