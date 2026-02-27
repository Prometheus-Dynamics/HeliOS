<script lang="ts">
  type HeatmapStreamOption = { id: string; label: string; hasMetrics: boolean };
  type HeatmapViewMode = { id: string; label: string; description: string };
  type GpuOverlaySummary = {
    totalGpuNodes: number;
    sharedSegments: number;
    segments: Array<{ id: string; color: string; nodes: string[]; labels: string[]; shared?: boolean }>;
    ungroupedLabels: string[];
  };

  type Props = {
    heatmapEnabled: boolean;
    gpuOverlayEnabled: boolean;
    heatmapStreamId: string | null;
    heatmapStreamOptions: HeatmapStreamOption[];
    selectedHeatmapStreamLabel: string | null;
    heatmapViewModes: HeatmapViewMode[];
    heatmapViewMode: string;
    heatmapActiveFilterCount: number;
    heatmapFilterQuery: string;
    heatmapMinAverageInput: string;
    heatmapMinSamplesInput: string;
    heatmapExcludedCount: number;
    graphHeatmap: { maxValue?: number } | null;
    metricsUpdatedAt: number | null;
    gpuOverlaySummary: GpuOverlaySummary;
    onHeatmapStreamChange: (event: Event) => void;
    onHeatmapSelectorEnter: () => void;
    onHeatmapSelectorLeave: () => void;
    onSetHeatmapViewMode: (mode: string) => void;
    onClearHeatmapFilters: () => void;
    formatHeatDuration: (value: number | null | undefined) => string;
    formatTimestamp: (value: number | null | undefined) => string;
  };

  let {
    heatmapEnabled,
    gpuOverlayEnabled,
    heatmapStreamId,
    heatmapStreamOptions,
    selectedHeatmapStreamLabel,
    heatmapViewModes,
    heatmapViewMode,
    heatmapActiveFilterCount,
    heatmapFilterQuery = $bindable(''),
    heatmapMinAverageInput = $bindable(''),
    heatmapMinSamplesInput = $bindable(''),
    heatmapExcludedCount,
    graphHeatmap,
    metricsUpdatedAt,
    gpuOverlaySummary,
    onHeatmapStreamChange,
    onHeatmapSelectorEnter,
    onHeatmapSelectorLeave,
    onSetHeatmapViewMode,
    onClearHeatmapFilters,
    formatHeatDuration,
    formatTimestamp
  }: Props = $props();
</script>

