<script lang="ts">
  import type { OrientationChoice, PaperChoice } from '../cameraCalibrationUtils';

  type Props = {
    open: boolean;
    colorChartPaper: PaperChoice;
    colorChartOrientation: OrientationChoice;
    colorChartPatchMm: number;
    colorChartMarginMm: number;
    colorChartDpi: number;
    chartPageWmm: number;
    chartPageHmm: number;
    chartFitsOnPaper: boolean;
    chartPaperLabel: string;
    chartRequiredXPct: number;
    chartRequiredYPct: number;
    chartRequiredWPct: number;
    chartRequiredHPct: number;
    chartScaleXPct: number;
    chartScaleLenPct: number;
    chartScaleBottomPct: number;
    chartTickHalfPct: number;
    chartScaleMidXPct: number;
    chartScaleLabelLeftPct: number;
    chartScaleLabelBottomPct: number;
    chartScaleLenMm: number;
    colorChartPdfUrl: string;
    colorChartPngUrl: string;
  };

  let {
    open = $bindable(false),
    colorChartPaper = $bindable('auto'),
    colorChartOrientation = $bindable('auto'),
    colorChartPatchMm = $bindable(25),
    colorChartMarginMm = $bindable(10),
    colorChartDpi = $bindable(300),
    chartPageWmm,
    chartPageHmm,
    chartFitsOnPaper,
    chartPaperLabel,
    chartRequiredXPct,
    chartRequiredYPct,
    chartRequiredWPct,
    chartRequiredHPct,
    chartScaleXPct,
    chartScaleLenPct,
    chartScaleBottomPct,
    chartTickHalfPct,
    chartScaleMidXPct,
    chartScaleLabelLeftPct,
    chartScaleLabelBottomPct,
    chartScaleLenMm,
    colorChartPdfUrl,
    colorChartPngUrl
  }: Props = $props();

  const chartScaleBaseY = $derived(100 - chartScaleBottomPct);
  const chartScaleLabelY = $derived(100 - chartScaleLabelBottomPct);
  const chartScaleTickHalf = $derived(chartTickHalfPct);

  function openColorChartPdf(): void {
    if (!colorChartPdfUrl) return;
    window.open(colorChartPdfUrl, '_blank', 'noopener,noreferrer');
  }
</script>

