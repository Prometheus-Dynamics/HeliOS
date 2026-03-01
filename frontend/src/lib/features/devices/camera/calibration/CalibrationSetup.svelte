<script lang="ts">
  type CalibrationImage = {
    name: string;
    size_bytes: number;
    content_type: string;
    stream_id?: string;
    kind?: string;
    captured_at_ms?: number;
  };

  type CalibrationSetupProps = {
    calibrationTool: 'lens' | 'color';
    calibrationBoard: {
      squaresX: number;
      squaresY: number;
      squareMm: number;
      markerMm: number;
      marginMm: number;
      dpi: number;
      dictionary?: string;
    };
    calibrationLensModel: 'pinhole' | 'fisheye';
    setCalibrationLensModel: (value: 'pinhole' | 'fisheye') => void;
    boardPreviewUrl: string;
    onOpenBoardCreator: () => void;
    colorChartPngUrl: string;
    colorChartPatchMm: number;
    colorChartMarginMm: number;
    colorChartDpi: number;
    onOpenColorChart: () => void;
    streamUuid: string | null;
    guidedModeEnabled: boolean;
    guidedModeBusy: boolean;
    guidedAccumulateLive: boolean;
    setGuidedModeEnabled: (enabled: boolean) => void;
    resetGuidedCoverage: () => void;
    setGuidedAccumulateLive: (enabled: boolean) => void;
    calibrationOwnPhotosOnly: boolean;
    onSetCalibrationOwnPhotosOnly: (value: boolean) => void;
    calibrationSelected: Record<string, boolean>;
    calibrationLoading: boolean;
    calibrationSolving: boolean;
    calibrationApplying: boolean;
    calibrationDeleting: boolean;
    selectedCount: number;
    chartSelectedName: string | null;
    visibleSnapshots: CalibrationImage[];
    apiPath: (path: string) => string;
    onTakeSnapshot: () => void;
    onToggleSnapshot: (name: string) => void;
    onSelectAllSnapshots: () => void;
    onClearSelected: () => void;
    onDeleteSelectedSnapshots: () => void;
    onDeleteSnapshot: (name: string) => void;
    onOpenPreview: (item: CalibrationImage) => void;
    ipaChartImage: string;
  };

  let {
    calibrationTool = $bindable('lens'),
    calibrationBoard,
    calibrationLensModel,
    setCalibrationLensModel,
    boardPreviewUrl,
    onOpenBoardCreator,
    colorChartPngUrl,
    colorChartPatchMm,
    colorChartMarginMm,
    colorChartDpi,
    onOpenColorChart,
    streamUuid,
    guidedModeEnabled,
    guidedModeBusy,
    guidedAccumulateLive,
    setGuidedModeEnabled,
    resetGuidedCoverage,
    setGuidedAccumulateLive,
    calibrationOwnPhotosOnly,
    onSetCalibrationOwnPhotosOnly,
    calibrationSelected,
    calibrationLoading,
    calibrationSolving,
    calibrationApplying,
    calibrationDeleting,
    selectedCount,
    chartSelectedName,
    visibleSnapshots,
    apiPath,
    onTakeSnapshot,
    onToggleSnapshot,
    onSelectAllSnapshots,
    onClearSelected,
    onDeleteSelectedSnapshots,
    onDeleteSnapshot,
    onOpenPreview,
    ipaChartImage
  }: CalibrationSetupProps = $props();

  export type $$Props = CalibrationSetupProps;

</script>

