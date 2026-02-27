<script lang="ts">
  import ModalShell from '$lib/components/ui/ModalShell.svelte';
  import type { SensorBenchCodecStat, SensorBenchListItem, SensorBenchModeResult, SensorBenchResult } from './sensorBenchTypes';

  type Props = {
    open: boolean;
    sensorBenchError: string | null;
    sensorBenchLoading: boolean;
    sensorBenchRuns: SensorBenchListItem[];
    sensorBenchSelectedId: string | null;
    sensorBenchSelectedResult: SensorBenchResult | null;
    sensorBenchSelection: SensorBenchModeResult | null;
    sensorBenchBestDecoder: SensorBenchCodecStat | null;
    sensorBenchBestEncoder: SensorBenchCodecStat | null;
    onClose: () => void;
    onRefresh: () => void;
    onUseDecoder: (implementation: string) => void;
    onUseEncoder: (implementation: string) => void;
  };

  let {
    open,
    sensorBenchError,
    sensorBenchLoading,
    sensorBenchRuns,
    sensorBenchSelectedId = $bindable<string | null>(null),
    sensorBenchSelectedResult,
    sensorBenchSelection,
    sensorBenchBestDecoder,
    sensorBenchBestEncoder,
    onClose,
    onRefresh,
    onUseDecoder,
    onUseEncoder
  }: Props = $props();
</script>

{#if open}
  <ModalShell
    open
    size="xl"
    className="z-[60]"
    panelClassName="border-surface-800 bg-surface-900/95 text-surface-50 shadow-2xl backdrop-blur"
    onClose={onClose}
  >
    {#snippet header()}
      <div class="min-w-0">
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Sensor benchmark</p>
        <p class="text-lg font-semibold text-surface-50">Saved results</p>
        <p class="text-sm text-surface-400">Filter to this device/backend and pick a run to inspect.</p>
      </div>
    {/snippet}
    {#snippet actions()}
      <button class="btn btn-ghost" type="button" onclick={onClose}>Close</button>
    {/snippet}
    {#snippet children()}
      <div class="max-h-[calc(90dvh-10rem)] overflow-y-auto pr-1 space-y-3">
        <div class="rounded border border-surface-800 bg-surface-950/40 p-3 space-y-2">
          <div class="flex items-center justify-between">
            <div class="text-2xs uppercase tracking-[0.3em] text-surface-500">Saved runs</div>
            <button class="btn btn-sm preset-tonal" type="button" onclick={onRefresh} disabled={sensorBenchLoading}>
              Refresh
            </button>
          </div>

          {#if sensorBenchError}
            <div class="text-sm text-error-100">{sensorBenchError}</div>
          {/if}

          <select
            class="w-full border border-surface-700 bg-surface-900/70 px-3 py-2"
            bind:value={sensorBenchSelectedId}
            disabled={sensorBenchLoading || sensorBenchRuns.length === 0}
          >
            {#each sensorBenchRuns as item (item.summary.benchmark_id)}
              <option value={item.summary.benchmark_id}>
                {item.summary.completed_at}{item.summary.canceled ? ' (canceled)' : ''}
              </option>
            {/each}
          </select>
        </div>

        {#if sensorBenchSelectedResult}
          <div class="rounded border border-surface-800 bg-surface-950/40 p-3 space-y-2">
            <div class="text-2xs uppercase tracking-[0.3em] text-surface-500">Selection</div>
            {#if sensorBenchSelection}
              <div class="text-sm text-surface-200">
                <span class="font-mono">{sensorBenchSelection.format}</span>
                <span class="text-surface-500">{sensorBenchSelection.resolution}</span>
              </div>
              <div class="grid gap-3 sm:grid-cols-2 text-xs text-surface-300">
                <div class="space-y-1">
                  <div class="text-surface-500">Best decoder</div>
                  <div class="font-mono">{sensorBenchBestDecoder?.implementation ?? '—'}</div>
                  {#if sensorBenchBestDecoder?.implementation}
                    <button
                      class="btn btn-sm preset-tonal"
                      type="button"
                      onclick={() => onUseDecoder(sensorBenchBestDecoder?.implementation ?? '')}
                    >
                      Use this decoder
                    </button>
                  {/if}
                </div>
                <div class="space-y-1">
                  <div class="text-surface-500">Best encoder (RG24)</div>
                  <div class="font-mono">{sensorBenchBestEncoder?.implementation ?? '—'}</div>
                  {#if sensorBenchBestEncoder?.implementation}
                    <button
                      class="btn btn-sm preset-tonal"
                      type="button"
                      onclick={() => onUseEncoder(sensorBenchBestEncoder?.implementation ?? '')}
                    >
                      Use this encoder
                    </button>
                  {/if}
                </div>
              </div>
            {:else}
              <div class="text-sm text-surface-500">No matching entry for the selected format/resolution.</div>
            {/if}
          </div>
        {/if}
      </div>
    {/snippet}
  </ModalShell>
{/if}
