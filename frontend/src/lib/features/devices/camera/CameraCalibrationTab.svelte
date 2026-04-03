<script lang="ts">
  import { onMount } from 'svelte';
  import CalibrationSetup from './calibration/CalibrationSetup.svelte';
  import CalibrationRun from './calibration/CalibrationRun.svelte';
  import CalibrationPreviewModal from './calibration/CalibrationPreviewModal.svelte';
  import CalibrationColorChartModal from './calibration/CalibrationColorChartModal.svelte';
  import CalibrationBoardCreatorModal from './calibration/CalibrationBoardCreatorModal.svelte';
  import type {
    CalibrationBoard,
    CalibrationImage,
    CalibrationParams,
    CalibrationResult,
    IpaStatus
  } from './cameraCalibrationTypes';
  import {
    formatMaybeNumber,
    overlayUrlForImage,
    pickPaperDims,
    type OrientationChoice,
    type PaperChoice
  } from './cameraCalibrationUtils';
  import { SvelteMap } from 'svelte/reactivity';

  const props = $props<{
    streamId: string;
    streamUuid: string | null;
    streamIdEffective: string;
    apiPath: (path: string) => string;
	    sourceResolution: { width: number; height: number } | null;
	    currentCalibrationParams: CalibrationParams | null;
	    guidedModeEnabled: boolean;
	    guidedModeBusy: boolean;
	    setGuidedModeEnabled: (enabled: boolean) => void;
	    resetGuidedCoverage: () => void;
	    guidedAccumulateLive: boolean;
	    setGuidedAccumulateLive: (enabled: boolean) => void;

    calibrationBoard: CalibrationBoard;
    calibrationLensModel: 'pinhole' | 'fisheye';
    setCalibrationLensModel: (value: 'pinhole' | 'fisheye') => void;
    calibrationImages: CalibrationImage[];
    calibrationOwnPhotosOnly: boolean;
    setCalibrationOwnPhotosOnly: (value: boolean) => void | Promise<void>;
    calibrationSelected: Record<string, boolean>;
    calibrationLoading: boolean;
    calibrationSolving: boolean;
    calibrationApplying: boolean;
    calibrationDeleting: boolean;
    calibrationIncludeOverlays: boolean;
    calibrationSolveError: string | null;
    calibrationResult: CalibrationResult | null;
    calibrationImportSourcesLoading: boolean;
    calibrationImporting: boolean;
    calibrationImportError: string | null;
    calibrationImportSourceId: string;
    calibrationImportSources: Array<{ id: string; label: string; calibration?: unknown }>;
    refreshCalibrationImages: () => Promise<void>;
    refreshCalibrationImportSources: () => Promise<void>;
    takeCalibrationSnapshot: () => Promise<void>;
    deleteCalibrationSnapshot: (name: string) => Promise<void>;
    deleteCalibrationOverlays: () => Promise<void>;
    solveCalibration: () => Promise<void>;
    saveSolvedCalibration: () => Promise<void>;
    copyCalibrationFromSelectedStream: () => Promise<void>;
    importCalibrationFromJsonFile: (file: File) => Promise<void>;

    calibrationPreviewOpen: boolean;
    calibrationPreviewItem: CalibrationImage | null;
    openCalibrationPreview: (item: CalibrationImage) => void;
    closeCalibrationPreview: () => void;
    setCalibrationSelected: (next: Record<string, boolean>) => void;
    setCalibrationIncludeOverlays: (value: boolean) => void;
    setCalibrationImportSourceId: (value: string) => void;

    ipaLoading: boolean;
    ipaStatus: IpaStatus | null;
    refreshIpaStatus: () => Promise<void>;
    applyIpaCcm: () => Promise<void>;
    openIpaChartSolverForImage: (name: string) => void;

    ipaCt: number;
    setIpaCt: (value: number) => void;
    ipaCcm: number[][];
    ipaAdvanced: boolean;
    setIpaAdvanced: (value: boolean) => void;
    ipaTarget: 'both' | 'pisp' | 'vc4';
    setIpaTarget: (value: 'both' | 'pisp' | 'vc4') => void;
    setIpaCcm: (value: number[][]) => void;

    ipaChartImage: string;
    setIpaChartImage: (value: string) => void;
    ipaChartModalOpen: boolean;
    closeIpaChartSolver: () => void;
    addIpaChartCorner: (event: MouseEvent) => void;
    ipaChartCorners: Array<[number, number]>;
    setIpaChartCorners: (value: Array<[number, number]>) => void;
    ipaChartNaturalSize: { w: number; h: number };
    setIpaChartNaturalSize: (value: { w: number; h: number }) => void;
    solveIpaChartCcm: () => Promise<void>;
    ipaChartSolveBusy: boolean;
    ipaChartSolveError: string | null;
    ipaChartSolveResult: { ccm: number[][]; rmsError: number } | null;
  }>();

  const apiPath = $derived(props.apiPath);
  const sourceResolution = $derived(props.sourceResolution);
  const currentCalibrationParams = $derived(props.currentCalibrationParams);

  const selectedCount = $derived(
    props.calibrationImages.reduce((count, item) => count + (props.calibrationSelected[item.name] ? 1 : 0), 0)
  );

  const chartSelectedName = $derived(props.ipaChartImage?.trim?.() ? props.ipaChartImage : null);
  const hasChartImage = $derived(Boolean(props.ipaChartImage?.trim?.().length));

  let boardCreatorOpen = $state(false);
  type BoardPaper = PaperChoice;
  type BoardOrientation = OrientationChoice;
  let boardPaper = $state<BoardPaper>('letter');
  let boardOrientation = $state<BoardOrientation>('portrait');
  let colorChartOpen = $state(false);
  type ChartPaper = PaperChoice;
  type ChartOrientation = OrientationChoice;
  let colorChartPaper = $state<ChartPaper>('auto');
  let colorChartOrientation = $state<ChartOrientation>('auto');
  let colorChartPatchMm = $state(25);
  let colorChartMarginMm = $state(10);
  let colorChartDpi = $state(300);

  const visibleSnapshots = $derived(props.calibrationImages ?? []);

  const pxPerMm = $derived(Number(props.calibrationBoard.dpi) / 25.4);
  const boardSquarePx = $derived(Math.max(16, Math.round(Number(props.calibrationBoard.squareMm) * pxPerMm)));
  const boardMarkerPx = $derived(Math.max(8, Math.min(boardSquarePx - 2, Math.round(Number(props.calibrationBoard.markerMm) * pxPerMm))));

  const boardPngUrl = $derived(
    apiPath(
      `/streams/${encodeURIComponent(props.streamIdEffective)}/calibration/board?squares_x=${props.calibrationBoard.squaresX}&squares_y=${props.calibrationBoard.squaresY}&square_px=${boardSquarePx}&marker_px=${boardMarkerPx}&dictionary=${encodeURIComponent(props.calibrationBoard.dictionary || '4x4_1000')}`
    )
  );

  const boardPdfUrl = $derived(
    apiPath(
      `/streams/${encodeURIComponent(props.streamIdEffective)}/calibration/board.pdf?squares_x=${props.calibrationBoard.squaresX}&squares_y=${props.calibrationBoard.squaresY}&square_mm=${props.calibrationBoard.squareMm}&marker_mm=${props.calibrationBoard.markerMm}&margin_mm=${props.calibrationBoard.marginMm}&dpi=${props.calibrationBoard.dpi}&paper=${encodeURIComponent(boardPaper)}&orientation=${encodeURIComponent(boardOrientation)}&dictionary=${encodeURIComponent(props.calibrationBoard.dictionary || '4x4_1000')}`
    )
  );

  const colorChartPngUrl = $derived(
    apiPath(
      `/device/ipa/ccm/chart?patch_mm=${colorChartPatchMm}&margin_mm=${colorChartMarginMm}&dpi=${colorChartDpi}`
    )
  );

  const colorChartPdfUrl = $derived(
    apiPath(
      `/device/ipa/ccm/chart.pdf?patch_mm=${colorChartPatchMm}&margin_mm=${colorChartMarginMm}&dpi=${colorChartDpi}&paper=${encodeURIComponent(colorChartPaper)}&orientation=${encodeURIComponent(colorChartOrientation)}`
    )
  );

  const boardWmm = $derived(Number(props.calibrationBoard.squaresX) * Number(props.calibrationBoard.squareMm));
  const boardHmm = $derived(Number(props.calibrationBoard.squaresY) * Number(props.calibrationBoard.squareMm));

  const boardLayout = $derived((() => {
    const marginMm = Math.max(0, Number(props.calibrationBoard.marginMm));
    const boardW = Math.max(0, Number(boardWmm));
    const boardH = Math.max(0, Number(boardHmm));
    const requiredWmm = boardW + 2 * marginMm;
    const requiredHmm = boardH + 2 * marginMm;
    const picked = pickPaperDims(requiredWmm, requiredHmm, boardPaper, boardOrientation);

    const pageWmm = Math.max(1, Number(picked.pageWmm));
    const pageHmm = Math.max(1, Number(picked.pageHmm));
    const extraWmm = Math.max(0, pageWmm - requiredWmm);
    const extraHmm = Math.max(0, pageHmm - requiredHmm);
    const imageXmm = marginMm + extraWmm * 0.5;
    const imageYmm = marginMm + extraHmm * 0.5;
    const requiredXmm = (pageWmm - requiredWmm) * 0.5;
    const requiredYmm = (pageHmm - requiredHmm) * 0.5;
    return {
      marginMm,
      boardWmm: boardW,
      boardHmm: boardH,
      requiredWmm,
      requiredHmm,
      pageWmm,
      pageHmm,
      imageXmm,
      imageYmm,
      requiredXmm,
      requiredYmm,
      fits: picked.fits,
      resolvedPaper: picked.resolvedPaper,
      resolvedOrientation: picked.resolvedOrientation
    };
  })());

  const boardPageWmm = $derived(boardLayout.pageWmm);
  const boardPageHmm = $derived(boardLayout.pageHmm);
  const boardFitsOnPaper = $derived(boardPaper === 'custom' ? true : boardLayout.fits);
  const boardPaperLabel = $derived((() => {
    const paperLabel =
      boardLayout.resolvedPaper === 'letter'
        ? 'Letter'
        : boardLayout.resolvedPaper === 'a4'
          ? 'A4'
          : 'Custom';
    const suffix = boardLayout.resolvedPaper === 'custom' ? '' : ` · ${boardLayout.resolvedOrientation}`;
    if (boardPaper === 'auto') return `Auto → ${paperLabel}${suffix}`;
    if (boardPaper === 'custom') return 'Custom (fit board)';
    return `${paperLabel}${suffix}`;
  })());

  const COLOR_CHART_COLS = 6;
  const COLOR_CHART_ROWS = 4;
  const chartWmm = $derived(COLOR_CHART_COLS * Number(colorChartPatchMm));
  const chartHmm = $derived(COLOR_CHART_ROWS * Number(colorChartPatchMm));

  const chartLayout = $derived((() => {
    const marginMm = Math.max(0, Number(colorChartMarginMm));
    const chartW = Math.max(0, Number(chartWmm));
    const chartH = Math.max(0, Number(chartHmm));
    const requiredWmm = chartW + 2 * marginMm;
    const requiredHmm = chartH + 2 * marginMm;
    const picked = pickPaperDims(requiredWmm, requiredHmm, colorChartPaper, colorChartOrientation);

    const pageWmm = Math.max(1, Number(picked.pageWmm));
    const pageHmm = Math.max(1, Number(picked.pageHmm));
    const extraWmm = Math.max(0, pageWmm - requiredWmm);
    const extraHmm = Math.max(0, pageHmm - requiredHmm);
    const imageXmm = marginMm + extraWmm * 0.5;
    const imageYmm = marginMm + extraHmm * 0.5;
    const requiredXmm = (pageWmm - requiredWmm) * 0.5;
    const requiredYmm = (pageHmm - requiredHmm) * 0.5;
    return {
      marginMm,
      chartWmm: chartW,
      chartHmm: chartH,
      requiredWmm,
      requiredHmm,
      pageWmm,
      pageHmm,
      imageXmm,
      imageYmm,
      requiredXmm,
      requiredYmm,
      fits: picked.fits,
      resolvedPaper: picked.resolvedPaper,
      resolvedOrientation: picked.resolvedOrientation
    };
  })());

  const chartPageWmm = $derived(chartLayout.pageWmm);
  const chartPageHmm = $derived(chartLayout.pageHmm);
  const chartFitsOnPaper = $derived(colorChartPaper === 'custom' ? true : chartLayout.fits);
  const chartPaperLabel = $derived((() => {
    const paperLabel =
      chartLayout.resolvedPaper === 'letter'
        ? 'Letter'
        : chartLayout.resolvedPaper === 'a4'
          ? 'A4'
          : 'Custom';
    const suffix = chartLayout.resolvedPaper === 'custom' ? '' : ` · ${chartLayout.resolvedOrientation}`;
    if (colorChartPaper === 'auto') return `Auto → ${paperLabel}${suffix}`;
    if (colorChartPaper === 'custom') return 'Custom (fit chart)';
    return `${paperLabel}${suffix}`;
  })());

  function handleIpaChartImageLoad(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLImageElement)) return;
    if (!target.naturalWidth || !target.naturalHeight) return;
    props.setIpaChartNaturalSize({ w: target.naturalWidth, h: target.naturalHeight });
  }

  const boardScalePadMm = 6;
  const boardScaleLabelGapMm = 3.8;
  const boardScaleLenMm = $derived(Math.min(100, Math.max(20, boardLayout.pageWmm - boardScalePadMm * 2)));
  const boardScaleXmm = $derived(Math.max(boardScalePadMm, boardLayout.pageWmm - boardScaleLenMm - boardScalePadMm));
  const boardScaleYmm = $derived(Math.max(6, boardLayout.pageHmm - boardScalePadMm - boardScaleLabelGapMm));
  const boardScaleLabelYmm = $derived(Math.min(boardLayout.pageHmm - 2, boardScaleYmm + boardScaleLabelGapMm));
  const boardScaleLabelXmm = $derived(Math.max(0, boardScaleXmm + boardScaleLenMm));
  const boardTickHalfMm = $derived(1.1);
  const boardTickHalfPct = $derived((boardTickHalfMm / Math.max(1, boardLayout.pageHmm)) * 100);

  const boardRequiredXPct = $derived((boardLayout.requiredXmm / Math.max(1, boardLayout.pageWmm)) * 100);
  const boardRequiredYPct = $derived((boardLayout.requiredYmm / Math.max(1, boardLayout.pageHmm)) * 100);
  const boardRequiredWPct = $derived((boardLayout.requiredWmm / Math.max(1, boardLayout.pageWmm)) * 100);
  const boardRequiredHPct = $derived((boardLayout.requiredHmm / Math.max(1, boardLayout.pageHmm)) * 100);
  const boardInnerLeftPct = $derived((boardLayout.marginMm / Math.max(1, boardLayout.requiredWmm)) * 100);
  const boardInnerTopPct = $derived((boardLayout.marginMm / Math.max(1, boardLayout.requiredHmm)) * 100);
  const boardInnerWPct = $derived((boardLayout.boardWmm / Math.max(1, boardLayout.requiredWmm)) * 100);
  const boardInnerHPct = $derived((boardLayout.boardHmm / Math.max(1, boardLayout.requiredHmm)) * 100);

  const boardScaleXPct = $derived((boardScaleXmm / Math.max(1, boardLayout.pageWmm)) * 100);
  const boardScaleLenPct = $derived((boardScaleLenMm / Math.max(1, boardLayout.pageWmm)) * 100);
  const boardScaleMidXPct = $derived(((boardScaleXmm + boardScaleLenMm * 0.5) / Math.max(1, boardLayout.pageWmm)) * 100);
  const boardScaleBottomPct = $derived((boardScaleYmm / Math.max(1, boardLayout.pageHmm)) * 100);
  const boardScaleLabelBottomPct = $derived((boardScaleLabelYmm / Math.max(1, boardLayout.pageHmm)) * 100);
  const boardScaleLabelLeftPct = $derived((boardScaleLabelXmm / Math.max(1, boardLayout.pageWmm)) * 100);

  const chartScaleLenMm = $derived(Math.min(100, Math.max(20, chartLayout.pageWmm - chartLayout.imageXmm * 2)));
  const chartScaleXmm = $derived(Math.max(0, (chartLayout.pageWmm - chartScaleLenMm) * 0.5));
  const chartScaleYmm = $derived(Math.min(8, Math.max(4, chartLayout.imageYmm * 0.4)));
  const chartScaleLabelYmm = $derived(Math.max(1, chartScaleYmm - 4.25));
  const chartScaleLabelXmm = $derived(Math.max(0, chartScaleXmm + chartScaleLenMm * 0.5 - 14.2));
  const chartTickHalfMm = $derived(1.1);
  const chartTickHalfPct = $derived((chartTickHalfMm / Math.max(1, chartLayout.pageHmm)) * 100);

  const chartRequiredXPct = $derived((chartLayout.requiredXmm / Math.max(1, chartLayout.pageWmm)) * 100);
  const chartRequiredYPct = $derived((chartLayout.requiredYmm / Math.max(1, chartLayout.pageHmm)) * 100);
  const chartRequiredWPct = $derived((chartLayout.requiredWmm / Math.max(1, chartLayout.pageWmm)) * 100);
  const chartRequiredHPct = $derived((chartLayout.requiredHmm / Math.max(1, chartLayout.pageHmm)) * 100);

  const chartScaleXPct = $derived((chartScaleXmm / Math.max(1, chartLayout.pageWmm)) * 100);
  const chartScaleLenPct = $derived((chartScaleLenMm / Math.max(1, chartLayout.pageWmm)) * 100);
  const chartScaleMidXPct = $derived(((chartScaleXmm + chartScaleLenMm * 0.5) / Math.max(1, chartLayout.pageWmm)) * 100);
  const chartScaleBottomPct = $derived((chartScaleYmm / Math.max(1, chartLayout.pageHmm)) * 100);
  const chartScaleLabelBottomPct = $derived((chartScaleLabelYmm / Math.max(1, chartLayout.pageHmm)) * 100);
  const chartScaleLabelLeftPct = $derived((chartScaleLabelXmm / Math.max(1, chartLayout.pageWmm)) * 100);

  function toggleSnapshot(name: string): void {
    const next = { ...props.calibrationSelected };
    if (next[name]) delete next[name];
    else next[name] = true;
    props.setCalibrationSelected(next);
  }

  function toggleChartImage(name: string): void {
    props.setIpaChartImage(props.ipaChartImage === name ? '' : name);
  }

  function requestDeleteSnapshot(name: string): void {
    if (props.calibrationDeleting) return;
    if (!confirm(`Delete "${name}"? This cannot be undone.`)) return;
    void props.deleteCalibrationSnapshot(name);
  }

  function selectAllSnapshots(): void {
    const next: Record<string, boolean> = {};
    for (const item of visibleSnapshots) next[item.name] = true;
    props.setCalibrationSelected(next);
  }

  function clearSelectedSnapshots(): void {
    props.setCalibrationSelected({});
  }

  async function requestDeleteSelectedSnapshots(): Promise<void> {
    if (props.calibrationDeleting) return;
    const selected = Object.entries(props.calibrationSelected)
      .filter(([, enabled]) => Boolean(enabled))
      .map(([name]) => name);
    if (!selected.length) return;
    const target = selected.length === 1 ? `"${selected[0]}"` : `${selected.length} selected snapshots`;
    if (!confirm(`Delete ${target}? This cannot be undone.`)) return;
    for (const name of selected) {
      await props.deleteCalibrationSnapshot(name);
    }
  }

  type BoardPreset = 'a4-standard' | 'a4-full' | 'letter-standard' | 'letter-full';

  const BOARD_PRESETS: Record<BoardPreset, {
    squaresX: number;
    squaresY: number;
    squareMm: number;
    markerMm: number;
    marginMm: number;
    dpi: number;
    paper: BoardPaper;
    orientation: BoardOrientation;
  }> = {
    'a4-standard': {
      squaresX: 9,
      squaresY: 13,
      squareMm: 20,
      markerMm: 14,
      marginMm: 10,
      dpi: 300,
      paper: 'a4',
      orientation: 'portrait'
    },
    'a4-full': {
      squaresX: 7,
      squaresY: 10,
      squareMm: 26,
      markerMm: 18,
      marginMm: 10,
      dpi: 300,
      paper: 'a4',
      orientation: 'portrait'
    },
    'letter-standard': {
      squaresX: 9,
      squaresY: 12,
      squareMm: 20,
      markerMm: 14,
      marginMm: 10,
      dpi: 300,
      paper: 'letter',
      orientation: 'portrait'
    },
    'letter-full': {
      squaresX: 8,
      squaresY: 10,
      squareMm: 24,
      markerMm: 17,
      marginMm: 10,
      dpi: 300,
      paper: 'letter',
      orientation: 'portrait'
    }
  };

  function applyBoardPreset(preset: BoardPreset): void {
    if (!props.calibrationBoard.dictionary || !props.calibrationBoard.dictionary.trim()) {
      props.calibrationBoard.dictionary = '4x4_1000';
    }
    const config = BOARD_PRESETS[preset];
    if (!config) return;
    props.calibrationBoard.squaresX = config.squaresX;
    props.calibrationBoard.squaresY = config.squaresY;
    props.calibrationBoard.squareMm = config.squareMm;
    props.calibrationBoard.markerMm = config.markerMm;
    props.calibrationBoard.marginMm = config.marginMm;
    props.calibrationBoard.dpi = config.dpi;
    boardPaper = config.paper;
    boardOrientation = config.orientation;
  }

  let calibrationTool = $state<'lens' | 'color'>('lens');
  const overlayByImage = $derived.by(() => {
    const map = new SvelteMap<string, string>();
    const views = props.calibrationResult?.debugViews;
    if (Array.isArray(views)) {
      for (const view of views) {
        if (view?.image && view?.overlay) {
          map.set(view.image, view.overlay);
        }
      }
    }
    return map;
  });

  function overlayUrlForImageForImage(imageName: string): string | null {
    return overlayUrlForImage(overlayByImage, props.apiPath, imageName);
  }

  onMount(() => {
    void props.refreshCalibrationImportSources();
  });
