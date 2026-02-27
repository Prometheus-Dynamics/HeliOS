import type { Interval, Resolution } from '$lib/ts-bindings/http/client';
import type { CameraStatus } from './devices';

// Legacy camera-config surface was removed from the OpenAPI. Keep the UI compiling by
// defining lightweight placeholders for the previous shapes.
export type CameraDriverVariant = {
  driver_namespace: string;
  driver_id?: string | null;
  driver_camera_id?: string | null;
  [key: string]: unknown;
};
export type CaptureDeviceStatus = Record<string, unknown>;
export type ControlInfo = Record<string, unknown>;
export type Format = { fourcc: unknown; [key: string]: unknown };
export type FormatInfo = Record<string, unknown>;
export type PipelineBinding = { pipeline_id: string; [key: string]: unknown };
export type StreamCalibration = Record<string, unknown>;

export type CameraPropertySummary = {
  label: string;
  value: string;
};

export type FormatSummary = {
  label: string;
  detail: string;
  modes: number;
};

export type IntervalOption = {
  id: string;
  label: string;
  fps: number | null;
  interval: Interval;
  isActive: boolean;
};

export type ResolutionOption = {
  id: string;
  label: string;
  width: number;
  height: number;
  intervals: IntervalOption[];
  resolution: Resolution;
  isActive: boolean;
};

export type CameraFormatOption = {
  id: string;
  label: string;
  description: string;
  fourcc: string;
  fourccInfo: Format['fourcc'];
  resolutions: ResolutionOption[];
  encoders: string[];
  decoders: string[];
  isActive: boolean;
};

export type RegisteredStreamConfig = {
  id: string;
  cameraUid: string;
  driverNamespace: string;
  driverId: string;
  label: string;
  driverCameraId: string;
  captureSessionAlias: string | null;
  hardwareId?: string;
  status: CameraStatus;
  framesPath: string | null;
  captureSessionId: string | null;
  manifest: Record<string, unknown> | null;
  pipelines: PipelineBinding[];
  clientCount: number;
  resolutionHint: string;
  supportedFormats: FormatSummary[];
  properties: CameraPropertySummary[];
  controls: ControlInfo[];
  calibration: StreamCalibration | null;
};

export type CameraConfigDetail = RegisteredStreamConfig & {
  formats: CameraFormatOption[];
};

export type CameraConfigResponse = {
  drivers: CameraConfigDetail[];
  activeDriverId: string | null;
  codecDisplayNames: Record<string, string>;
};

export type StreamDescriptor = CaptureDeviceStatus;
export type CameraDriver = CameraDriverVariant;
export type CameraFormats = FormatInfo[];
