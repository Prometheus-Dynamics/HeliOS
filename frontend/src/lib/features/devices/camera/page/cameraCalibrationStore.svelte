<script lang="ts" module>
  export type CalibrationSolveResult = {
    calibration: { fx: number; fy: number; cx: number; cy: number; k1: number; k2: number; p1: number; p2: number; k3: number; undistortIters: number; lensModel?: 'pinhole' | 'fisheye' };
    reprojectionErrorPx?: number;
    viewsUsed?: number;
    pointsUsed?: number;
    warnings?: string[];
    debugViews?: Array<{
      image: string;
      overlay?: string | null;
      tagsDetected: number;
      pointsDetected: number;
      used: boolean;
      coverageRatio: number;
      rawTagsDetected?: number;
      rawIds?: number[];
      rawDuplicateIds?: number[];
      rawOutOfRangeIds?: number[];
    }>;
  };

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
      calibrationImages: [] as Array<{ name: string; size_bytes: number; content_type: string; stream_id?: string; kind?: string; captured_at_ms?: number }>,
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
      calibrationResult: null as CalibrationSolveResult | null,
      calibrationImportSourcesLoading: false,
      calibrationImporting: false,
      calibrationImportError: null as string | null,
      calibrationImportSourceId: '',
      calibrationImportSources: [] as Array<{
        id: string;
        label: string;
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
      }>,
      calibrationPreviewOpen: false,
      calibrationPreviewItem: null as any | null,

      ipaLoading: false,
      ipaStatus: null as { files: Array<{ target: string; path: string; exists: boolean; ccmCt?: number | null; ccm?: number[] | null }> } | null,
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
