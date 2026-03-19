import type { DeviceOperationAckResponse, RestartTargetId } from '$lib/ts-bindings/http/client';

export type DeviceSettingsResponse = {
  hostname: string;
  team_number?: number | null;
  interfaces: DeviceNetworkInterfaceResponse[];
  os_release?: OsReleaseInfo | null;
  nt4?: Nt4SettingsResponse | null;
  lighting?: LightingSettings | null;
  usb_power?: UsbPowerSettings | null;
  fan?: FanSettings | null;
  fan_status?: FanStatus | null;
  diagnostics: DiagnosticsSettings;
  revision: string;
};

export type OsReleaseInfo = {
  version_id?: string | null;
  build_id?: string | null;
  pretty_name?: string | null;
  active_root?: string | null;
};

export type UpdateDeviceSettingsRequest = {
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
  nt4?: Nt4SettingsRequest | null;
  lighting?: LightingSettings | null;
  usb_power?: UsbPowerSettings | null;
  fan?: FanSettings | null;
  [key: string]: unknown;
};

export type Nt4SettingsResponse = {
  enabled: boolean;
  subscriptions_enabled?: boolean;
  emulate_limelight_api?: boolean;
  emulate_photonvision_api?: boolean;
  server_host?: string | null;
  server_port?: number | null;
  public_api_url?: string | null;
};

export type Nt4SettingsRequest = Nt4SettingsResponse;

export type Nt4TopicInfo = {
  name: string;
  data_type: string;
  properties?: unknown;
};

export type Nt4TopicsResponse = {
  host: string;
  port: number;
  prefix: string;
  topics: Nt4TopicInfo[];
};

export type Nt4ValueResponse = {
  host: string;
  port: number;
  topic: string;
  data_type?: string | null;
  value: unknown;
};

export type DeviceNetworkInterfaceResponse = {
  name: string;
  mode: 'dhcp' | 'static';
  mac?: string | null;
  static_ipv4?: StaticIpv4SettingsResponse | null;
  dhcp_ipv4?: DhcpIpv4LeaseResponse | null;
};

export type StaticIpv4SettingsResponse = {
  address: string;
  prefix: number;
  gateway?: string | null;
};

export type DhcpIpv4LeaseResponse = {
  address: string;
  prefix: number;
  gateway?: string | null;
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

export type DeviceSnapshotsResponse = {
  snapshots: DeviceSnapshotResponse[];
};

export type LightingSettings = {
  enabled: boolean;
  gpio: number;
  count: number;
  use_pwm: boolean;
  color_order: string;
  frequency_hz: number;
  brightness?: number | null;
  label?: string | null;
  protocol: string;
  default_animations?: Record<string, string> | null;
};

export type FanSettings = {
  enabled: boolean;
  pwm_path: string;
  min_percent: number;
  max_percent: number;
  manual_percent?: number | null;
  poll_interval_ms: number;
  invert_pwm?: boolean;
  curve: FanCurvePoint[];
  tacho_path?: string | null;
};

export type FanCurvePoint = {
  temp_c: number;
  percent: number;
};

export type FanStatus = {
  mode: 'disabled' | 'manual' | 'curve';
  target_percent: number;
  temperature_c?: number | null;
  rpm?: number | null;
  path_in_use?: string | null;
  last_error?: string | null;
  updated_at_ms?: number | null;
};

export type DiagnosticsSettings = {
  keep: number;
  max_mb: number;
  tar: boolean;
};

export type UsbPowerSettings = {
  enabled: boolean;
  usb_a_gpio?: number | null;
  usb_c_gpio?: number | null;
  usb_a_active_high: boolean;
  usb_c_active_high: boolean;
  usb_a_enabled: boolean;
  usb_c_enabled: boolean;
  tuning_enabled?: boolean;
  disable_autosuspend?: boolean;
  disable_usb2_lpm?: boolean;
  force_power_control_on?: boolean;
};

export type DeviceSnapshotResponse = {
  id: string;
  label: string;
  created_by: string;
  created_at: string;
  size_bytes: number;
  status: string;
};

export type BootloaderStatus = {
  supported: boolean;
  current_version?: string | null;
  required_version?: string | null;
  needs_update: boolean;
  update_available: boolean;
  update_file?: string | null;
  update_sig?: string | null;
  staged: boolean;
  status_message?: string | null;
};

export type BootloaderUpdateResponse = {
  staged: boolean;
  rebooting: boolean;
  message: string;
};

export type { DeviceOperationAckResponse, RestartTargetId };

export type RestartTile = {
  id: RestartTargetId;
  label: string;
  detail: string;
  status: string;
  busy: boolean;
  error?: string | null;
};
