<script lang="ts">
  import type { CalibrationImage } from '../cameraCalibrationTypes';

  type CalibrationPreviewModalProps = {
    open: boolean;
    item: CalibrationImage | null;
    calibrationSelected: Record<string, boolean>;
    ipaChartImage: string;
    calibrationDeleting: boolean;
    apiPath: (path: string) => string;
    overlayUrlForImage: (name: string) => string | null;
    onClose: () => void;
    onDelete: (name: string) => void;
    onToggleSnapshot: (name: string) => void;
    onToggleChartImage: (name: string) => void;
    onOpenIpaChartSolverForImage: (name: string) => void;
    onSetCalibrationTool: (tool: 'lens' | 'color') => void;
  };

  const {
    open,
    item,
    calibrationSelected,
    ipaChartImage,
    calibrationDeleting,
    apiPath,
    overlayUrlForImage,
    onClose,
    onDelete,
    onToggleSnapshot,
    onToggleChartImage,
    onOpenIpaChartSolverForImage,
    onSetCalibrationTool
  }: CalibrationPreviewModalProps = $props();

  let previewShowOverlay = $state(false);

  $effect(() => {
    void open;
    void item;
    if (open && item) {
      previewShowOverlay = false;
    }
  });

  function openAsset(url: string): void {
    window.open(url, '_blank', 'noopener,noreferrer');
  }

  export type $$Props = CalibrationPreviewModalProps;
</script>

{#if open && item}
  {@const lensSelected = Boolean(calibrationSelected[item.name])}
  {@const isChart = ipaChartImage === item.name}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-surface-950/70 px-4"
    role="dialog"
    aria-modal="true"
    onclick={(event) => {
      if (event.target === event.currentTarget) onClose();
    }}
    onkeydown={(event) => {
      if (event.key === 'Escape') onClose();
    }}
    tabindex="-1"
  >
    <div class="relative w-full max-w-5xl rounded border border-surface-800/70 bg-surface-950/95 p-6 text-sm text-surface-400 shadow-2xl max-h-[90vh] max-h-[90svh] max-h-[90dvh] overflow-y-auto">
      <div class="flex items-start justify-between gap-4">
        <div class="flex-1 min-w-0">
          <p class="text-micro uppercase tracking-[0.3em] text-surface-500">Calibration image</p>
          <div class="mt-2 flex items-center gap-2">
            <p class="min-w-0 truncate font-mono text-xs text-surface-200">{item.name}</p>
          </div>
        </div>
        <div class="flex flex-wrap gap-2">
          <button class="btn btn-xs preset-outline" type="button" onclick={onClose}>
            Close
          </button>
          <button
            class="btn btn-xs preset-outline text-error-200 hover:text-error-100"
            type="button"
            onclick={() => onDelete(item.name)}
            disabled={calibrationDeleting}
          >
            {calibrationDeleting ? 'Deleting…' : 'Delete'}
          </button>
          <button
            class="btn btn-xs preset-outline"
            type="button"
            onclick={() => openAsset(apiPath(`/media/${encodeURIComponent(item.name)}`))}
          >
            Download
          </button>
        </div>
      </div>

      <div class="mt-4 grid gap-6 xl:grid-cols-[1.2fr_1fr]">
        <div class="min-w-0 rounded border border-surface-800/60 bg-surface-900/40 p-3">
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Preview</p>
          <div class="mt-2 flex flex-wrap items-center justify-between gap-2 text-xs text-surface-400">
            <label class="flex items-center gap-2">
              <input
                type="checkbox"
                checked={previewShowOverlay}
                disabled={!overlayUrlForImage(item.name)}
                onchange={(e) => (previewShowOverlay = e.currentTarget.checked)}
              />
              Show overlay
            </label>
            {#if previewShowOverlay}
              <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">generated (not saved)</span>
            {/if}
          </div>
          <div class="mt-2 w-full max-h-[min(60vh,42rem)] max-h-[min(60svh,42rem)] max-h-[min(60dvh,42rem)] min-w-0 overflow-auto rounded border border-surface-800/60 bg-surface-950/40">
            <img
              class="block h-auto w-full"
              src={(previewShowOverlay ? overlayUrlForImage(item.name) : null) ?? apiPath(`/media/${encodeURIComponent(item.name)}`)}
              alt={item.name}
            />
          </div>
        </div>

        <div class="space-y-4">
          <div class="rounded border border-surface-800/60 bg-surface-900/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Use</p>
            <div class="mt-3 space-y-3">
              <label class="flex items-center justify-between gap-3 text-sm text-surface-200">
                <span class="flex items-center gap-2">
                  <input type="checkbox" checked={lensSelected} onchange={() => onToggleSnapshot(item.name)} />
                  Lens calibration
                </span>
                {#if lensSelected}
                  <span class="rounded border border-primary-500/40 bg-primary-500/10 px-2 py-1 text-micro-tight uppercase tracking-[0.3em] text-primary-200">Selected</span>
                {/if}
              </label>

              <label class="flex items-center justify-between gap-3 text-sm text-surface-200">
                <span class="flex items-center gap-2">
                  <input type="radio" name="modal-chart-image" checked={isChart} onchange={() => onToggleChartImage(item.name)} />
                  Chart image
                </span>
                {#if isChart}
                  <span class="rounded border border-secondary-500/40 bg-secondary-500/10 px-2 py-1 text-micro-tight uppercase tracking-[0.3em] text-secondary-200">Chart</span>
                {/if}
              </label>
            </div>
          </div>

          <div class="flex flex-wrap gap-2">
            <button class="btn btn-xs preset-tonal" type="button" onclick={() => onOpenIpaChartSolverForImage(item.name)}>
              Pick corners
            </button>
            <button class="btn btn-xs preset-tonal" type="button" onclick={() => onSetCalibrationTool('lens')}>
              Go to lens
            </button>
            <button class="btn btn-xs preset-tonal" type="button" onclick={() => onSetCalibrationTool('color')}>
              Go to color
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
{/if}
