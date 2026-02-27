export type CameraPageTabId = 'stream' | 'media' | 'controls' | 'pipelines' | 'pose' | 'calibration';

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
};

export const PIPELINE_UI_METADATA_KEY = 'helios.pipeline.ui';
export const DEFAULT_LIBCAMERA_TARGET_FPS = 30;
