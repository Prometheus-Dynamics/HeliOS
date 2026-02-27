<script lang="ts">
  import type { CalibrationBoard } from '../cameraCalibrationTypes';
  import type { OrientationChoice, PaperChoice } from '../cameraCalibrationUtils';

  type BoardPreset = 'a4-standard' | 'a4-full' | 'letter-standard' | 'letter-full';

  type Props = {
    open: boolean;
    boardPaper: PaperChoice;
    boardOrientation: OrientationChoice;
    boardPaperLabel: string;
    boardPageWmm: number;
    boardPageHmm: number;
    boardFitsOnPaper: boolean;
    boardRequiredWmm: number;
    boardRequiredHmm: number;
    boardRequiredXPct: number;
    boardRequiredYPct: number;
    boardRequiredWPct: number;
    boardRequiredHPct: number;
    boardInnerLeftPct: number;
    boardInnerTopPct: number;
    boardInnerWPct: number;
    boardInnerHPct: number;
    boardScaleXPct: number;
    boardScaleLenPct: number;
    boardScaleBottomPct: number;
    boardTickHalfPct: number;
    boardScaleMidXPct: number;
    boardScaleLabelLeftPct: number;
    boardScaleLabelBottomPct: number;
    boardScaleLenMm: number;
    boardPdfUrl: string;
    boardPngUrl: string;
    calibrationBoard: CalibrationBoard;
    applyBoardPreset: (preset: BoardPreset) => void;
  };

  let {
    open = $bindable(false),
    boardPaper = $bindable('letter'),
    boardOrientation = $bindable('portrait'),
    boardPaperLabel,
    boardPageWmm,
    boardPageHmm,
    boardFitsOnPaper,
    boardRequiredWmm,
    boardRequiredHmm,
    boardRequiredXPct,
    boardRequiredYPct,
    boardRequiredWPct,
    boardRequiredHPct,
    boardInnerLeftPct,
    boardInnerTopPct,
    boardInnerWPct,
    boardInnerHPct,
    boardScaleXPct,
    boardScaleLenPct,
    boardScaleBottomPct,
    boardTickHalfPct,
    boardScaleMidXPct,
    boardScaleLabelLeftPct,
    boardScaleLabelBottomPct,
    boardScaleLenMm,
    boardPdfUrl,
    boardPngUrl,
    calibrationBoard,
    applyBoardPreset
  }: Props = $props();

  const DEFAULT_BOARD = {
    squaresX: 9,
    squaresY: 12,
    squareMm: 20,
    markerMm: 14,
    marginMm: 10,
    dpi: 300,
    dictionary: '4x4_1000'
  };

  const ARUCO_DICTIONARIES = [
    { value: '4x4_50', label: '4x4_50 (50 tags)' },
    { value: '4x4_100', label: '4x4_100 (100 tags)' },
    { value: '4x4_250', label: '4x4_250 (250 tags)' },
    { value: '4x4_1000', label: '4x4_1000 (1000 tags)' },
    { value: '5x5_50', label: '5x5_50 (50 tags)' },
    { value: '5x5_100', label: '5x5_100 (100 tags)' },
    { value: '5x5_250', label: '5x5_250 (250 tags)' },
    { value: '5x5_1000', label: '5x5_1000 (1000 tags)' },
    { value: '6x6_50', label: '6x6_50 (50 tags)' },
    { value: '6x6_100', label: '6x6_100 (100 tags)' },
    { value: '6x6_250', label: '6x6_250 (250 tags)' },
    { value: '6x6_1000', label: '6x6_1000 (1000 tags)' },
    { value: '7x7_50', label: '7x7_50 (50 tags)' },
    { value: '7x7_100', label: '7x7_100 (100 tags)' },
    { value: '7x7_250', label: '7x7_250 (250 tags)' },
    { value: '7x7_1000', label: '7x7_1000 (1000 tags)' },
    { value: 'aruco_original', label: 'aruco_original (1024 tags)' },
    { value: 'aruco_mip_36h12', label: 'aruco_mip_36h12 (36h12)' }
  ];

  $effect(() => {
    if (!open || !calibrationBoard) return;
    if (!Number.isFinite(calibrationBoard.squaresX) || calibrationBoard.squaresX <= 0) calibrationBoard.squaresX = DEFAULT_BOARD.squaresX;
    if (!Number.isFinite(calibrationBoard.squaresY) || calibrationBoard.squaresY <= 0) calibrationBoard.squaresY = DEFAULT_BOARD.squaresY;
    if (!Number.isFinite(calibrationBoard.squareMm) || calibrationBoard.squareMm <= 0) calibrationBoard.squareMm = DEFAULT_BOARD.squareMm;
    if (!Number.isFinite(calibrationBoard.markerMm) || calibrationBoard.markerMm <= 0) calibrationBoard.markerMm = DEFAULT_BOARD.markerMm;
    if (!Number.isFinite(calibrationBoard.marginMm) || calibrationBoard.marginMm < 0) calibrationBoard.marginMm = DEFAULT_BOARD.marginMm;
    if (!Number.isFinite(calibrationBoard.dpi) || calibrationBoard.dpi <= 0) calibrationBoard.dpi = DEFAULT_BOARD.dpi;
    if (!calibrationBoard.dictionary || !calibrationBoard.dictionary.trim()) calibrationBoard.dictionary = DEFAULT_BOARD.dictionary;
  });

  const boardScaleBaseY = $derived(100 - boardScaleBottomPct);
  const boardScaleLabelY = $derived(100 - boardScaleLabelBottomPct);
  const boardScaleTickHalf = $derived(boardTickHalfPct);
