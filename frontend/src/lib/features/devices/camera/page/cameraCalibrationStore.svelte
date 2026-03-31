<script lang="ts" module>
  import type { CalibrationImage, CalibrationParams, CalibrationResult, IpaStatus } from '$lib/features/devices/camera/cameraCalibrationTypes';

  export function createCameraCalibrationState() {
    const state = $state({
      calibrationBoard: {
        squaresX: 9,
        squaresY: 12,
        markerMm: 14,
        squareMm: 20,
        marginMm: 10,
        dpi: 300,
        dictionary: '4x4_1000'
      },
      calibrationLensModel: 'pinhole' as 'pinhole' | 'fisheye',
      calibrationImages: [] as CalibrationImage[],
      calibrationOwnPhotosOnly: true,
      calibrationSelected: {} as Record<string, boolean>,
      calibrationLoading: false,
      calibrationSolving: false,
      calibrationApplying: false,
      calibrationDeleting: false,
      calibrationSolveError: null as string | null,
      calibrationGuidedMode: false,
      calibrationGuidedBusy: false,
      calibrationGuidedResetToken: 0,
      calibrationGuidedCaptureToken: 0,
      calibrationGuidedAccumulateLive: false,
      calibrationIncludeOverlays: false,
      calibrationResult: null as CalibrationResult | null,
      calibrationImportSourcesLoading: false,
      calibrationImporting: false,
      calibrationImportError: null as string | null,
      calibrationImportSourceId: '',
      calibrationImportSources: [] as Array<{
        id: string;
        label: string;
        calibration: CalibrationParams;
      }>,
      calibrationPreviewOpen: false,
      calibrationPreviewItem: null as CalibrationImage | null,

      ipaLoading: false,
      ipaStatus: null as IpaStatus | null,
      ipaTarget: 'both' as 'both' | 'pisp' | 'vc4',
      ipaCt: 4000,
      ipaCcm: [
      [1, 0, 0],
      [0, 1, 0],
      [0, 0, 1]
    ] as number[][],
      ipaAdvanced: false,
      ipaChartModalOpen: false,
      ipaChartImage: '',
      ipaChartCorners: [] as Array<[number, number]>,
      ipaChartNaturalSize: { w: 1, h: 1 },
      ipaChartSolveBusy: false,
      ipaChartSolveError: null as string | null,
      ipaChartSolveResult: null as { ccm: number[][]; rmsError: number } | null
    });

    return state;
  }
</script>