</script>

<div class="space-y-4">
  <div class="space-y-4">
    <CalibrationSetup
      bind:calibrationTool={calibrationTool}
      calibrationBoard={props.calibrationBoard}
      calibrationLensModel={props.calibrationLensModel}
      setCalibrationLensModel={props.setCalibrationLensModel}
      boardPreviewUrl={boardPngUrl}
      onOpenBoardCreator={() => (boardCreatorOpen = true)}
      colorChartPngUrl={colorChartPngUrl}
      colorChartPatchMm={colorChartPatchMm}
      colorChartMarginMm={colorChartMarginMm}
      colorChartDpi={colorChartDpi}
      onOpenColorChart={() => (colorChartOpen = true)}
      streamUuid={props.streamUuid}
      guidedModeEnabled={props.guidedModeEnabled}
      guidedModeBusy={props.guidedModeBusy}
      guidedAccumulateLive={props.guidedAccumulateLive}
      setGuidedModeEnabled={props.setGuidedModeEnabled}
      resetGuidedCoverage={props.resetGuidedCoverage}
      setGuidedAccumulateLive={props.setGuidedAccumulateLive}
      calibrationOwnPhotosOnly={props.calibrationOwnPhotosOnly}
      onSetCalibrationOwnPhotosOnly={(value) => void props.setCalibrationOwnPhotosOnly(value)}
      calibrationSelected={props.calibrationSelected}
      calibrationLoading={props.calibrationLoading}
      calibrationSolving={props.calibrationSolving}
      calibrationApplying={props.calibrationApplying}
      calibrationDeleting={props.calibrationDeleting}
      selectedCount={selectedCount}
      chartSelectedName={chartSelectedName}
      visibleSnapshots={visibleSnapshots}
      apiPath={apiPath}
      onTakeSnapshot={() => void props.takeCalibrationSnapshot()}
      onToggleSnapshot={toggleSnapshot}
      onSelectAllSnapshots={selectAllSnapshots}
      onClearSelected={clearSelectedSnapshots}
      onDeleteSelectedSnapshots={() => void requestDeleteSelectedSnapshots()}
      onDeleteSnapshot={requestDeleteSnapshot}
      onOpenPreview={props.openCalibrationPreview}
      ipaChartImage={props.ipaChartImage}
    />
  </div>
  <div class="space-y-4">
    <CalibrationRun
      calibrationTool={calibrationTool}
      calibrationBoard={props.calibrationBoard}
      streamUuid={props.streamUuid}
      currentCalibrationParams={currentCalibrationParams}
      sourceResolution={sourceResolution}
      selectedCount={selectedCount}
      calibrationResult={props.calibrationResult}
      calibrationSolveError={props.calibrationSolveError}
      calibrationSolving={props.calibrationSolving}
      calibrationApplying={props.calibrationApplying}
      calibrationLoading={props.calibrationLoading}
      calibrationImages={props.calibrationImages}
      calibrationDeleting={props.calibrationDeleting}
      calibrationIncludeOverlays={props.calibrationIncludeOverlays}
      calibrationImportSourcesLoading={props.calibrationImportSourcesLoading}
      calibrationImporting={props.calibrationImporting}
      calibrationImportError={props.calibrationImportError}
      calibrationImportSourceId={props.calibrationImportSourceId}
      calibrationImportSources={props.calibrationImportSources}
      apiPath={apiPath}
      formatMaybeNumber={formatMaybeNumber}
      overlayUrlForImage={overlayUrlForImageForImage}
      onToggleIncludeOverlays={(value) => props.setCalibrationIncludeOverlays(value)}
      onSetCalibrationImportSourceId={props.setCalibrationImportSourceId}
      onCopyCalibrationFromStream={() => void props.copyCalibrationFromSelectedStream()}
      onImportCalibrationJsonFile={(file) => void props.importCalibrationFromJsonFile(file)}
      onDeleteOverlays={() => void props.deleteCalibrationOverlays()}
      onSolveCalibration={() => void props.solveCalibration()}
      onSaveCalibration={() => void props.saveSolvedCalibration()}
      ipaStatus={props.ipaStatus}
      ipaLoading={props.ipaLoading}
      ipaCt={props.ipaCt}
      setIpaCt={props.setIpaCt}
      ipaAdvanced={props.ipaAdvanced}
      setIpaAdvanced={props.setIpaAdvanced}
      ipaTarget={props.ipaTarget}
      setIpaTarget={props.setIpaTarget}
      ipaCcm={props.ipaCcm}
      setIpaCcm={props.setIpaCcm}
      ipaChartImage={props.ipaChartImage}
      hasChartImage={hasChartImage}
      onToggleChartImage={toggleChartImage}
      onOpenPreview={props.openCalibrationPreview}
      onDeleteSnapshot={requestDeleteSnapshot}
      onOpenIpaChartSolverForImage={props.openIpaChartSolverForImage}
      onApplyIpaCcm={() => void props.applyIpaCcm()}
    />
  </div>
