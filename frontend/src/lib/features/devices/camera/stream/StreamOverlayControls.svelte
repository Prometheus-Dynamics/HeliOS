<script lang="ts">
  import RangeBandSlider from '$lib/components/controls/RangeBandSlider.svelte';

  type StreamCrop = [number, number, number, number];
  type StreamCrosshair = [number, number];
  type StreamOrderingMode =
    | 'none'
    | 'largest_to_smallest'
    | 'smallest_to_largest'
    | 'top_most'
    | 'bottom_most'
    | 'left_most'
    | 'right_most'
    | 'top_left'
    | 'top_right'
    | 'bottom_left'
    | 'bottom_right'
    | 'center_most'
    | 'crosshair';

  let {
    streamCropGuidesEnabled = $bindable(),
    streamCropApplying,
    streamCropError,
    streamCropWarning,
    normalizedStreamCrop,
    formatCropValue,
    updateStreamCropRange,
    updateStreamCropBand,
    resetStreamCrop,
    streamCrosshairEnabled,
    streamCrosshairApplying,
    streamCrosshairError,
    streamCrosshairWarning,
    normalizedStreamCrosshair,
    updateStreamCrosshair,
    updateStreamCrosshairEnabled,
    resetStreamCrosshair,
    streamOrderingApplying,
    streamOrderingError,
    streamOrderingWarning,
    normalizedStreamOrderingMode,
    streamOrderingModes,
    updateStreamOrderingMode,
    streamCropMin,
    streamCropMax,
    streamCropStep
  }: {
    streamCropGuidesEnabled: boolean;
    streamCropApplying: boolean;
    streamCropError: string | null;
    streamCropWarning: string | null;
    normalizedStreamCrop: StreamCrop;
    formatCropValue: (value: number) => string;
    updateStreamCropRange: (axis: 'x' | 'y', bound: 'min' | 'max', rawValue: number, immediate?: boolean) => void;
    updateStreamCropBand: (axis: 'x' | 'y', minRawValue: number, maxRawValue: number, immediate?: boolean) => void;
    resetStreamCrop: () => void;
    streamCrosshairEnabled: boolean;
    streamCrosshairApplying: boolean;
    streamCrosshairError: string | null;
    streamCrosshairWarning: string | null;
    normalizedStreamCrosshair: StreamCrosshair;
    updateStreamCrosshair: (axis: 'x' | 'y', rawValue: number, immediate?: boolean) => void;
    updateStreamCrosshairEnabled: (value: boolean) => void;
    resetStreamCrosshair: () => void;
    streamOrderingApplying: boolean;
    streamOrderingError: string | null;
    streamOrderingWarning: string | null;
    normalizedStreamOrderingMode: StreamOrderingMode;
    streamOrderingModes: Array<{ value: StreamOrderingMode; label: string }>;
    updateStreamOrderingMode: (value: string, immediate?: boolean) => void;
    streamCropMin: number;
    streamCropMax: number;
    streamCropStep: number;
  } = $props();
</script>

