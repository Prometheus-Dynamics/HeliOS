import type { StreamPreviewFormat } from '$lib/api/streamPreviewFormat';
import type { DeviceIdentity, DeviceMetrics, SensorPeripheralFirmwareStatus } from '$lib/ts-bindings/http/client';

export type CameraStatus = 'live' | 'degraded' | 'idle' | 'offline';

export type SummaryTile = {
  label: string;
  value: string;
  detail?: string;
};

export type CameraCard = {
  id: string;
  cameraUid: string;
  captureSessionId: string | null;
  captureSessionAlias: string | null;
  previewFormat?: StreamPreviewFormat | null;
  hardwareId?: string | null;
  name: string;
  driverNamespace: string;
  driverId: string | null;
  driverCameraId: string | null;
  status: CameraStatus;
  recordingActive?: boolean;
  recordingSinceMs?: number | null;
  resolution: string;
  pipeline: string | null;
  bandwidth: string;
  lastSeen: string;
};

export type SensorOrientation = {
  roll: number;
  pitch: number;
  yaw: number;
  quaternion?: SensorQuaternion | null;
};

export type SensorQuaternion = {
  w: number;
  x: number;
  y: number;
  z: number;
};

export type PeripheralIcon = {
  url: string;
  label?: string | null;
};

export type FirmwareAlertInfo = {
  severity: 'error' | 'warning';
  title: string;
  description: string;
  chipLabel: string;
};

export type PeripheralEntry = {
  id?: string;
  name: string;
  driverNamespace: string;
  driverCameraId: string;
  identity?: DeviceIdentity | null;
  status: string;
  interval: string;
  type: string | null;
  orientation?: SensorOrientation | null;
  lastCalibration?: string | null;
  hardwareId?: string | null;
  firmware?: SensorPeripheralFirmwareStatus | null;
  telemetry?: unknown | null;
  icon?: PeripheralIcon | null;
  firmwareAlert?: FirmwareAlertInfo | null;
  warnings?: Array<{ code: string; message: string }> | null;
};

export type TaskEntry = {
  title: string;
  action: string;
  btn: string;
};

export type DevicesPageErrors = {
  peripherals?: string | null;
};

export type DevicesPageData = {
  summary: SummaryTile[];
  cameras: CameraCard[];
  peripherals: PeripheralEntry[];
  tasks: TaskEntry[];
  errors?: DevicesPageErrors;
};
export type DeviceHealth = DeviceMetrics | null;