<div class="space-y-4">
  <section class="rounded border border-surface-800/60 bg-surface-900/40 p-4">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div class="min-w-0">
        <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Calibration</p>
        <p class="mt-1 text-sm font-semibold text-surface-100">Pick a workflow</p>
      </div>
      <div class="flex flex-wrap gap-2 rounded border border-surface-800/60 bg-surface-900/60 p-1">
        <button
          type="button"
          class={`btn btn-xs ${calibrationTool === 'lens' ? 'preset-filled' : 'preset-tonal'}`}
          onclick={() => (calibrationTool = 'lens')}
          aria-pressed={calibrationTool === 'lens'}
        >
          Lens
        </button>
        <button
          type="button"
          class={`btn btn-xs ${calibrationTool === 'color' ? 'preset-filled' : 'preset-tonal'}`}
          onclick={() => (calibrationTool = 'color')}
          aria-pressed={calibrationTool === 'color'}
        >
          Color
        </button>
      </div>
    </div>

    <div class={`mt-4 grid gap-3 ${calibrationTool === 'lens' ? 'xl:grid-cols-[minmax(0,1.15fr)_minmax(0,0.85fr)]' : ''}`}>
      <div class="rounded border border-surface-800/60 bg-surface-950/10 p-3">
        <div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_auto] md:items-start">
          <div class="min-w-0">
            <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">
              {calibrationTool === 'lens' ? 'ChArUco board' : 'Color chart'}
            </p>
            {#if calibrationTool === 'lens'}
              <p class="mt-1 text-xs text-surface-500">
                {calibrationBoard.squaresX}×{calibrationBoard.squaresY} · square {calibrationBoard.squareMm}mm · marker {calibrationBoard.markerMm}mm · margin {calibrationBoard.marginMm}mm · {calibrationBoard.dpi}dpi · {(calibrationBoard.dictionary || '4x4_1000').toUpperCase()}
              </p>
            {:else}
              <p class="mt-1 text-xs text-surface-500">
                6×4 · patch {colorChartPatchMm}mm · margin {colorChartMarginMm}mm · {colorChartDpi}dpi
              </p>
              <p class="mt-1 text-2xs text-surface-500">Print at 100% scale. Use a real ColorChecker for accurate color.</p>
            {/if}

            <div class="mt-3 flex flex-wrap items-start gap-3">
              {#if calibrationTool === 'lens'}
                <div class="rounded border border-surface-800/60 bg-surface-950/30 px-2 py-1 text-xs text-surface-500">
                  <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Lens model</p>
                  <div class="mt-1 inline-flex items-center rounded border border-surface-800/60 bg-surface-950/40 p-1">
                    <button
                      type="button"
                      class={`rounded px-3 py-1 text-2xs uppercase tracking-[0.3em] transition ${
                        calibrationLensModel === 'pinhole'
                          ? 'bg-primary-500/20 text-primary-100 border border-primary-500/60'
                          : 'text-surface-300 border border-transparent hover:text-primary-200'
                      }`}
                      onclick={() => setCalibrationLensModel('pinhole')}
                      aria-pressed={calibrationLensModel === 'pinhole'}
                    >
                      Pinhole
                    </button>
                    <button
                      type="button"
                      class={`rounded px-3 py-1 text-2xs uppercase tracking-[0.3em] transition ${
                        calibrationLensModel === 'fisheye'
                          ? 'bg-primary-500/20 text-primary-100 border border-primary-500/60'
                          : 'text-surface-300 border border-transparent hover:text-primary-200'
                      }`}
                      onclick={() => setCalibrationLensModel('fisheye')}
                      aria-pressed={calibrationLensModel === 'fisheye'}
                    >
                      Fisheye
                    </button>
                  </div>
                </div>
              {/if}
            </div>
          </div>

          <div class="flex flex-col items-center gap-2 md:items-end">
            <img
              class="h-28 w-36 rounded border border-surface-800/60 bg-surface-950/30 object-contain"
              src={calibrationTool === 'lens' ? boardPreviewUrl : colorChartPngUrl}
              alt={calibrationTool === 'lens' ? 'Calibration board preview' : 'Color chart preview'}
              loading="lazy"
            />
            {#if calibrationTool === 'lens'}
              <button class="btn btn-xs preset-tonal w-36" type="button" onclick={onOpenBoardCreator}>
                Edit board
              </button>
            {:else}
              <button class="btn btn-xs preset-tonal w-36" type="button" onclick={onOpenColorChart}>
                Edit chart
              </button>
            {/if}
          </div>
        </div>
      </div>
      {#if calibrationTool === 'lens'}
        <div class="rounded border border-surface-800/60 bg-surface-950/10 p-3">
          <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Live helpers</p>
          <div class="mt-2 flex flex-wrap items-center gap-2">
            <button
              class={`btn btn-xs ${guidedModeEnabled ? 'preset-filled' : 'preset-tonal'}`}
              type="button"
              onclick={() => setGuidedModeEnabled(!guidedModeEnabled)}
              disabled={guidedModeBusy || !streamUuid}
            >
              {guidedModeBusy ? 'Working…' : guidedModeEnabled ? 'Guided on' : 'Guided off'}
            </button>
            <button
              class="btn btn-xs preset-tonal"
              type="button"
              onclick={() => resetGuidedCoverage()}
              disabled={!guidedModeEnabled}
            >
              Reset coverage
            </button>
          </div>
          <p class="mt-2 text-2xs text-surface-500">Overlays are never saved.</p>
          {#if !streamUuid}
            <p class="mt-2 text-2xs text-surface-500">Waiting for stream UUID…</p>
          {:else if guidedModeEnabled}
            <p class="mt-2 text-2xs text-surface-500">Use snapshots to build guidance coverage. Missing targets are recommendations, not hard blockers.</p>
          {/if}
        </div>
      {/if}
    </div>
  </section>

  <section class="rounded border border-surface-800/60 bg-surface-900/40 p-4">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div class="min-w-0">
        <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Snapshots</p>
        <p class="mt-1 text-sm font-semibold text-surface-100">Capture & select images</p>
        <p class="mt-1 text-xs text-surface-500">3+ snapshots can solve; 8–15 sharp shots at different angles/distances are recommended.</p>
        <p class="mt-2 text-xs text-surface-500">
          <span class="text-surface-200">{visibleSnapshots.length}</span> shown ·
          {#if calibrationTool === 'lens'}
            <span class="text-surface-200">{selectedCount}</span> selected
          {:else}
            <span class="text-surface-200">{chartSelectedName ?? 'no chart'}</span> chart
          {/if}
        </p>
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <label class="mr-1 flex items-center gap-2 text-2xs uppercase tracking-[0.3em] text-surface-400">
          <input
            type="checkbox"
            checked={calibrationOwnPhotosOnly}
            onchange={(event) => onSetCalibrationOwnPhotosOnly((event.currentTarget as HTMLInputElement).checked)}
          />
          Own photos only
        </label>
        <button
          class="btn btn-xs preset-filled"
          type="button"
          onclick={() => onTakeSnapshot()}
          disabled={calibrationLoading || calibrationSolving || calibrationApplying}
        >
          {calibrationLoading ? 'Capturing…' : 'Capture snapshot'}
        </button>
        {#if calibrationTool === 'lens'}
          <button class="btn btn-xs preset-tonal" type="button" onclick={onSelectAllSnapshots} disabled={visibleSnapshots.length === 0}>
            Select all
          </button>
          <button class="btn btn-xs preset-tonal" type="button" onclick={onClearSelected} disabled={selectedCount === 0}>
            Clear
          </button>
          <button class="btn btn-xs preset-tonal" type="button" onclick={onDeleteSelectedSnapshots} disabled={selectedCount === 0 || calibrationDeleting}>
            Delete selected
          </button>
        {/if}
      </div>
    </div>

    {#if calibrationLoading && visibleSnapshots.length === 0}
      <p class="mt-4 text-sm text-surface-400">Loading…</p>
    {:else if visibleSnapshots.length === 0}
      <div class="mt-4 rounded border border-dashed border-surface-700/60 bg-surface-950/20 px-4 py-6">
        <p class="text-sm text-surface-300">No calibration snapshots for this filter.</p>
        <p class="mt-1 text-xs text-surface-500">Turn off “Own photos only” to browse all photo media.</p>
      </div>
    {:else}
      <div class="mt-4 grid gap-3 [grid-template-columns:repeat(auto-fit,minmax(12rem,1fr))]">
        {#each visibleSnapshots as item (item.name)}
          {@const selected = Boolean(calibrationSelected[item.name])}
          {@const isChart = ipaChartImage === item.name}
          <div
            class={`group relative overflow-hidden rounded border ${
              selected ? 'border-primary-500/70 bg-primary-950/15' : 'border-surface-800/60 bg-surface-950/10 hover:border-primary-400/40'
            } transition focus-within:ring-2 focus-within:ring-primary-500/20`}
          >
            <img class="block aspect-video w-full bg-surface-950 object-cover" src={apiPath(`/media/${encodeURIComponent(item.name)}`)} alt={item.name} loading="lazy" />

            <button class="absolute inset-0 z-0" type="button" onclick={() => onToggleSnapshot(item.name)} aria-label={selected ? 'Unselect snapshot' : 'Select snapshot'}></button>

            <div class="absolute left-2 top-2 z-10 flex items-center gap-2 rounded bg-surface-950/80 px-2 py-1">
              <input type="checkbox" checked={selected} onchange={() => onToggleSnapshot(item.name)} />
              {#if selected}
                <span class="text-micro-tight uppercase tracking-[0.3em] text-primary-200">Lens</span>
              {/if}
              {#if isChart}
                <span class="text-micro-tight uppercase tracking-[0.3em] text-secondary-200">Chart</span>
              {/if}
            </div>

            <div class="absolute right-2 top-2 z-10 flex items-center gap-2">
              <button
                class="inline-flex h-8 w-8 items-center justify-center rounded border border-surface-700/70 bg-surface-950/70 text-surface-200 transition opacity-0 group-hover:opacity-100 focus:opacity-100 focus-visible:opacity-100 group-focus-within:opacity-100 hover:border-error-400/50 hover:text-error-100 disabled:opacity-40"
                type="button"
                onclick={() => onDeleteSnapshot(item.name)}
                aria-label="Delete snapshot"
                disabled={calibrationDeleting}
              >
                <svg viewBox="0 0 24 24" class="h-4 w-4" fill="currentColor" aria-hidden="true">
                  <path
                    d="M9 3h6l1 2h4v2H4V5h4l1-2Zm1 6h2v8h-2V9Zm4 0h2v8h-2V9ZM7 9h2v8H7V9Z"
                  />
                </svg>
              </button>
              <button
                class="inline-flex h-8 w-8 items-center justify-center rounded border border-surface-700/70 bg-surface-950/70 text-surface-200 transition opacity-0 group-hover:opacity-100 focus:opacity-100 focus-visible:opacity-100 group-focus-within:opacity-100 hover:border-primary-400/50 hover:text-primary-100"
                type="button"
                onclick={() => onOpenPreview(item)}
                aria-label="Enlarge snapshot"
              >
                <svg viewBox="0 0 24 24" class="h-4 w-4" fill="currentColor" aria-hidden="true">
                  <path
                    d="M10 18a8 8 0 1 1 5.293-14.01A8 8 0 0 1 10 18Zm0-14a6 6 0 1 0 3.99 10.49A6 6 0 0 0 10 4Zm9.707 16.293-3.387-3.387a1 1 0 1 0-1.414 1.414l3.387 3.387a1 1 0 0 0 1.414-1.414Z"
                  />
                </svg>
              </button>
            </div>

            <div class="absolute inset-x-0 bottom-0 z-10 bg-gradient-to-t from-surface-950/95 to-surface-950/10 px-3 py-2">
              <p class="truncate font-mono text-xs text-surface-100">{item.name}</p>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </section>
</div>