<details class="rounded border border-surface-800 bg-surface-950/40 p-4" open>
  <summary class="cursor-pointer select-none text-sm text-surface-300">Crop</summary>
  <div class="mt-3 space-y-3">
    <p class="text-micro-tight text-surface-500">Normalized stream crop bounds (-1 to 1)</p>
    <div class="flex flex-wrap items-center justify-between gap-3">
      <label class="flex items-center gap-2 text-micro-tight text-surface-400">
        <input
          type="checkbox"
          checked={Boolean(streamCropGuidesEnabled)}
          onchange={(event) => (streamCropGuidesEnabled = event.currentTarget.checked)}
        />
        Show crop guides in preview
      </label>
      <div class="flex items-center gap-2">
        {#if streamCropApplying}
          <span class="text-micro-tight text-surface-500">Applying…</span>
        {/if}
        <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={resetStreamCrop}>
          Full frame
        </button>
      </div>
    </div>

    <div class="space-y-4">
      <div class="space-y-2">
        <div class="flex items-center justify-between text-micro-tight uppercase tracking-[0.22em] text-surface-500">
          <span>Horizontal (x)</span>
          <span>{formatCropValue(normalizedStreamCrop[0])} to {formatCropValue(normalizedStreamCrop[1])}</span>
        </div>
        <div class="flex items-center gap-2">
          <input
            class="input h-9 w-24 text-xs"
            type="number"
            min={streamCropMin}
            max={streamCropMax}
            step={streamCropStep}
            value={normalizedStreamCrop[0]}
            oninput={(event) => updateStreamCropRange('x', 'min', Number(event.currentTarget.value))}
            onchange={(event) => updateStreamCropRange('x', 'min', Number(event.currentTarget.value), true)}
          />
          <div class="flex-1">
            <RangeBandSlider
              min={streamCropMin}
              max={streamCropMax}
              step={streamCropStep}
              valueMin={normalizedStreamCrop[0]}
              valueMax={normalizedStreamCrop[1]}
              gradient="var(--color-primary-500)"
              on:change={(event) => updateStreamCropBand('x', Number(event.detail.min), Number(event.detail.max))}
            />
          </div>
          <input
            class="input h-9 w-24 text-xs"
            type="number"
            min={streamCropMin}
            max={streamCropMax}
            step={streamCropStep}
            value={normalizedStreamCrop[1]}
            oninput={(event) => updateStreamCropRange('x', 'max', Number(event.currentTarget.value))}
            onchange={(event) => updateStreamCropRange('x', 'max', Number(event.currentTarget.value), true)}
          />
        </div>
      </div>

      <div class="space-y-2">
        <div class="flex items-center justify-between text-micro-tight uppercase tracking-[0.22em] text-surface-500">
          <span>Vertical (y)</span>
          <span>{formatCropValue(normalizedStreamCrop[2])} to {formatCropValue(normalizedStreamCrop[3])}</span>
        </div>
        <div class="flex items-center gap-2">
          <input
            class="input h-9 w-24 text-xs"
            type="number"
            min={streamCropMin}
            max={streamCropMax}
            step={streamCropStep}
            value={normalizedStreamCrop[2]}
            oninput={(event) => updateStreamCropRange('y', 'min', Number(event.currentTarget.value))}
            onchange={(event) => updateStreamCropRange('y', 'min', Number(event.currentTarget.value), true)}
          />
          <div class="flex-1">
            <RangeBandSlider
              min={streamCropMin}
              max={streamCropMax}
              step={streamCropStep}
              valueMin={normalizedStreamCrop[2]}
              valueMax={normalizedStreamCrop[3]}
              gradient="var(--color-primary-500)"
              on:change={(event) => updateStreamCropBand('y', Number(event.detail.min), Number(event.detail.max))}
            />
          </div>
          <input
            class="input h-9 w-24 text-xs"
            type="number"
            min={streamCropMin}
            max={streamCropMax}
            step={streamCropStep}
            value={normalizedStreamCrop[3]}
            oninput={(event) => updateStreamCropRange('y', 'max', Number(event.currentTarget.value))}
            onchange={(event) => updateStreamCropRange('y', 'max', Number(event.currentTarget.value), true)}
          />
        </div>
      </div>
    </div>
    {#if streamCropError}
      <p class="text-xs text-error-300">{streamCropError}</p>
    {/if}
    {#if streamCropWarning}
      <p class="text-xs text-amber-300">{streamCropWarning}</p>
    {/if}
  </div>
</details>

<details class="rounded border border-surface-800 bg-surface-950/40 p-4">
  <summary class="cursor-pointer select-none text-sm text-surface-300">Crosshair</summary>
  <div class="mt-3 space-y-3">
    <p class="text-micro-tight text-surface-500">Normalized crosshair position (-1 to 1)</p>
    <p class="text-micro-tight text-surface-500">Crosshair is rendered by the active pipeline graph on the stream output frame.</p>
    <div class="flex flex-wrap items-center justify-between gap-3">
      <label class="flex items-center gap-2 text-micro-tight text-surface-400">
        <input
          type="checkbox"
          checked={Boolean(streamCrosshairEnabled)}
          onchange={(event) => updateStreamCrosshairEnabled(event.currentTarget.checked)}
        />
        Draw crosshair in output
      </label>
      <div class="flex items-center gap-2">
        {#if streamCrosshairApplying}
          <span class="text-micro-tight text-surface-500">Applying…</span>
        {/if}
        <button class="btn btn-3xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={resetStreamCrosshair}>
          Center
        </button>
      </div>
    </div>
    <div class="space-y-4">
      <div class="space-y-2">
        <div class="flex items-center justify-between text-micro-tight uppercase tracking-[0.22em] text-surface-500">
          <span>Horizontal (x)</span>
          <span>{formatCropValue(normalizedStreamCrosshair[0])}</span>
        </div>
        <div class="flex items-center gap-2">
          <div class="flex-1">
            <input
              class="h-2 w-full cursor-pointer accent-primary-500"
              type="range"
              min={streamCropMin}
              max={streamCropMax}
              step={streamCropStep}
              value={normalizedStreamCrosshair[0]}
              oninput={(event) => updateStreamCrosshair('x', Number(event.currentTarget.value))}
              onchange={(event) => updateStreamCrosshair('x', Number(event.currentTarget.value), true)}
            />
          </div>
          <input
            class="input h-9 w-24 text-xs"
            type="number"
            min={streamCropMin}
            max={streamCropMax}
            step={streamCropStep}
            value={normalizedStreamCrosshair[0]}
            oninput={(event) => updateStreamCrosshair('x', Number(event.currentTarget.value))}
            onchange={(event) => updateStreamCrosshair('x', Number(event.currentTarget.value), true)}
          />
        </div>
      </div>

      <div class="space-y-2">
        <div class="flex items-center justify-between text-micro-tight uppercase tracking-[0.22em] text-surface-500">
          <span>Vertical (y)</span>
          <span>{formatCropValue(normalizedStreamCrosshair[1])}</span>
        </div>
        <div class="flex items-center gap-2">
          <div class="flex-1">
            <input
              class="h-2 w-full cursor-pointer accent-primary-500"
              type="range"
              min={streamCropMin}
              max={streamCropMax}
              step={streamCropStep}
              value={normalizedStreamCrosshair[1]}
              oninput={(event) => updateStreamCrosshair('y', Number(event.currentTarget.value))}
              onchange={(event) => updateStreamCrosshair('y', Number(event.currentTarget.value), true)}
            />
          </div>
          <input
            class="input h-9 w-24 text-xs"
            type="number"
            min={streamCropMin}
            max={streamCropMax}
            step={streamCropStep}
            value={normalizedStreamCrosshair[1]}
            oninput={(event) => updateStreamCrosshair('y', Number(event.currentTarget.value))}
            onchange={(event) => updateStreamCrosshair('y', Number(event.currentTarget.value), true)}
          />
        </div>
      </div>
    </div>

    {#if streamCrosshairError}
      <p class="text-xs text-error-300">{streamCrosshairError}</p>
    {/if}
    {#if streamCrosshairWarning}
      <p class="text-xs text-amber-300">{streamCrosshairWarning}</p>
    {/if}
  </div>
</details>

<details class="rounded border border-surface-800 bg-surface-950/40 p-4" open>
  <summary class="cursor-pointer select-none text-sm text-surface-300">Ordering</summary>
  <div class="mt-3 space-y-3">
    <p class="text-micro-tight text-surface-500">Order detections before downstream targeting/filtering.</p>
    <div class="flex items-center justify-between gap-2">
      <span class="text-micro-tight uppercase tracking-[0.22em] text-surface-500">Mode</span>
      {#if streamOrderingApplying}
        <span class="text-micro-tight text-surface-500">Applying…</span>
      {/if}
    </div>
    <select
      class="input h-10 w-full"
      value={normalizedStreamOrderingMode}
      onchange={(event) => updateStreamOrderingMode(event.currentTarget.value, true)}
    >
      {#each streamOrderingModes as option (option.value)}
        <option value={option.value}>{option.label}</option>
      {/each}
    </select>
    {#if streamOrderingError}
      <p class="text-xs text-error-300">{streamOrderingError}</p>
    {/if}
    {#if streamOrderingWarning}
      <p class="text-xs text-amber-300">{streamOrderingWarning}</p>
    {/if}
  </div>
</details>
