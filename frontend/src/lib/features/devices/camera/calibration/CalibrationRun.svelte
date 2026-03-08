<script lang="ts">
  import { untrack } from 'svelte';
  import CalibrationResults from './CalibrationResults.svelte';
  import { estimateCalibrationFovDegs } from '../cameraCalibrationUtils';

  type CalibrationImage = {
    name: string;
    size_bytes: number;
    content_type: string;
    stream_id?: string;
    kind?: string;
    captured_at_ms?: number;
  };

  type CalibrationParams = {
    fx: number;
    fy: number;
    cx: number;
    cy: number;
    k1: number;
    k2: number;
    p1: number;
    p2: number;
    k3: number;
    undistortIters: number;
    lensModel?: 'pinhole' | 'fisheye';
  };

  type CalibrationResult = {
    calibration: {
      fx: number;
      fy: number;
      cx: number;
      cy: number;
      k1: number;
      k2: number;
      p1: number;
      p2: number;
      k3: number;
      undistortIters: number;
      lensModel?: 'pinhole' | 'fisheye';
    };
    reprojectionErrorPx: number;
    viewsUsed: number;
    pointsUsed: number;
    warnings?: string[];
    debugViews?: Array<{
      image: string;
      overlay?: string | null;
      tagsDetected: number;
      pointsDetected: number;
      used: boolean;
      coverageRatio: number;
    }>;
  };

  type IpaStatus = {
    files: Array<{ target: string; path: string; exists: boolean; ccmCt?: number | null; ccm?: number[] | null }>;
  };

  type CalibrationImportSource = {
    id: string;
    label: string;
  };

  type CalibrationRunProps = {
    calibrationTool: 'lens' | 'color';
    calibrationBoard: unknown;
    streamUuid: string | null;
    currentCalibrationParams: CalibrationParams | null;
    sourceResolution: { width: number; height: number } | null;
    selectedCount: number;
    calibrationResult: CalibrationResult | null;
    calibrationSolveError: string | null;
    calibrationSolving: boolean;
    calibrationApplying: boolean;
    calibrationLoading: boolean;
    calibrationImages: CalibrationImage[];
    calibrationDeleting: boolean;
    calibrationIncludeOverlays: boolean;
    calibrationImportSourcesLoading: boolean;
    calibrationImporting: boolean;
    calibrationImportError: string | null;
    calibrationImportSourceId: string;
    calibrationImportSources: CalibrationImportSource[];
    apiPath: (path: string) => string;
    formatMaybeNumber: (value: number | null, digits?: number) => string;
    overlayUrlForImage: (name: string) => string | null;
    onToggleIncludeOverlays: (value: boolean) => void;
    onSetCalibrationImportSourceId: (value: string) => void;
    onCopyCalibrationFromStream: () => void;
    onImportCalibrationJsonFile: (file: File) => void | Promise<void>;
    onDeleteOverlays: () => void;
    onSolveCalibration: () => void;
    onSaveCalibration: () => void;
    ipaStatus: IpaStatus | null;
    ipaLoading: boolean;
    ipaCt: number;
    setIpaCt: (value: number) => void;
    ipaAdvanced: boolean;
    setIpaAdvanced: (value: boolean) => void;
    ipaTarget: 'both' | 'pisp' | 'vc4';
    setIpaTarget: (value: 'both' | 'pisp' | 'vc4') => void;
    ipaCcm: number[][];
    setIpaCcm: (value: number[][]) => void;
    ipaChartImage: string;
    hasChartImage: boolean;
    onToggleChartImage: (name: string) => void;
    onOpenPreview: (item: CalibrationImage) => void;
    onDeleteSnapshot: (name: string) => void;
    onOpenIpaChartSolverForImage: (name: string) => void;
    onApplyIpaCcm: () => void;
  };

  const {
    calibrationTool,
    calibrationBoard,
    streamUuid,
    currentCalibrationParams,
    sourceResolution,
    selectedCount,
    calibrationResult,
    calibrationSolveError,
    calibrationSolving,
    calibrationApplying,
    calibrationLoading,
    calibrationImages,
    calibrationDeleting,
    calibrationIncludeOverlays,
    calibrationImportSourcesLoading,
    calibrationImporting,
    calibrationImportError,
    calibrationImportSourceId,
    calibrationImportSources,
    apiPath,
    formatMaybeNumber,
    overlayUrlForImage,
    onToggleIncludeOverlays,
    onSetCalibrationImportSourceId,
    onCopyCalibrationFromStream,
    onImportCalibrationJsonFile,
    onDeleteOverlays,
    onSolveCalibration,
    onSaveCalibration,
    ipaStatus,
    ipaLoading,
    ipaCt,
    setIpaCt,
    ipaAdvanced,
    setIpaAdvanced,
    ipaTarget,
    setIpaTarget,
    ipaCcm,
    setIpaCcm,
    ipaChartImage,
    hasChartImage,
    onToggleChartImage,
    onOpenPreview,
    onDeleteSnapshot,
    onOpenIpaChartSolverForImage,
    onApplyIpaCcm
  }: CalibrationRunProps = $props();
  untrack(() => calibrationBoard);

  export type $$Props = CalibrationRunProps;

  const hasSavedCalibration = $derived(
    Boolean(currentCalibrationParams) && Number(currentCalibrationParams?.fx ?? 0) > 0 && Number(currentCalibrationParams?.fy ?? 0) > 0
  );
  const hasDownloadPayload = $derived(Boolean(calibrationResult?.calibration) || hasSavedCalibration);
  const downloadPayload = $derived(
    hasSavedCalibration
      ? {
          fx: currentCalibrationParams?.fx,
          fy: currentCalibrationParams?.fy,
          cx: currentCalibrationParams?.cx,
          cy: currentCalibrationParams?.cy,
          k1: currentCalibrationParams?.k1,
          k2: currentCalibrationParams?.k2,
          k3: currentCalibrationParams?.k3,
          p1: currentCalibrationParams?.p1,
          p2: currentCalibrationParams?.p2,
          undistortIters: currentCalibrationParams?.undistortIters,
          lensModel: currentCalibrationParams?.lensModel
        }
      : calibrationResult?.calibration
        ? {
            fx: calibrationResult.calibration.fx,
            fy: calibrationResult.calibration.fy,
            cx: calibrationResult.calibration.cx,
            cy: calibrationResult.calibration.cy,
            k1: calibrationResult.calibration.k1,
            k2: calibrationResult.calibration.k2,
            k3: calibrationResult.calibration.k3,
            p1: calibrationResult.calibration.p1,
            p2: calibrationResult.calibration.p2,
            undistortIters: calibrationResult.calibration.undistortIters,
            lensModel: calibrationResult.calibration.lensModel
          }
        : null
  );

  // Svelte 5 `{@const}` tags have strict placement rules; compute this in script instead.
  const currentFovs = $derived(estimateCalibrationFovDegs(sourceResolution, currentCalibrationParams));
  const calibrationQualityTier = $derived.by(() => {
    if (!calibrationResult) return null;
    const views = Math.max(0, Number(calibrationResult.viewsUsed ?? 0));
    const points = Math.max(0, Number(calibrationResult.pointsUsed ?? 0));
    const reproj = Number(calibrationResult.reprojectionErrorPx ?? Number.POSITIVE_INFINITY);
    if (views >= 8 && points >= 200 && Number.isFinite(reproj) && reproj <= 1.2) {
      return {
        label: 'High quality',
        detail: 'Strong view diversity and low reprojection error.',
        className: 'border-success-700/50 bg-success-950/20 text-success-100'
      };
    }
    if (views >= 4 && points >= 80 && Number.isFinite(reproj) && reproj <= 2.0) {
      return {
        label: 'Medium quality',
        detail: 'Usable solve. More diverse snapshots can improve stability.',
        className: 'border-warning-700/50 bg-warning-950/20 text-warning-100'
      };
    }
    return {
      label: 'Low quality',
      detail: 'Solve succeeded, but additional angles/distances are recommended before saving.',
      className: 'border-warning-700/50 bg-warning-950/20 text-warning-100'
    };
  });

  let calibrationImportInput = $state<HTMLInputElement | null>(null);

  function triggerCalibrationJsonImport(): void {
    calibrationImportInput?.click();
  }

  function handleCalibrationJsonImport(event: Event): void {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0] ?? null;
    if (!file) return;
    void onImportCalibrationJsonFile(file);
    input.value = '';
  }

  function downloadCalibrationJson(): void {
    if (!downloadPayload) return;
    const streamLabel = (streamUuid ?? 'stream').trim() || 'stream';
    const timestamp = new Date().toISOString().replaceAll(':', '-').replaceAll('.', '-');
    const payload = JSON.stringify(downloadPayload, null, 2);
    const blob = new Blob([payload], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = `calibration-${streamLabel}-${timestamp}.json`;
    document.body.appendChild(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
  }

  function openIpaDownload(url: string): void {
    if (typeof window === 'undefined') return;
    window.open(url, '_blank', 'noopener,noreferrer');
  }
</script>

<section class="rounded border border-surface-800/60 bg-surface-900/40 p-4">
  <div class="flex flex-wrap items-start justify-between gap-3">
    <div class="min-w-0">
      <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Solver</p>
      <p class="mt-1 text-sm font-semibold text-surface-100">{calibrationTool === 'lens' ? 'Lens calibration' : 'Color correction'}</p>
    </div>
    {#if calibrationTool === 'lens'}
      <div class="flex flex-wrap items-center justify-end gap-1">
        <select
          class="h-8 w-full rounded border border-surface-700/70 bg-surface-950/70 px-2 text-xs text-surface-200 focus-visible:outline-none sm:min-w-[14rem]"
          value={calibrationImportSourceId}
          onchange={(event) => onSetCalibrationImportSourceId((event.currentTarget as HTMLSelectElement).value)}
          disabled={calibrationImporting || calibrationImportSourcesLoading || calibrationImportSources.length === 0}
          aria-label="Select source stream"
        >
          <option value="" selected={calibrationImportSourceId === ''}>Select source stream</option>
          {#each calibrationImportSources as source (source.id)}
            <option value={source.id}>{source.label}</option>
          {/each}
        </select>
        <button
          class="inline-flex h-8 w-8 items-center justify-center rounded border border-surface-700/70 bg-surface-950/70 text-surface-200 transition hover:border-amber-400/50 hover:text-amber-100"
          type="button"
          onclick={onCopyCalibrationFromStream}
          disabled={!calibrationImportSourceId || calibrationImporting || calibrationImportSourcesLoading}
          aria-label="Copy calibration from source stream"
          title="Copy calibration from source stream"
        >
          <svg viewBox="0 0 24 24" class="h-4 w-4" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
            <rect x="9" y="9" width="10" height="11" rx="2" />
            <path d="M15 9V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3" />
          </svg>
        </button>
        <button
          class="inline-flex h-8 w-8 items-center justify-center rounded border border-surface-700/70 bg-surface-950/70 text-surface-200 transition hover:border-primary-400/50 hover:text-primary-100"
          type="button"
          onclick={downloadCalibrationJson}
          aria-label="Download calibration JSON"
          title="Download calibration JSON"
          disabled={!hasDownloadPayload}
        >
          <svg viewBox="0 0 24 24" class="h-4 w-4" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
            <path d="M12 3v11" stroke-linecap="round" />
            <path d="m7 10 5 5 5-5" stroke-linecap="round" stroke-linejoin="round" />
            <path d="M4 18h16v3H4z" />
          </svg>
        </button>
        <button
          class="inline-flex h-8 w-8 items-center justify-center rounded border border-surface-700/70 bg-surface-950/70 text-surface-200 transition hover:border-secondary-400/50 hover:text-secondary-100 disabled:opacity-40"
          type="button"
          onclick={triggerCalibrationJsonImport}
          disabled={calibrationImporting}
          aria-label="Import calibration JSON"
          title="Import calibration JSON"
        >
          <svg viewBox="0 0 24 24" class="h-4 w-4" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
            <path d="M12 16V5" stroke-linecap="round" />
            <path d="m7 10 5-5 5 5" stroke-linecap="round" stroke-linejoin="round" />
            <path d="M4 18h16v3H4z" />
          </svg>
        </button>
      </div>
      <input
        class="hidden"
        type="file"
        accept=".json,application/json"
        bind:this={calibrationImportInput}
        onchange={handleCalibrationJsonImport}
      />
    {/if}
  </div>
  {#if calibrationTool === 'lens' && calibrationImportError}
    <p class="mt-2 text-xs text-error-300">{calibrationImportError}</p>
  {/if}

  {#if calibrationTool === 'lens'}
    <div class="mt-4 space-y-4">
      <div class="rounded border border-surface-800/60 bg-surface-950/10 p-3">
        <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Current Calibration</p>
        {#if currentCalibrationParams}
          {#if !(Number(currentCalibrationParams.fx ?? 0) > 0 && Number(currentCalibrationParams.fy ?? 0) > 0)}
            <p class="mt-2 text-xs text-surface-400">
              Intrinsics look unset (fx/fy ≤ 0). Solve + Save to store calibration on the stream.
            </p>
          {/if}
          <div class="mt-2 grid gap-2 [grid-template-columns:repeat(auto-fit,minmax(16rem,1fr))]">
            <div class="grid gap-1 text-xs text-surface-200">
              <div class="flex justify-between gap-3">
                <span class="text-surface-500">fx</span>
                <span class="font-mono">{Number(currentCalibrationParams.fx ?? 0).toFixed(2)}</span>
              </div>
              <div class="flex justify-between gap-3">
                <span class="text-surface-500">fy</span>
                <span class="font-mono">{Number(currentCalibrationParams.fy ?? 0).toFixed(2)}</span>
              </div>
              <div class="flex justify-between gap-3">
                <span class="text-surface-500">cx</span>
                <span class="font-mono">{Number(currentCalibrationParams.cx ?? 0).toFixed(2)}</span>
              </div>
              <div class="flex justify-between gap-3">
                <span class="text-surface-500">cy</span>
                <span class="font-mono">{Number(currentCalibrationParams.cy ?? 0).toFixed(2)}</span>
              </div>
            </div>
            <div class="grid gap-1 text-xs text-surface-200">
              <div class="flex justify-between gap-3">
                <span class="text-surface-500">k1</span>
                <span class="font-mono">{Number(currentCalibrationParams.k1 ?? 0).toExponential(3)}</span>
              </div>
              <div class="flex justify-between gap-3">
                <span class="text-surface-500">k2</span>
                <span class="font-mono">{Number(currentCalibrationParams.k2 ?? 0).toExponential(3)}</span>
              </div>
              <div class="flex justify-between gap-3">
                <span class="text-surface-500">k3</span>
                <span class="font-mono">{Number(currentCalibrationParams.k3 ?? 0).toExponential(3)}</span>
              </div>
              <div class="flex justify-between gap-3">
                <span class="text-surface-500">p1 / p2</span>
                <span class="font-mono">{Number(currentCalibrationParams.p1 ?? 0).toExponential(3)} / {Number(currentCalibrationParams.p2 ?? 0).toExponential(3)}</span>
              </div>
              <div class="flex justify-between gap-3">
                <span class="text-surface-500">model</span>
                <span class="font-mono">{currentCalibrationParams.lensModel ?? 'pinhole'}</span>
              </div>
            </div>
            <div class="grid gap-1 text-xs text-surface-200">
              <div class="flex justify-between gap-3">
                <span class="text-surface-500">Resolution</span>
                <span class="font-mono">{sourceResolution ? `${sourceResolution.width}×${sourceResolution.height}` : '—'}</span>
              </div>
              <div class="flex justify-between gap-3">
                <span class="text-surface-500">HFOV</span>
                <span class="font-mono">{formatMaybeNumber(currentFovs.hfov, 2)}°</span>
              </div>
              <div class="flex justify-between gap-3">
                <span class="text-surface-500">VFOV</span>
                <span class="font-mono">{formatMaybeNumber(currentFovs.vfov, 2)}°</span>
              </div>
              <div class="flex justify-between gap-3">
                <span class="text-surface-500">DFOV</span>
                <span class="font-mono">
                  {formatMaybeNumber(currentFovs.dfov, 2)}°
                </span>
              </div>
            </div>
          </div>
        {:else}
          <p class="mt-2 text-xs text-surface-400">No `cv:aruco:decode_quads_calibrated` intrinsics found in the current stream graph.</p>
        {/if}
      </div>

      {#if calibrationSolveError}
        <div class="rounded border border-error-800/60 bg-error-950/20 px-4 py-3 text-xs text-error-200">
          {calibrationSolveError}
        </div>
      {/if}

      {#if !calibrationResult}
        <div class="rounded border border-surface-800/60 bg-surface-950/10 px-4 py-3 text-sm text-surface-300">
          {#if selectedCount === 0}
            Select snapshots first.
          {:else}
            Ready to solve with <span class="text-surface-100">{selectedCount}</span> selected.
          {/if}
        </div>
      {:else}
        {#if calibrationQualityTier}
          <div class={`rounded border px-4 py-3 text-xs ${calibrationQualityTier.className}`}>
            <p class="font-semibold uppercase tracking-[0.2em]">{calibrationQualityTier.label}</p>
            <p class="mt-1">{calibrationQualityTier.detail}</p>
          </div>
        {/if}
        <CalibrationResults
          calibrationResult={calibrationResult}
          sourceResolution={sourceResolution}
          formatMaybeNumber={formatMaybeNumber}
          overlayUrlForImage={overlayUrlForImage}
        />
      {/if}

      <div class="rounded border border-surface-800/60 bg-surface-950/10 px-3 py-2">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <label class="flex items-center gap-2 text-xs text-surface-400">
            <input
              type="checkbox"
              class="checkbox checkbox-xs"
              checked={calibrationIncludeOverlays}
              onchange={(e) => onToggleIncludeOverlays((e.currentTarget as HTMLInputElement).checked)}
            />
            Save overlays to media (JPEG)
          </label>
          <button
            class="btn btn-xs preset-tonal"
            type="button"
            onclick={onDeleteOverlays}
            disabled={calibrationDeleting}
          >
            Delete overlays
          </button>
        </div>
        <p class="mt-2 text-2xs text-surface-500">Overlays are optional and can be large; disable to keep solves ephemeral.</p>
      </div>

      <div class="mt-4 flex flex-wrap justify-end gap-2">
        <button
          class="btn btn-xs preset-filled"
          type="button"
          onclick={onSolveCalibration}
          disabled={selectedCount === 0 || calibrationSolving || calibrationApplying || calibrationLoading}
        >
          {calibrationSolving ? 'Solving…' : 'Solve'}
        </button>
        <button
          class="btn btn-xs preset-tonal"
          type="button"
          onclick={onSaveCalibration}
          disabled={!calibrationResult || calibrationApplying || calibrationSolving}
        >
          {calibrationApplying ? 'Saving…' : 'Save'}
        </button>
      </div>
    </div>
  {:else}
    <div class="mt-4 space-y-4">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div class="min-w-0">
          <p class="text-xs text-surface-500">Solve a 3×3 CCM from a ColorChecker Classic 24 photo and write it into the IPA JSON.</p>
        </div>
        <div class="flex flex-wrap gap-2">
          <button
            class="btn btn-xs preset-tonal"
            type="button"
            onclick={() => openIpaDownload(apiPath('/device/ipa/download?target=pisp'))}
          >
            pisp JSON
          </button>
          <button
            class="btn btn-xs preset-tonal"
            type="button"
            onclick={() => openIpaDownload(apiPath('/device/ipa/download?target=vc4'))}
          >
            vc4 JSON
          </button>
        </div>
      </div>

      {#if ipaStatus?.files?.length}
        <div class="grid gap-2 [grid-template-columns:repeat(auto-fit,minmax(16rem,1fr))]">
          {#each ipaStatus.files as file (file.path)}
            <div class="rounded border border-surface-800/60 bg-surface-950/10 px-3 py-2">
              <p class="text-xs font-semibold text-surface-200">{file.target}</p>
              <p class="mt-0.5 truncate font-mono text-micro text-surface-500">{file.path}</p>
              <p class="mt-1 text-xs text-surface-400">
                {file.exists ? 'present' : 'missing'}
                {#if file.ccmCt}
                  · ct {file.ccmCt}
                {/if}
              </p>
            </div>
          {/each}
        </div>
      {/if}

      <div class="grid gap-3">
        <div class="flex flex-wrap items-end justify-between gap-2">
          <div class="min-w-0">
            <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Chart image</p>
            <p class="mt-1 text-xs text-surface-500">Pick one snapshot with the ColorChecker visible and evenly lit.</p>
          </div>
          <div class="flex items-center gap-3">
            <label class="text-sm">
              <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">CT</span>
              <input class="input input-sm mt-1 w-36" type="number" min="0" step="1" value={ipaCt} oninput={(e) => setIpaCt(Number((e.currentTarget as HTMLInputElement).value) || 4000)} />
            </label>
            <label class="mt-5 flex items-center gap-2 text-xs text-surface-400">
              <input type="checkbox" checked={ipaAdvanced} onchange={(e) => setIpaAdvanced((e.currentTarget as HTMLInputElement).checked)} />
              Advanced
            </label>
          </div>
        </div>

        {#if calibrationImages.length === 0}
          <p class="text-sm text-surface-400">Capture a snapshot first.</p>
        {:else}
          <div class="grid gap-3 [grid-template-columns:repeat(auto-fit,minmax(12rem,1fr))]">
            {#each calibrationImages as item (item.name)}
              {@const selected = ipaChartImage === item.name}
              <div
                class={`group relative overflow-hidden rounded border ${
                  selected ? 'border-secondary-500/60 bg-secondary-950/15' : 'border-surface-800/60 bg-surface-950/10 hover:border-secondary-400/40'
                } transition focus-within:ring-2 focus-within:ring-secondary-500/20`}
              >
                <img class="block aspect-video w-full bg-surface-950 object-cover" src={apiPath(`/media/${encodeURIComponent(item.name)}`)} alt={item.name} loading="lazy" />
                <button class="absolute inset-0 z-0" type="button" onclick={() => onToggleChartImage(item.name)} aria-label={selected ? 'Unselect chart image' : 'Select chart image'}></button>

                <div class="absolute left-2 top-2 z-10 flex items-center gap-2 rounded bg-surface-950/80 px-2 py-1">
                  <input type="radio" name="ccm-chart-image" checked={selected} onchange={() => onToggleChartImage(item.name)} />
                  <span class="text-micro-tight uppercase tracking-[0.3em] text-secondary-200">{selected ? 'Chart' : 'Set chart'}</span>
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
                    class="inline-flex h-8 w-8 items-center justify-center rounded border border-surface-700/70 bg-surface-950/70 text-surface-200 transition opacity-0 group-hover:opacity-100 focus:opacity-100 focus-visible:opacity-100 group-focus-within:opacity-100 hover:border-secondary-400/50 hover:text-secondary-100"
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
      </div>

      {#if ipaAdvanced}
        <div class="grid gap-3 md:grid-cols-3">
          <label class="text-sm">
            <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Target</span>
            <select
              class="select select-sm mt-1 w-full"
              value={ipaTarget}
              onchange={(e) => {
                const value = (e.currentTarget as HTMLSelectElement).value;
                if (value === 'both' || value === 'pisp' || value === 'vc4') setIpaTarget(value);
              }}
            >
              <option value="both">Both</option>
              <option value="pisp">pisp</option>
              <option value="vc4">vc4</option>
            </select>
          </label>
        </div>

        <div class="mt-3 grid gap-2 md:grid-cols-3">
          {#each [0, 1, 2] as row (row)}
            {#each [0, 1, 2] as col (col)}
              <label class="text-sm">
                <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">m{row + 1}{col + 1}</span>
                <input
                  class="input input-sm mt-1 w-full font-mono"
                  type="number"
                  step="0.0001"
                  value={ipaCcm?.[row]?.[col] ?? 0}
                  oninput={(e) => {
                    const next = Number((e.currentTarget as HTMLInputElement).value);
                    const matrix = ipaCcm.map((r) => r.slice());
                    if (!matrix[row]) matrix[row] = [0, 0, 0];
                    matrix[row][col] = Number.isFinite(next) ? next : 0;
                    setIpaCcm(matrix);
                  }}
                />
              </label>
            {/each}
          {/each}
        </div>
      {:else}
        <div class="grid gap-2 md:grid-cols-3">
          {#each [0, 1, 2] as row (row)}
            <div class="rounded border border-surface-800/60 bg-surface-950/10 px-3 py-2 font-mono text-xs text-surface-200">
              {Number(ipaCcm?.[row]?.[0] ?? 0).toFixed(4)} {Number(ipaCcm?.[row]?.[1] ?? 0).toFixed(4)} {Number(ipaCcm?.[row]?.[2] ?? 0).toFixed(4)}
            </div>
          {/each}
        </div>
      {/if}

      <div class="mt-4 flex flex-wrap justify-end gap-2">
        <button
          class="btn btn-xs preset-tonal"
          type="button"
          onclick={() => ipaChartImage && onOpenIpaChartSolverForImage(ipaChartImage)}
          disabled={!hasChartImage}
        >
          Pick corners
        </button>
        <button class="btn btn-xs preset-filled" type="button" onclick={onApplyIpaCcm} disabled={ipaLoading}>
          {ipaLoading ? 'Applying…' : 'Apply CCM'}
        </button>
      </div>
    </div>
  {/if}
</section>
