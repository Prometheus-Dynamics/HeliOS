import type {
  BootloaderStatus,
  BootloaderUpdateRequest,
  BootloaderUpdateResponse,
  CaptureSnapshotRequest,
  DeviceOperationAckResponse,
  DeviceSnapshotResponse,
  DeviceSnapshotsResponse,
  FanConfig,
  FanCurvePoint,
  FanMode,
  FanStatus,
  LedConfig,
  NetworkInterfaceSettings,
  Nt4Settings,
  Nt4TopicInfo,
  Nt4TopicsRequest,
  Nt4TopicsResponse,
  Nt4ValueRequest,
  Nt4ValueResponse,
  OsReleaseInfo,
  RestartTargetId,
  UsbPowerSettings
} from '$lib/api/client';

export type Ipv4AssignmentView = {
  address: string;
  prefix: number;
  gateway?: string | null;
};

export type DeviceNetworkInterfaceView = {
  name: string;
  mode: 'dhcp' | 'static';
  mac?: string | null;
  static_ipv4?: Ipv4AssignmentView | null;
  dhcp_ipv4?: Ipv4AssignmentView | null;
};

export type DiagnosticsSettings = {
  keep: number;
  max_mb: number;
  tar: boolean;
};

export type DeviceSettingsData = {
  hostname: string;
  team_number?: number | null;
  interfaces: DeviceNetworkInterfaceView[];
  os_release?: OsReleaseInfo | null;
  nt4?: Nt4Settings | null;
  lighting?: LedConfig | null;
  usb_power?: UsbPowerSettings | null;
  fan?: FanConfig | null;
  fan_status?: FanStatus | null;
  diagnostics: DiagnosticsSettings;
  revision: string;
};

export type DeviceSettingsPatchRequest = {
  requested_by?: string;
  expected_revision?: string;
  hostname?: string;
  team_number?: number | null;
  interfaces?: Array<{
    name: string;
    assignment:
      | { mode: 'dhcp' }
      | { mode: 'static'; address: string; prefix: number; gateway?: string | null };
  }>;
  nt4?: Nt4Settings | null;
  lighting?: LedConfig | null;
  usb_power?: UsbPowerSettings | null;
  fan?: FanConfig | null;
  diagnostics?: DiagnosticsSettings;
  [key: string]: unknown;
};

export type InterfaceForm = {
  name: string;
  mode: 'dhcp' | 'static';
  address: string;
  prefix: number;
  gateway: string;
  leaseLabel: string | null;
  mac: string | null;
};

export type { BootloaderStatus, BootloaderUpdateRequest, BootloaderUpdateResponse, CaptureSnapshotRequest };
export type { DeviceOperationAckResponse, DeviceSnapshotResponse, DeviceSnapshotsResponse, RestartTargetId };
export type { FanConfig, FanCurvePoint, FanMode, FanStatus, LedConfig, NetworkInterfaceSettings, Nt4Settings };
export type { Nt4TopicInfo, Nt4TopicsRequest, Nt4TopicsResponse, Nt4ValueRequest, Nt4ValueResponse };
export type { OsReleaseInfo, UsbPowerSettings };

export type RestartTile = {
  id: RestartTargetId;
  label: string;
  detail: string;
  status: string;
  busy: boolean;
  error?: string | null;
};
