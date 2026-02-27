export type CalibrationBoard = {
  squaresX: number;
  squaresY: number;
  squareMm: number;
  markerMm: number;
  marginMm: number;
  dpi: number;
  dictionary?: string;
};

export type CalibrationImage = {
  name: string;
  size_bytes: number;
  content_type: string;
  stream_id?: string;
  kind?: string;
  captured_at_ms?: number;
};

export type CalibrationResult = {
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

export type CalibrationParams = {
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

export type IpaStatus = {
  files: Array<{ target: string; path: string; exists: boolean; ccmCt?: number | null; ccm?: number[] | null }>;
};
