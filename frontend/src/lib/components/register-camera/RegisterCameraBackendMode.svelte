<script lang="ts">
  import type { Interval, Mode, ProbedDevice } from '$lib/ts-bindings/http/client';

  type Props = {
    device: ProbedDevice | null;
    selectedBackendIndex: number;
    selectedFormat: string | null;
    selectedResolutionKey: string | null;
    selectedInterval: Interval | null;
    currentModes: () => Mode[];
    formats: () => string[];
    resolutionsForSelectedFormat: () => Mode[];
    intervalsForSelected: () => Interval[];
    formatLabel: (format: string | null) => string;
    resolutionLabel: (mode: Mode) => string;
    resolutionKey: (mode: Mode) => string;
    intervalToFps: (interval: Interval) => string | null;
    onBackendChange: (index: number) => void;
    onFormatChange: (format: string) => void;
    onResolutionChange: (resolutionKey: string) => void;
    onIntervalChange: (index: number) => void;
  };

  const {
    device,
    selectedBackendIndex,
    selectedFormat,
    selectedResolutionKey,
    selectedInterval,
    currentModes,
    formats,
    resolutionsForSelectedFormat,
    intervalsForSelected,
    formatLabel,
    resolutionLabel,
    resolutionKey,
    intervalToFps,
    onBackendChange,
    onFormatChange,
    onResolutionChange,
    onIntervalChange
  }: Props = $props();
</script>

<div class="rounded-lg border border-surface-800 bg-surface-950/60">
  <div class="flex items-center justify-between px-4 py-3">
    <span class="text-xs uppercase tracking-[0.3em] text-surface-500">Backend & mode</span>
    <span class="text-xs text-surface-400">Pick how this camera will be captured</span>
  </div>
  <div class="space-y-3 px-4 pb-4">
    {#if (device?.backends?.length ?? 0) === 0}
      <p class="text-sm text-surface-400">No backends reported for this device.</p>
    {:else}
      <div class="flex flex-wrap gap-2">
        {#each device?.backends ?? [] as backend, index (`${backend.kind ?? 'backend'}:${index}`)}
          <button
            type="button"
            class={`rounded-md border px-3 py-2 text-sm transition ${
              selectedBackendIndex === index
                ? 'border-primary-400 bg-primary-500/10 text-primary-50'
                : 'border-surface-800 bg-surface-900 text-surface-200 hover:border-surface-700'
            }`}
            onclick={() => onBackendChange(index)}
          >
            {backend.kind ?? `Backend ${index + 1}`}
          </button>
        {/each}
      </div>
    {/if}

    {#if currentModes().length}
      <div class="space-y-2">
        <p class="text-micro uppercase tracking-[0.2em] text-surface-500">Format</p>
        {#if formats().length}
          <div class="flex flex-wrap gap-2">
            {#each formats() as fmt, index (`${fmt}:${index}`)}
              <button
                type="button"
                class={`rounded-md border px-3 py-2 text-sm transition ${
                  fmt === selectedFormat
                    ? 'border-primary-400 bg-primary-500/10 text-primary-50'
                    : 'border-surface-800 bg-surface-900 text-surface-200 hover:border-surface-700'
                }`}
                onclick={() => onFormatChange(fmt)}
              >
                {formatLabel(fmt)}
              </button>
            {/each}
          </div>
        {:else}
          <p class="text-sm text-surface-400">No formats reported.</p>
        {/if}
      </div>

      <div class="space-y-2">
        <p class="text-micro uppercase tracking-[0.2em] text-surface-500">Resolution</p>
        {#if resolutionsForSelectedFormat().length}
          <div class="flex flex-wrap gap-2">
            {#each resolutionsForSelectedFormat() as mode, index (`${resolutionKey(mode) ?? 'resolution'}:${index}`)}
              <button
                type="button"
                class={`rounded-md border px-3 py-2 text-sm transition ${
                  resolutionKey(mode) === selectedResolutionKey
                    ? 'border-primary-400 bg-primary-500/10 text-primary-50'
                    : 'border-surface-800 bg-surface-900 text-surface-200 hover:border-surface-700'
                }`}
                onclick={() => onResolutionChange(resolutionKey(mode))}
              >
                {resolutionLabel(mode)}
              </button>
            {/each}
          </div>
        {:else}
          <p class="text-sm text-surface-400">No resolutions reported for this format.</p>
        {/if}
      </div>

      <div class="space-y-2">
        <p class="text-micro uppercase tracking-[0.2em] text-surface-500">Interval / FPS</p>
        {#if intervalsForSelected().length}
          <div class="flex flex-wrap gap-2">
            {#each intervalsForSelected() as interval, idx (`${interval.numerator ?? 0}:${interval.denominator ?? 0}:${idx}`)}
              <button
                type="button"
                class={`rounded-md border px-3 py-2 text-sm transition ${
                  selectedInterval === interval
                    ? 'border-primary-400 bg-primary-500/10 text-primary-50'
                    : 'border-surface-800 bg-surface-900 text-surface-200 hover:border-surface-700'
                }`}
                onclick={() => onIntervalChange(idx)}
              >
                {intervalToFps(interval) ?? 'Unknown FPS'}
              </button>
            {/each}
          </div>
        {:else}
          <p class="text-sm text-surface-400">No intervals reported for this resolution.</p>
        {/if}
      </div>
    {:else}
      <p class="text-sm text-surface-400">No capture modes reported for this backend.</p>
    {/if}
  </div>
</div>