{#if heatmapEnabled}
  <div class="pointer-events-none absolute right-4 top-4 z-20 flex flex-col items-end gap-2">
    <div
      class="pointer-events-auto rounded border border-white/15 bg-surface-950/85 px-3 py-2 text-micro-tight uppercase tracking-[0.22em] text-surface-100 shadow-2xl backdrop-blur"
      onpointerenter={onHeatmapSelectorEnter}
      onpointerleave={onHeatmapSelectorLeave}
    >
      <select
        class="w-52 rounded border border-white/30 bg-surface-900/85 px-3 py-1.5 text-[0.8rem] font-bold uppercase tracking-[0.2em] text-white focus:border-primary-300 focus:outline-none"
        value={heatmapStreamId ?? ''}
        onchange={onHeatmapStreamChange}
        disabled={heatmapStreamOptions.length === 0}
        title="Select the camera stream used for the metrics heatmap."
        onfocus={onHeatmapSelectorEnter}
        onblur={onHeatmapSelectorLeave}
        onmouseenter={onHeatmapSelectorEnter}
        onclick={onHeatmapSelectorEnter}
      >
        {#if heatmapStreamOptions.length === 0}
          <option value="" selected>No active streams</option>
        {:else}
          <option value="" disabled hidden>Select stream…</option>
          {#each heatmapStreamOptions as option (option.id)}
            <option value={option.id}>{option.label}</option>
          {/each}
        {/if}
      </select>
    </div>
  </div>
{/if}

{#if heatmapEnabled || gpuOverlayEnabled}
  <div class="pointer-events-none absolute left-4 top-4 z-20 flex flex-col items-start gap-3">
    {#if heatmapEnabled}
      <div class="pointer-events-auto rounded border border-white/15 bg-surface-950/85 px-3 py-3 text-[0.58rem] uppercase tracking-[0.16em] text-surface-100 shadow-2xl backdrop-blur min-w-[19rem] max-w-[26rem]">
        <div class="flex items-center gap-2">
          <span>Fast</span>
          <span class="inline-block h-2 w-24 rounded-full bg-gradient-to-r from-sky-400 via-indigo-500 to-rose-500" aria-hidden="true"></span>
          <span>Slow</span>
        </div>
        <div class="mt-2 flex min-h-[2.4rem] flex-col text-[0.52rem] font-semibold normal-case tracking-normal text-surface-200/90">
          <span class="uppercase tracking-[0.18em] text-surface-300">
            Peak Δt {graphHeatmap ? formatHeatDuration(graphHeatmap.maxValue) : '—'}
          </span>
          <span class={`text-[0.48rem] ${selectedHeatmapStreamLabel ? 'text-surface-400/80' : 'text-surface-500/70'}`}>
            Stream {selectedHeatmapStreamLabel ?? '—'}
          </span>
          <span class="text-[0.48rem] text-surface-500">
            {#if metricsUpdatedAt}
              Updated {formatTimestamp(metricsUpdatedAt)}
            {:else}
              Waiting for samples…
            {/if}
          </span>
        </div>
        <div class="mt-3 flex flex-wrap gap-1.5 text-[0.48rem] font-semibold uppercase tracking-[0.11em]">
          {#each heatmapViewModes as mode (mode.id)}
            <button
              type="button"
              class={`rounded border px-1.5 py-[3px] text-[0.48rem] tracking-[0.11em] transition ${
                heatmapViewMode === mode.id
                  ? 'border-white/80 bg-white/10 text-white'
                  : 'border-white/20 text-surface-300 hover:border-white/40 hover:text-white'
              }`}
              title={mode.description}
              onclick={() => onSetHeatmapViewMode(mode.id)}
            >
              {mode.label}
            </button>
          {/each}
        </div>
        <div class="mt-3 rounded border border-white/10 bg-surface-900/60 px-3 py-2 text-[0.7rem] font-semibold normal-case leading-snug text-surface-100/90">
          <div class="flex items-center justify-between text-micro-tight uppercase tracking-[0.18em] text-surface-300">
            <span>Filters</span>
            {#if heatmapActiveFilterCount > 0}
              <span class="rounded-full border border-primary-400/60 bg-primary-500/10 px-2 py-[1px] text-micro-tight font-semibold text-primary-100">
                {heatmapActiveFilterCount} active
              </span>
            {/if}
          </div>
          <div class="mt-2 space-y-2">
            <label class="flex flex-col gap-1 text-[0.58rem] uppercase tracking-[0.14em] text-surface-400">
              <span>Search</span>
              <input
                class="rounded border border-white/15 bg-surface-950/70 px-2 py-1 text-[0.7rem] font-semibold text-white placeholder:text-surface-500 focus:border-primary-300 focus:outline-none"
                type="text"
                value={heatmapFilterQuery}
                oninput={(event) => (heatmapFilterQuery = (event.currentTarget as HTMLInputElement).value)}
                placeholder="Name or node ID"
              />
            </label>
            <div class="grid grid-cols-2 gap-2 text-[0.58rem] uppercase tracking-[0.14em] text-surface-400">
              <label class="flex flex-col gap-1">
                <span>Min avg (ms)</span>
                <input
                  class="rounded border border-white/15 bg-surface-950/70 px-2 py-1 text-[0.7rem] font-semibold text-white placeholder:text-surface-500 focus:border-primary-300 focus:outline-none"
                  type="number"
                  inputmode="decimal"
                  min="0"
                  step="0.1"
                  value={heatmapMinAverageInput}
                  oninput={(event) => (heatmapMinAverageInput = (event.currentTarget as HTMLInputElement).value)}
                  placeholder="e.g. 2.5"
                />
              </label>
              <label class="flex flex-col gap-1">
                <span>Min samples</span>
                <input
                  class="rounded border border-white/15 bg-surface-950/70 px-2 py-1 text-[0.7rem] font-semibold text-white placeholder:text-surface-500 focus:border-primary-300 focus:outline-none"
                  type="number"
                  inputmode="numeric"
                  min="0"
                  step="1"
                  value={heatmapMinSamplesInput}
                  oninput={(event) => (heatmapMinSamplesInput = (event.currentTarget as HTMLInputElement).value)}
                  placeholder="e.g. 10"
                />
              </label>
            </div>
          </div>
          <div class="mt-2 border-t border-white/10 pt-2">
            <div class="flex items-center gap-2 text-[0.62rem] uppercase tracking-[0.16em] text-surface-300">
              <span>Exclusions</span>
              {#if heatmapExcludedCount > 0}
                <span class="rounded-full border border-white/20 px-2 py-[1px] text-[0.62rem] font-semibold text-white">
                  {heatmapExcludedCount}
                </span>
              {/if}
            </div>
            <div class="mt-2 flex flex-wrap gap-2 text-[0.62rem]">
              <button
                type="button"
                class="rounded border border-white/15 px-2.5 py-1 font-semibold uppercase tracking-[0.2em] text-surface-200 transition hover:border-white/30 hover:text-white"
                onclick={onClearHeatmapFilters}
              >
                Reset filters
              </button>
            </div>
          </div>
        </div>
      </div>
    {/if}
    {#if gpuOverlayEnabled}
      <div class="pointer-events-auto rounded border border-emerald-300/25 bg-surface-950/85 px-3 py-3 text-[0.58rem] uppercase tracking-[0.16em] text-emerald-50 shadow-2xl backdrop-blur min-w-[18rem] max-w-[26rem]">
        <div class="flex items-center justify-between gap-2 text-micro font-semibold">
          <span class="flex items-center gap-2">
            <span class="inline-block h-2 w-2 rounded-full bg-emerald-300 shadow-[0_0_0_4px_rgba(52,211,153,0.2)]"></span>
            GPU buffers
          </span>
          <span class="rounded-full border border-emerald-400/60 bg-emerald-500/10 px-2 py-[1px] text-micro-tight font-semibold text-emerald-100">
            Overlay
          </span>
        </div>
        <p class="mt-1 text-[0.48rem] font-semibold normal-case tracking-normal text-surface-300">
          {#if gpuOverlaySummary.totalGpuNodes === 0}
            No GPU-capable nodes detected
          {:else}
            {gpuOverlaySummary.totalGpuNodes} GPU-capable node{gpuOverlaySummary.totalGpuNodes === 1 ? '' : 's'} ·
            {gpuOverlaySummary.sharedSegments} shared segment{gpuOverlaySummary.sharedSegments === 1 ? '' : 's'}
          {/if}
        </p>
        {#if gpuOverlaySummary.segments.length > 0}
          <div class="mt-3 max-h-[60vh] max-h-[60svh] max-h-[60dvh] space-y-2 overflow-y-auto pr-1 text-[0.7rem] normal-case leading-snug text-surface-100/90">
            {#each gpuOverlaySummary.segments as segment (segment.id)}
              <div class="rounded border border-emerald-200/20 bg-surface-900/50 px-2.5 py-2">
                <div class="flex items-center gap-2 text-[0.62rem] uppercase tracking-[0.18em] text-emerald-100">
                  <span
                    class="inline-block h-2.5 w-2.5 rounded-full"
                    style={`background:${segment.color}; box-shadow: 0 0 0 5px color-mix(in srgb, ${segment.color} 18%, transparent);`}
                    aria-hidden="true"
                  ></span>
                  <span class="text-[0.68rem] font-semibold">Segment {segment.id}</span>
                  <span class="text-micro-tight text-surface-400">{segment.nodes.length} {segment.nodes.length === 1 ? 'node' : 'nodes'}</span>
                  {#if segment.shared}
                    <span class="rounded-full border border-emerald-400/50 bg-emerald-500/10 px-2 py-[1px] text-micro-tight font-semibold text-emerald-100">
                      Shared
                    </span>
                  {/if}
                </div>
                <p class="mt-1 text-[0.72rem] font-semibold leading-snug text-surface-100">
                  {segment.labels.join(', ')}
                </p>
              </div>
            {/each}
          </div>
        {:else}
          <p class="mt-2 text-[0.7rem] font-semibold normal-case text-surface-100/80">
            No GPU buffer groups were detected for this plan.
          </p>
        {/if}
        {#if gpuOverlaySummary.ungroupedLabels.length > 0}
          <div class="mt-2 border-t border-white/10 pt-2 text-[0.64rem] font-semibold normal-case text-surface-300">
            <div class="uppercase tracking-[0.18em] text-surface-400">Ungrouped GPU nodes</div>
            <p class="mt-1 text-[0.7rem] text-surface-100">
              {gpuOverlaySummary.ungroupedLabels.join(', ')}
            </p>
          </div>
        {/if}
      </div>
    {/if}
  </div>
{/if}