</script>

{#if open}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-surface-950/70 px-4"
    role="dialog"
    aria-modal="true"
    aria-label="Create ChArUco board"
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
          <p class="text-micro uppercase tracking-[0.3em] text-surface-500">ChArUco board</p>
          <p class="mt-2 text-xs text-surface-500">Customize the board, then print the PDF at 100% scale.</p>
        </div>
        <div class="flex flex-wrap gap-2">
          <button class="btn btn-xs preset-outline" type="button" onclick={() => (open = false)}>
            Close
          </button>
        </div>
      </div>

      <div class="mt-4 grid gap-4 lg:grid-cols-[22rem,minmax(0,1fr)]">
        <div class="space-y-4">
          <div class="rounded border border-surface-800/60 bg-surface-900/40 p-4">
            <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Presets</p>
            <div class="mt-3 flex flex-wrap gap-2">
              <button class="btn btn-xs preset-tonal" type="button" onclick={() => applyBoardPreset('a4-standard')}>
                A4 Standard
              </button>
              <button class="btn btn-xs preset-tonal" type="button" onclick={() => applyBoardPreset('a4-full')}>
                A4 Full
              </button>
              <button class="btn btn-xs preset-tonal" type="button" onclick={() => applyBoardPreset('letter-standard')}>
                Letter Standard
              </button>
              <button class="btn btn-xs preset-tonal" type="button" onclick={() => applyBoardPreset('letter-full')}>
                Letter Full
              </button>
            </div>
          </div>

          <div class="rounded border border-surface-800/60 bg-surface-900/40 p-4">
            <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Paper</p>
            <div class="mt-3 grid gap-3 [grid-template-columns:repeat(auto-fit,minmax(10rem,1fr))]">
              <label class="text-sm">
                <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Size</span>
                <select
                  class="select select-sm mt-1 w-full"
                  value={boardPaper}
                  onchange={(e) => {
                    const value = (e.currentTarget as HTMLSelectElement).value;
                    if (value === 'auto' || value === 'letter' || value === 'a4' || value === 'custom') boardPaper = value;
                  }}
                >
                  <option value="auto">Auto</option>
                  <option value="letter">Letter (8.5×11")</option>
                  <option value="a4">A4 (210×297mm)</option>
                  <option value="custom">Custom (fit board)</option>
                </select>
              </label>

              <label class="text-sm">
                <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Orientation</span>
                <select
                  class="select select-sm mt-1 w-full"
                  value={boardOrientation}
                  disabled={boardPaper === 'custom'}
                  onchange={(e) => {
                    const value = (e.currentTarget as HTMLSelectElement).value;
                    if (value === 'auto' || value === 'portrait' || value === 'landscape') boardOrientation = value;
                  }}
                >
                  <option value="auto">Auto</option>
                  <option value="portrait">Portrait</option>
                  <option value="landscape">Landscape</option>
                </select>
              </label>
            </div>

            <div class="mt-3 space-y-1 text-xs text-surface-500">
              <p>
                <span class="text-surface-200">{boardPaperLabel}</span> · Page{' '}
                <span class="font-mono text-surface-200">
                  {boardPageWmm.toFixed(1)}×{boardPageHmm.toFixed(1)}mm
                </span>
              </p>
              <p>
                Required{' '}
                <span class="font-mono text-surface-200">
                  {boardRequiredWmm.toFixed(1)}×{boardRequiredHmm.toFixed(1)}mm
                </span>{' '}
                (board + margins)
              </p>
              {#if !boardFitsOnPaper}
                <p class="text-rose-200">Does not fit on selected paper; reduce size, change orientation, or use Auto/Custom.</p>
              {/if}
            </div>
          </div>

          <div class="rounded border border-surface-800/60 bg-surface-900/40 p-4">
            <p class="text-2xs uppercase tracking-[0.3em] text-surface-500">Dimensions</p>
            <div class="mt-3 grid gap-3 [grid-template-columns:repeat(auto-fit,minmax(10rem,1fr))]">
              <label class="text-sm">
                <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Squares X</span>
                <input
                  class="input input-sm mt-1 w-full"
                  type="number"
                  min="2"
                  step="1"
                  value={calibrationBoard.squaresX}
                  oninput={(e) => (calibrationBoard.squaresX = Number((e.currentTarget as HTMLInputElement).value) || DEFAULT_BOARD.squaresX)}
                />
              </label>
              <label class="text-sm">
                <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Squares Y</span>
                <input
                  class="input input-sm mt-1 w-full"
                  type="number"
                  min="2"
                  step="1"
                  value={calibrationBoard.squaresY}
                  oninput={(e) => (calibrationBoard.squaresY = Number((e.currentTarget as HTMLInputElement).value) || DEFAULT_BOARD.squaresY)}
                />
              </label>
              <label class="text-sm">
                <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Square mm</span>
                <input
                  class="input input-sm mt-1 w-full"
                  type="number"
                  min="2"
                  step="0.1"
                  value={calibrationBoard.squareMm}
                  oninput={(e) => {
                    const next = Number((e.currentTarget as HTMLInputElement).value) || 25;
                    calibrationBoard.squareMm = next;
                    if (!calibrationBoard.markerMm || calibrationBoard.markerMm >= next) {
                      calibrationBoard.markerMm = Math.max(1, Math.round(next * 0.7 * 10) / 10);
                    }
                  }}
                />
              </label>
              <label class="text-sm">
                <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Marker mm</span>
                <input
                  class="input input-sm mt-1 w-full"
                  type="number"
                  min="1"
                  step="0.1"
                  value={calibrationBoard.markerMm}
                  oninput={(e) => (calibrationBoard.markerMm = Number((e.currentTarget as HTMLInputElement).value) || calibrationBoard.markerMm)}
                />
              </label>
              <label class="text-sm">
                <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Margin mm</span>
                <input
                  class="input input-sm mt-1 w-full"
                  type="number"
                  min="0"
                  step="0.5"
                  value={calibrationBoard.marginMm}
                  oninput={(e) => (calibrationBoard.marginMm = Number((e.currentTarget as HTMLInputElement).value) || DEFAULT_BOARD.marginMm)}
                />
              </label>
              <label class="text-sm">
                <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">DPI</span>
                <input
                  class="input input-sm mt-1 w-full"
                  type="number"
                  min="72"
                  step="1"
                  value={calibrationBoard.dpi}
                  oninput={(e) => (calibrationBoard.dpi = Number((e.currentTarget as HTMLInputElement).value) || DEFAULT_BOARD.dpi)}
                />
              </label>
              <label class="text-sm">
                <span class="text-2xs uppercase tracking-[0.3em] text-surface-500">Dictionary</span>
                <select
                  class="select select-sm mt-1 w-full"
                  value={calibrationBoard.dictionary || DEFAULT_BOARD.dictionary}
                  onchange={(e) => {
                    const value = (e.currentTarget as HTMLSelectElement).value;
                    calibrationBoard.dictionary = value || DEFAULT_BOARD.dictionary;
                  }}
                >
                  {#each ARUCO_DICTIONARIES as dict}
                    <option value={dict.value}>{dict.label}</option>
                  {/each}
                </select>
              </label>
            </div>
            <p class="mt-2 text-xs text-surface-500">ArUco dictionary used for marker IDs (default: 4x4_1000).</p>
          </div>
        </div>

        <div class="flex h-[clamp(18rem,60vh,32rem)] h-[clamp(18rem,60svh,32rem)] h-[clamp(18rem,60dvh,32rem)] items-center justify-center overflow-hidden rounded border border-surface-800/60 bg-white p-3">
          <div
            class="relative h-full w-auto max-w-full overflow-hidden rounded border border-surface-800/30 bg-white shadow-inner"
            style={`aspect-ratio:${Math.max(1, boardPageWmm)}/${Math.max(1, boardPageHmm)}; transform: translateZ(0);`}
            aria-label="ChArUco board preview"
          >
            <a
              class="absolute right-2 top-2 z-10 inline-flex h-9 w-9 items-center justify-center rounded border border-surface-800/60 bg-white/95 text-surface-900 shadow-sm transition hover:border-primary-500/50 hover:text-primary-700 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary-500/30"
              href={boardPdfUrl}
              aria-label="Download board PDF"
              title={boardFitsOnPaper ? 'Download PDF' : 'Board does not fit on selected paper'}
            >
              <svg viewBox="0 0 24 24" class="h-4.5 w-4.5" fill="currentColor" aria-hidden="true">
                <path
                  d="M12 3a1 1 0 0 1 1 1v8.586l2.293-2.293a1 1 0 1 1 1.414 1.414l-4 4a1 1 0 0 1-1.414 0l-4-4a1 1 0 1 1 1.414-1.414L11 12.586V4a1 1 0 0 1 1-1ZM5 19a1 1 0 0 1 1-1h12a1 1 0 1 1 0 2H6a1 1 0 0 1-1-1Z"
                />
              </svg>
            </a>

            <div class="absolute bottom-2 left-2 z-10 rounded border border-surface-800/50 bg-white/95 px-2 py-1 text-micro font-medium text-surface-900 shadow-sm">
              {boardPaperLabel}
            </div>

            <div
              class="absolute"
              style={`left:${boardRequiredXPct}%; top:${boardRequiredYPct}%; width:${boardRequiredWPct}%; height:${boardRequiredHPct}%;`}
            >
              <img
                class="absolute bg-white"
                src={boardPngUrl}
                alt="ChArUco board"
                loading="eager"
                style={`left:${boardInnerLeftPct}%; top:${boardInnerTopPct}%; width:${boardInnerWPct}%; height:${boardInnerHPct}%; image-rendering:pixelated; transform: translateZ(0);`}
              />
            </div>

            <svg class="absolute inset-0 h-full w-full" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
              <line x1={boardScaleXPct} y1={boardScaleBaseY} x2={boardScaleXPct + boardScaleLenPct} y2={boardScaleBaseY} stroke="rgba(0,0,0,0.8)" stroke-width="0.2" />
              <line
                x1={boardScaleXPct}
                y1={boardScaleBaseY - boardScaleTickHalf}
                x2={boardScaleXPct}
                y2={boardScaleBaseY + boardScaleTickHalf}
                stroke="rgba(0,0,0,0.8)"
                stroke-width="0.2"
              />
              <line
                x1={boardScaleXPct + boardScaleLenPct}
                y1={boardScaleBaseY - boardScaleTickHalf}
                x2={boardScaleXPct + boardScaleLenPct}
                y2={boardScaleBaseY + boardScaleTickHalf}
                stroke="rgba(0,0,0,0.8)"
                stroke-width="0.2"
              />
              <line
                x1={boardScaleMidXPct}
                y1={boardScaleBaseY - boardScaleTickHalf * 0.7}
                x2={boardScaleMidXPct}
                y2={boardScaleBaseY + boardScaleTickHalf * 0.7}
                stroke="rgba(0,0,0,0.7)"
                stroke-width="0.2"
              />
              <text x={boardScaleLabelLeftPct} y={boardScaleLabelY} text-anchor="end" font-size="2.2" fill="rgba(0,0,0,0.8)" font-weight="600">
                {`Scale bar: ${boardScaleLenMm.toFixed(0)} mm`}
              </text>
            </svg>
          </div>
        </div>
      </div>
    </div>
  </div>
{/if}