{#if open}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-surface-950/70 px-4"
    role="dialog"
    aria-modal="true"
    aria-label="Create ColorChecker chart"
    onclick={(event) => {
      if (event.target === event.currentTarget) open = false;
    }}
    onkeydown={(event) => {
      if (event.key === 'Escape') open = false;
    }}
    tabindex="-1"
  >
    <div class="relative w-full max-w-6xl rounded border border-surface-800/70 bg-surface-950/95 p-6 text-sm text-surface-400 shadow-2xl max-h-[92vh] max-h-[92svh] max-h-[92dvh] overflow-y-auto">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div class="min-w-0">
          <p class="text-micro uppercase tracking-[0.3em] text-surface-500">ColorChecker chart</p>
          <p class="mt-2 text-xs text-surface-500">Print at 100% scale. For accurate color, use a real ColorChecker chart.</p>
        </div>
        <div class="flex flex-wrap gap-2">
          <button class="btn btn-xs preset-outline" type="button" onclick={() => (open = false)}>
            Close
          </button>
        </div>
      </div>

      <div class="mt-4 grid gap-4 xl:grid-cols-[22rem,minmax(0,1fr)]">
        <div class="space-y-4">
          <div class="rounded border border-surface-800/60 bg-surface-900/40 p-4">
            <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Paper</p>
            <div class="mt-3 grid gap-3 [grid-template-columns:repeat(auto-fit,minmax(10rem,1fr))]">
              <label class="text-sm">
                <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Size</span>
                <select
                  class="select select-sm mt-1 w-full"
                  value={colorChartPaper}
                  onchange={(e) => {
                    const value = (e.currentTarget as HTMLSelectElement).value;
                    if (value === 'auto' || value === 'letter' || value === 'a4' || value === 'custom') colorChartPaper = value;
                  }}
                >
                  <option value="auto">Auto</option>
                  <option value="letter">Letter (8.5×11")</option>
                  <option value="a4">A4 (210×297mm)</option>
                  <option value="custom">Custom (fit chart)</option>
                </select>
              </label>

              <label class="text-sm">
                <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Orientation</span>
                <select
                  class="select select-sm mt-1 w-full"
                  value={colorChartOrientation}
                  disabled={colorChartPaper === 'custom'}
                  onchange={(e) => {
                    const value = (e.currentTarget as HTMLSelectElement).value;
                    if (value === 'auto' || value === 'portrait' || value === 'landscape') colorChartOrientation = value;
                  }}
                >
                  <option value="auto">Auto</option>
                  <option value="portrait">Portrait</option>
                  <option value="landscape">Landscape</option>
                </select>
              </label>
            </div>
          </div>

          <div class="rounded border border-surface-800/60 bg-surface-900/40 p-4">
            <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Dimensions</p>
            <div class="mt-3 grid gap-3 [grid-template-columns:repeat(auto-fit,minmax(10rem,1fr))]">
              <label class="text-sm">
                <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Patch mm</span>
                <input
                  class="input input-sm mt-1 w-full"
                  type="number"
                  min="5"
                  step="0.5"
                  value={colorChartPatchMm}
                  oninput={(e) => (colorChartPatchMm = Number((e.currentTarget as HTMLInputElement).value) || 25)}
                />
              </label>
              <label class="text-sm">
                <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Margin mm</span>
                <input
                  class="input input-sm mt-1 w-full"
                  type="number"
                  min="0"
                  step="0.5"
                  value={colorChartMarginMm}
                  oninput={(e) => (colorChartMarginMm = Number((e.currentTarget as HTMLInputElement).value) || 10)}
                />
              </label>
              <label class="text-sm">
                <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">DPI</span>
                <input
                  class="input input-sm mt-1 w-full"
                  type="number"
                  min="72"
                  step="1"
                  value={colorChartDpi}
                  oninput={(e) => (colorChartDpi = Number((e.currentTarget as HTMLInputElement).value) || 300)}
                />
              </label>
            </div>
          </div>
        </div>

        <div class="flex h-[clamp(18rem,60vh,32rem)] h-[clamp(18rem,60svh,32rem)] h-[clamp(18rem,60dvh,32rem)] items-center justify-center overflow-hidden rounded border border-surface-800/60 bg-white p-3">
          <div
            class="relative h-full w-full max-w-full max-h-full overflow-hidden rounded border border-surface-800/30 bg-white shadow-inner"
            style={`aspect-ratio:${Math.max(1, chartPageWmm)}/${Math.max(1, chartPageHmm)}; transform: translateZ(0);`}
            aria-label="ColorChecker chart preview"
          >
            <button
              type="button"
              class="absolute right-2 top-2 z-10 inline-flex h-9 w-9 items-center justify-center rounded border border-surface-800/60 bg-white/95 text-surface-900 shadow-sm transition hover:border-primary-500/50 hover:text-primary-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary-500/30"
              onclick={openColorChartPdf}
              aria-label="Download chart PDF"
              title={chartFitsOnPaper ? 'Download PDF' : 'Chart does not fit on selected paper'}
            >
              <svg viewBox="0 0 24 24" class="h-4.5 w-4.5" fill="currentColor" aria-hidden="true">
                <path
                  d="M12 3a1 1 0 0 1 1 1v8.586l2.293-2.293a1 1 0 1 1 1.414 1.414l-4 4a1 1 0 0 1-1.414 0l-4-4a1 1 0 1 1 1.414-1.414L11 12.586V4a1 1 0 0 1 1-1ZM5 19a1 1 0 0 1 1-1h12a1 1 0 1 1 0 2H6a1 1 0 0 1-1-1Z"
                />
              </svg>
            </button>

            <div class="absolute bottom-2 left-2 z-10 rounded border border-surface-800/50 bg-white/95 px-2 py-1 text-micro font-medium text-surface-900 shadow-sm">
              {chartPaperLabel}
            </div>

            <div
              class="absolute"
              style={`left:${chartRequiredXPct}%; top:${chartRequiredYPct}%; width:${chartRequiredWPct}%; height:${chartRequiredHPct}%;`}
            >
              <img
                class="absolute inset-0 h-full w-full bg-white"
                src={colorChartPngUrl}
                alt="ColorChecker chart"
                loading="eager"
                style="image-rendering:pixelated;"
              />
            </div>

            <svg class="absolute inset-0 h-full w-full" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
              <line x1={chartScaleXPct} y1={chartScaleBaseY} x2={chartScaleXPct + chartScaleLenPct} y2={chartScaleBaseY} stroke="rgba(0,0,0,0.8)" stroke-width="0.2" />
              <line
                x1={chartScaleXPct}
                y1={chartScaleBaseY - chartScaleTickHalf}
                x2={chartScaleXPct}
                y2={chartScaleBaseY + chartScaleTickHalf}
                stroke="rgba(0,0,0,0.8)"
                stroke-width="0.2"
              />
              <line
                x1={chartScaleXPct + chartScaleLenPct}
                y1={chartScaleBaseY - chartScaleTickHalf}
                x2={chartScaleXPct + chartScaleLenPct}
                y2={chartScaleBaseY + chartScaleTickHalf}
                stroke="rgba(0,0,0,0.8)"
                stroke-width="0.2"
              />
              <line
                x1={chartScaleMidXPct}
                y1={chartScaleBaseY - chartScaleTickHalf * 0.7}
                x2={chartScaleMidXPct}
                y2={chartScaleBaseY + chartScaleTickHalf * 0.7}
                stroke="rgba(0,0,0,0.7)"
                stroke-width="0.2"
              />
              <text x={chartScaleLabelLeftPct} y={chartScaleLabelY} text-anchor="end" font-size="2.2" fill="rgba(0,0,0,0.8)" font-weight="600">
                {`Scale bar: ${chartScaleLenMm.toFixed(0)} mm`}
              </text>
            </svg>
          </div>
        </div>
      </div>
    </div>
  </div>
{/if}
