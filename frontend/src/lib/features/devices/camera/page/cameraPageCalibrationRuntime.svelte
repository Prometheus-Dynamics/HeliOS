<script lang="ts" module>
  import type { StreamInfo } from '$lib/api/httpClient';
  import { toaster } from '$lib';
  import { reportError } from '$lib/ui/errorPolicy';
  import { createCameraCalibrationState } from './cameraCalibrationStore.svelte';
  import { createCameraCalibrationController } from './cameraCalibrationController';
  import { normalizeCalibrationParams, normalizeCalibrationSolveResult } from './cameraStateUtils';

  type CalibrationRuntimeDeps = {
    streamId: string;
    stream: () => StreamInfo | null;
    apiPath: (path: string) => string;
    refresh: () => Promise<void>;
  };

  export function createCameraPageCalibrationRuntime(deps: CalibrationRuntimeDeps) {
    const { streamId, stream, apiPath, refresh } = deps;

    const calibrationState = createCameraCalibrationState();

    const mediaState = $state({ mediaTabReady: true });
    const benchmarkState = $state({ benchmarkReady: true });

    const calibrationController = createCameraCalibrationController(
      {
        get stream() {
          return stream();
        },
        get streamId() {
          return streamId;
        },
        get calibrationBoard() {
          return calibrationState.calibrationBoard;
        },
        set calibrationBoard(value) {
          calibrationState.calibrationBoard = value;
        },
        get calibrationLensModel() {
          return calibrationState.calibrationLensModel;
        },
        set calibrationLensModel(value) {
          calibrationState.calibrationLensModel = value;
        },
        get calibrationImages() {
          return calibrationState.calibrationImages;
        },
        set calibrationImages(value) {
          calibrationState.calibrationImages = value;
        },
        get calibrationOwnPhotosOnly() {
          return calibrationState.calibrationOwnPhotosOnly;
        },
        set calibrationOwnPhotosOnly(value) {
          calibrationState.calibrationOwnPhotosOnly = value;
        },
        get calibrationSelected() {
          return calibrationState.calibrationSelected;
        },
        set calibrationSelected(value) {
          calibrationState.calibrationSelected = value;
        },
        get calibrationLoading() {
          return calibrationState.calibrationLoading;
        },
        set calibrationLoading(value) {
          calibrationState.calibrationLoading = value;
        },
        get calibrationSolving() {
          return calibrationState.calibrationSolving;
        },
        set calibrationSolving(value) {
          calibrationState.calibrationSolving = value;
        },
        get calibrationApplying() {
          return calibrationState.calibrationApplying;
        },
        set calibrationApplying(value) {
          calibrationState.calibrationApplying = value;
        },
        get calibrationDeleting() {
          return calibrationState.calibrationDeleting;
        },
        set calibrationDeleting(value) {
          calibrationState.calibrationDeleting = value;
        },
        get calibrationSolveError() {
          return calibrationState.calibrationSolveError;
        },
        set calibrationSolveError(value) {
          calibrationState.calibrationSolveError = value;
        },
        get calibrationGuidedMode() {
          return calibrationState.calibrationGuidedMode;
        },
        set calibrationGuidedMode(value) {
          calibrationState.calibrationGuidedMode = value;
        },
        get calibrationGuidedBusy() {
          return calibrationState.calibrationGuidedBusy;
        },
        set calibrationGuidedBusy(value) {
          calibrationState.calibrationGuidedBusy = value;
        },
        get calibrationGuidedResetToken() {
          return calibrationState.calibrationGuidedResetToken;
        },
        set calibrationGuidedResetToken(value) {
          calibrationState.calibrationGuidedResetToken = value;
        },
        get calibrationGuidedCaptureToken() {
          return calibrationState.calibrationGuidedCaptureToken;
        },
        set calibrationGuidedCaptureToken(value) {
          calibrationState.calibrationGuidedCaptureToken = value;
        },
        get calibrationGuidedAccumulateLive() {
          return calibrationState.calibrationGuidedAccumulateLive;
        },
        set calibrationGuidedAccumulateLive(value) {
          calibrationState.calibrationGuidedAccumulateLive = value;
        },
        get calibrationIncludeOverlays() {
          return calibrationState.calibrationIncludeOverlays;
        },
        set calibrationIncludeOverlays(value) {
          calibrationState.calibrationIncludeOverlays = value;
        },
        get calibrationResult() {
          return calibrationState.calibrationResult;
        },
        set calibrationResult(value) {
          calibrationState.calibrationResult = value;
        },
        get calibrationImportSourcesLoading() {
          return calibrationState.calibrationImportSourcesLoading;
        },
        set calibrationImportSourcesLoading(value) {
          calibrationState.calibrationImportSourcesLoading = value;
        },
        get calibrationImporting() {
          return calibrationState.calibrationImporting;
        },
        set calibrationImporting(value) {
          calibrationState.calibrationImporting = value;
        },
        get calibrationImportError() {
          return calibrationState.calibrationImportError;
        },
        set calibrationImportError(value) {
          calibrationState.calibrationImportError = value;
        },
        get calibrationImportSourceId() {
          return calibrationState.calibrationImportSourceId;
        },
        set calibrationImportSourceId(value) {
          calibrationState.calibrationImportSourceId = value;
        },
        get calibrationImportSources() {
          return calibrationState.calibrationImportSources;
        },
        set calibrationImportSources(value) {
          calibrationState.calibrationImportSources = value;
        },
        get calibrationPreviewOpen() {
          return calibrationState.calibrationPreviewOpen;
        },
        set calibrationPreviewOpen(value) {
          calibrationState.calibrationPreviewOpen = value;
        },
        get calibrationPreviewItem() {
          return calibrationState.calibrationPreviewItem;
        },
        set calibrationPreviewItem(value) {
          calibrationState.calibrationPreviewItem = value;
        },
        get ipaLoading() {
          return calibrationState.ipaLoading;
        },
        set ipaLoading(value) {
          calibrationState.ipaLoading = value;
        },
        get ipaStatus() {
          return calibrationState.ipaStatus;
        },
        set ipaStatus(value) {
          calibrationState.ipaStatus = value;
        },
        get ipaTarget() {
          return calibrationState.ipaTarget;
        },
        set ipaTarget(value) {
          calibrationState.ipaTarget = value;
        },
        get ipaCt() {
          return calibrationState.ipaCt;
        },
        set ipaCt(value) {
          calibrationState.ipaCt = value;
        },
        get ipaCcm() {
          return calibrationState.ipaCcm;
        },
        set ipaCcm(value) {
          calibrationState.ipaCcm = value;
        },
        get ipaAdvanced() {
          return calibrationState.ipaAdvanced;
        },
        set ipaAdvanced(value) {
          calibrationState.ipaAdvanced = value;
        },
        get ipaChartModalOpen() {
          return calibrationState.ipaChartModalOpen;
        },
        set ipaChartModalOpen(value) {
          calibrationState.ipaChartModalOpen = value;
        },
        get ipaChartImage() {
          return calibrationState.ipaChartImage;
        },
        set ipaChartImage(value) {
          calibrationState.ipaChartImage = value;
        },
        get ipaChartCorners() {
          return calibrationState.ipaChartCorners;
        },
        set ipaChartCorners(value) {
          calibrationState.ipaChartCorners = value;
        },
        get ipaChartNaturalSize() {
          return calibrationState.ipaChartNaturalSize;
        },
        set ipaChartNaturalSize(value) {
          calibrationState.ipaChartNaturalSize = value;
        },
        get ipaChartSolveBusy() {
          return calibrationState.ipaChartSolveBusy;
        },
        set ipaChartSolveBusy(value) {
          calibrationState.ipaChartSolveBusy = value;
        },
        get ipaChartSolveError() {
          return calibrationState.ipaChartSolveError;
        },
        set ipaChartSolveError(value) {
          calibrationState.ipaChartSolveError = value;
        },
        get ipaChartSolveResult() {
          return calibrationState.ipaChartSolveResult;
        },
        set ipaChartSolveResult(value) {
          calibrationState.ipaChartSolveResult = value;
        }
      },
      {
        apiPath,
        toaster,
        reportError,
        refresh,
        normalizeCalibrationSolveResult,
        normalizeCalibrationParams
      }
    );

    const {
      refreshIpaStatus,
      applyIpaCcm,
      openCalibrationPreview,
      closeCalibrationPreview,
      openIpaChartSolverForImage,
      closeIpaChartSolver,
      addIpaChartCorner,
      solveIpaChartCcm,
      setGuidedCalibrationMode,
      resetGuidedCalibrationCoverage,
      setCalibrationOwnPhotosOnly,
      refreshCalibrationImages,
      refreshCalibrationImportSources,
      takeCalibrationSnapshot,
      deleteCalibrationSnapshot,
      deleteCalibrationOverlays,
      solveCalibration,
      saveSolvedCalibration,
      copyCalibrationFromSelectedStream,
      importCalibrationFromJsonFile
    } = calibrationController;

    return {
      calibrationState,
      mediaTabReady: mediaState.mediaTabReady,
      benchmarkReady: benchmarkState.benchmarkReady,
      refreshIpaStatus,
      applyIpaCcm,
      openCalibrationPreview,
      closeCalibrationPreview,
      openIpaChartSolverForImage,
      closeIpaChartSolver,
      addIpaChartCorner,
      solveIpaChartCcm,
      setGuidedCalibrationMode,
      resetGuidedCalibrationCoverage,
      setCalibrationOwnPhotosOnly,
      refreshCalibrationImages,
      refreshCalibrationImportSources,
      takeCalibrationSnapshot,
      deleteCalibrationSnapshot,
      deleteCalibrationOverlays,
      solveCalibration,
      saveSolvedCalibration,
      copyCalibrationFromSelectedStream,
      importCalibrationFromJsonFile
    };
  }
</script>
