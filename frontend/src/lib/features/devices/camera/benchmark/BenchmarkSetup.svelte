<script lang="ts">
  import type { ProbedBackend, ProbedDevice } from '$lib/ts-bindings/http/client';

  type Props = {
    benchTargetFps: number;
    benchSampleFrames: number;
    benchSampleTimeoutMs: number;
    backend: ProbedBackend | null;
    device: ProbedDevice | null;
    selectedModesCount: number;
    totalModes: number;
    includedFormats: string[];
    includedResolutions: string[];
    availableFormats: string[];
    availableResolutions: string[];
    onToggleFormat: (format: string) => void;
    onToggleResolution: (resolution: string) => void;
    onIncludeAllFormats: () => void;
    onExcludeAllFormats: () => void;
    onIncludeAllResolutions: () => void;
    onExcludeAllResolutions: () => void;
    onStart: () => void;
    onCancel: () => void;
    isRunning: boolean;
  };

  let {
    benchTargetFps = $bindable(120),
    benchSampleFrames = $bindable(100),
    benchSampleTimeoutMs = $bindable(1500),
    backend,
    device,
    selectedModesCount,
    totalModes,
    includedFormats,
    includedResolutions,
    availableFormats,
    availableResolutions,
    onToggleFormat,
    onToggleResolution,
    onIncludeAllFormats,
    onExcludeAllFormats,
    onIncludeAllResolutions,
    onExcludeAllResolutions,
    onStart,
    onCancel,
    isRunning
  }: Props = $props();
</script>

<div class="rounded border border-surface-800 bg-surface-900/70 p-4 space-y-3">
  <div class="text-sm text-surface-300">
    Benchmarks selected sensor modes against all available decoders and RG24 encoders, and persists results to disk.
  </div>

  <div class="grid gap-4 lg:grid-cols-2">
    <div class="space-y-3">
      <div class="flex flex-wrap items-end gap-3">
        <label class="text-sm w-full max-w-[10rem]">
          <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Target FPS</span>
          <input class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2" type="number" min="1" step="1" bind:value={benchTargetFps} />
        </label>
        <label class="text-sm w-full max-w-[10rem]">
          <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Frames</span>
          <input class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2" type="number" min="1" step="1" bind:value={benchSampleFrames} />
        </label>
        <label class="text-sm w-full max-w-[10rem]">
          <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Timeout (ms)</span>
          <input class="mt-1 w-full border border-surface-700 bg-surface-900/70 px-3 py-2" type="number" min="100" step="50" bind:value={benchSampleTimeoutMs} />
        </label>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <button
          class="btn btn-sm preset-filled-primary-500 uppercase tracking-[0.3em]"
          type="button"
          onclick={onStart}
          disabled={!backend || !device || selectedModesCount === 0}
        >
          Run sensor benchmark
        </button>
        <button
          class="btn btn-sm preset-tonal"
          type="button"
          onclick={onCancel}
          disabled={!isRunning}
        >
          Cancel
        </button>
        <span class="text-2xs text-surface-500">Stops conflicting streams and restores them afterwards.</span>
      </div>
    </div>

    <div class="space-y-3">
      <div class="flex items-center justify-between">
        <div class="text-2xs uppercase tracking-[0.3em] text-surface-500">Filters</div>
        <div class="text-2xs text-surface-500">{selectedModesCount}/{totalModes} modes selected</div>
      </div>

      <div class="space-y-2">
        <div class="flex items-center justify-between gap-2">
          <div class="text-2xs uppercase tracking-[0.3em] text-surface-500">Formats</div>
          <div class="flex gap-2">
            <button class="btn btn-sm preset-tonal" type="button" onclick={onIncludeAllFormats}>Include all</button>
            <button class="btn btn-sm preset-tonal" type="button" onclick={onExcludeAllFormats}>Exclude all</button>
          </div>
        </div>
        <div class="flex flex-wrap gap-2">
          {#each availableFormats as fmt (fmt)}
            <button
              type="button"
              class={`rounded-md border px-3 py-1.5 text-xs transition ${
                includedFormats.includes(fmt)
                  ? 'border-primary-400 bg-primary-500/10 text-primary-50'
                  : 'border-surface-800 bg-surface-900 text-surface-200 hover:border-surface-700'
              }`}
              onclick={() => onToggleFormat(fmt)}
            >
              {fmt}
            </button>
          {/each}
        </div>
      </div>

      <div class="space-y-2">
        <div class="flex items-center justify-between gap-2">
          <div class="text-2xs uppercase tracking-[0.3em] text-surface-500">Resolutions</div>
          <div class="flex gap-2">
            <button class="btn btn-sm preset-tonal" type="button" onclick={onIncludeAllResolutions}>Include all</button>
            <button class="btn btn-sm preset-tonal" type="button" onclick={onExcludeAllResolutions}>Exclude all</button>
          </div>
        </div>
        <div class="flex flex-wrap gap-2">
          {#each availableResolutions as res (res)}
            <button
              type="button"
              class={`rounded-md border px-3 py-1.5 text-xs transition ${
                includedResolutions.includes(res)
                  ? 'border-primary-400 bg-primary-500/10 text-primary-50'
                  : 'border-surface-800 bg-surface-900 text-surface-200 hover:border-surface-700'
              }`}
              onclick={() => onToggleResolution(res)}
            >
              {res}
            </button>
          {/each}
        </div>
      </div>
    </div>
  </div>
</div>
