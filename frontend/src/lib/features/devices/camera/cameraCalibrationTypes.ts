import type {
  IpaStatus as GeneratedIpaStatus,
  SolveCalibrationResponse as GeneratedCalibrationResult,
  StreamCalibrationParams as GeneratedCalibrationParams
} from '$lib/api/client';

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

export type CalibrationResult = GeneratedCalibrationResult;
export type CalibrationParams = GeneratedCalibrationParams;

export type IpaStatus = GeneratedIpaStatus;