</div>

<CalibrationPreviewModal
  open={props.calibrationPreviewOpen}
  item={props.calibrationPreviewItem}
  calibrationSelected={props.calibrationSelected}
  ipaChartImage={props.ipaChartImage}
  calibrationDeleting={props.calibrationDeleting}
  apiPath={apiPath}
  overlayUrlForImage={overlayUrlForImageForImage}
  onClose={props.closeCalibrationPreview}
  onDelete={requestDeleteSnapshot}
  onToggleSnapshot={toggleSnapshot}
  onToggleChartImage={toggleChartImage}
  onOpenIpaChartSolverForImage={props.openIpaChartSolverForImage}
  onSetCalibrationTool={(tool) => (calibrationTool = tool)}
/>

{#if props.ipaChartModalOpen && props.ipaChartImage}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-surface-950/70 px-4"
    role="dialog"
    aria-modal="true"
    onclick={(event) => {
      if (event.target === event.currentTarget) props.closeIpaChartSolver();
    }}
    onkeydown={(event) => {
      if (event.key === 'Escape') props.closeIpaChartSolver();
    }}
    tabindex="-1"
  >
    <div class="relative w-full max-w-6xl rounded border border-surface-800/70 bg-surface-950/95 p-6 text-sm text-surface-400 shadow-2xl max-h-[92vh] max-h-[92svh] max-h-[92dvh] overflow-y-auto">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div class="min-w-0">
          <p class="text-micro uppercase tracking-[0.3em] text-surface-500">ColorChecker solve</p>
          <p class="mt-2 truncate font-mono text-xs text-surface-200">{props.ipaChartImage}</p>
          <p class="mt-1 text-xs text-surface-500">Click corners: TL → TR → BR → BL. Then Solve.</p>
        </div>
        <div class="flex flex-wrap gap-2">
          <button class="btn btn-xs preset-outline" type="button" onclick={() => props.setIpaChartCorners([])} disabled={props.ipaChartCorners.length === 0}>
            Reset corners
          </button>
          <button class="btn btn-xs preset-filled" type="button" onclick={() => void props.solveIpaChartCcm()} disabled={props.ipaChartSolveBusy || props.ipaChartCorners.length !== 4}>
            {props.ipaChartSolveBusy ? 'Solving…' : 'Solve'}
          </button>
          <button class="btn btn-xs preset-outline" type="button" onclick={props.closeIpaChartSolver}>
            Close
          </button>
        </div>
      </div>

      {#if props.ipaChartSolveError}
        <p class="mt-3 text-sm text-error-300">{props.ipaChartSolveError}</p>
      {/if}

      <div class="mt-4 grid gap-6 xl:grid-cols-[1.4fr_1fr]">
	        <div class="min-w-0">
	          <div class="relative min-w-0 overflow-hidden rounded border border-surface-800/60 bg-surface-950/40">
	            <button class="block w-full" type="button" onclick={props.addIpaChartCorner} aria-label="Pick chart corners">
	              <img
	                class="block h-auto w-full select-none"
	                src={apiPath(`/media/${encodeURIComponent(props.ipaChartImage)}`)}
	                alt={props.ipaChartImage}
	                onload={handleIpaChartImageLoad}
	              />
	            </button>
	            {#each props.ipaChartCorners as pt, idx (idx)}
	              {@const xPct = `${(pt[0] / Math.max(1, props.ipaChartNaturalSize.w)) * 100}%`}
	              {@const yPct = `${(pt[1] / Math.max(1, props.ipaChartNaturalSize.h)) * 100}%`}
	              <div
                class="absolute -translate-x-1/2 -translate-y-1/2 rounded-full border border-surface-950 bg-primary-500/80 text-micro-tight text-surface-950"
                style={`left:${xPct}; top:${yPct}; width:18px; height:18px; display:flex; align-items:center; justify-content:center;`}
              >
                {idx + 1}
              </div>
            {/each}
          </div>
        </div>

        <div class="space-y-4">
          <div class="rounded border border-surface-800/60 bg-surface-900/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Corners</p>
            <p class="mt-2 text-sm text-surface-200">{props.ipaChartCorners.length}/4 selected</p>
          </div>
          <div class="rounded border border-surface-800/60 bg-surface-900/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Solved CCM</p>
            {#if props.ipaChartSolveResult}
              <p class="mt-2 text-xs text-surface-400">RMS error {props.ipaChartSolveResult.rmsError.toFixed(4)}</p>
            {:else}
              <p class="mt-2 text-xs text-surface-500">Solve to populate the CCM matrix.</p>
            {/if}
	            <div class="mt-3 grid gap-2">
	              {#each [0, 1, 2] as row (row)}
	                <div class="rounded border border-surface-800/60 bg-surface-950/30 px-3 py-2 font-mono text-xs text-surface-200">
	                  {Number(props.ipaCcm?.[row]?.[0] ?? 0).toFixed(4)} {Number(props.ipaCcm?.[row]?.[1] ?? 0).toFixed(4)} {Number(props.ipaCcm?.[row]?.[2] ?? 0).toFixed(4)}
	                </div>
	              {/each}
	            </div>
          </div>
          <p class="text-xs text-surface-500">Apply CCM in the main Calibration tab to write it into the IPA JSON and restart the engine.</p>
        </div>
      </div>
    </div>
  </div>
{/if}

<CalibrationColorChartModal
  bind:open={colorChartOpen}
  bind:colorChartPaper
  bind:colorChartOrientation
  bind:colorChartPatchMm
  bind:colorChartMarginMm
  bind:colorChartDpi
  {chartPageWmm}
  {chartPageHmm}
  {chartFitsOnPaper}
  {chartPaperLabel}
  {chartRequiredXPct}
  {chartRequiredYPct}
  {chartRequiredWPct}
  {chartRequiredHPct}
  {chartScaleXPct}
  {chartScaleLenPct}
  {chartScaleBottomPct}
  {chartTickHalfPct}
  {chartScaleMidXPct}
  {chartScaleLabelLeftPct}
  {chartScaleLabelBottomPct}
  {chartScaleLenMm}
  {colorChartPdfUrl}
  {colorChartPngUrl}
/>

<CalibrationBoardCreatorModal
  bind:open={boardCreatorOpen}
  bind:boardPaper
  bind:boardOrientation
  {boardPaperLabel}
  {boardPageWmm}
  {boardPageHmm}
  {boardFitsOnPaper}
  boardRequiredWmm={boardLayout.requiredWmm}
  boardRequiredHmm={boardLayout.requiredHmm}
  {boardRequiredXPct}
  {boardRequiredYPct}
  {boardRequiredWPct}
  {boardRequiredHPct}
  {boardInnerLeftPct}
  {boardInnerTopPct}
  {boardInnerWPct}
  {boardInnerHPct}
  {boardScaleXPct}
  {boardScaleLenPct}
  {boardScaleBottomPct}
  {boardTickHalfPct}
  {boardScaleMidXPct}
  {boardScaleLabelLeftPct}
  {boardScaleLabelBottomPct}
  {boardScaleLenMm}
  {boardPdfUrl}
  {boardPngUrl}
  calibrationBoard={props.calibrationBoard}
  {applyBoardPreset}
/>
