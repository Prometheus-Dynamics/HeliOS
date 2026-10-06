// Wire types of helios-api v1 (backend/src/helios/api; docs/docs/api/http.md).
// Field names are the API's; adapt.ts maps them to the UI model.

export interface ApiErrorBody {
  error: { code: ApiErrorCode; message: string; needs?: string };
}

export type ApiErrorCode =
  | "bad_request"
  | "unauthorized"
  | "forbidden"
  | "not_found"
  | "conflict"
  | "unprocessable"
  | "payload_too_large"
  | "too_many_requests"
  | "not_available"
  | "backend_unavailable"
  | "internal";

/** `GET /v1/auth/status`. Open devices allow everything; secured ones need a session or a token. */
export interface AuthStatus {
  mode: "open" | "secured";
  authenticated: boolean;
  via: "open" | "session" | "token" | null;
  /** Sent as X-Helios-CSRF on mutations made with the session cookie. */
  csrf_token?: string;
  session_expires_at_ms?: number;
  password_set_at_ms?: number;
  tokens?: number;
  /** The device's auth file is unreadable; only `heliosctl auth reset` on the device fixes it. */
  problem?: string;
}

export interface ApiToken {
  id: string;
  label: string;
  /** The token's first characters, to recognise it. */
  prefix: string;
  created_at_ms: number;
  last_used_at_ms: number | null;
}

/** A new token: `token` is in this answer only. */
export interface NewApiToken extends ApiToken {
  token: string;
}

export interface Health {
  service: string;
  status: string;
  api_version: string;
  version: string;
}

export interface DeviceOs {
  name: string | null;
  version: string | null;
  pretty_name: string | null;
  build_id: string | null;
  kernel: string | null;
}

export interface ClockFacts {
  source: string;
  synchronized: boolean | null;
  offset_us: number | null;
  stratum: number | null;
  timebase: string | null;
}

export interface Device {
  node_id: string;
  hostname: string | null;
  model: string | null;
  serial: string | null;
  architecture: string;
  os: DeviceOs;
  uptime_s: number | null;
  api_version: string;
  api: string;
  orion: { reachable: boolean; desired_revision: number | null; observed_revision: number | null; peers_ready: number | null; peers_configured: number | null; message: string | null };
  clock: ClockFacts | null;
}

/** The Raze pd-device identity document plus the API's own section. */
export interface Identity {
  contract: number;
  model: string | null;
  rev: string | null;
  serial: string | null;
  hostname: string | null;
  os: { name: string | null; version: string | null };
  device_package: { version: string | null; commit: string | null };
  bootloader?: { version: string | null; timestamp?: number };
  update_methods: string[];
  update?: Record<string, unknown>;
  manage_url: string | null;
  macs: Record<string, string>;
  endpoints?: Record<string, string>;
  actions?: { id: string; label?: string; destructive?: boolean }[];
  camera_stream?: string;
  helios: { source: "pd-device" | "helios-api"; api_version: string; version: string; node_id: string; endpoints: Record<string, string> };
}

export interface Reading {
  id: string;
  label: string;
  value: number;
  unit?: string;
  warn_above?: number;
}

export interface Metrics {
  at_ms: number;
  cpu: number | null;
  cpu_cores: number[];
  load: [number, number, number] | null;
  memory_total_bytes: number | null;
  memory_available_bytes: number | null;
  temperature_c: number | null;
  throttled: number | null;
  disk: { path: string; total_bytes: number; used_bytes: number; available_bytes: number } | null;
  uptime_s: number | null;
  metrics: Reading[];
}

export interface UnitStatus {
  unit: string;
  active_state: string;
  sub_state: string;
  main_pid: number | null;
  memory_bytes: number | null;
  restarts: number | null;
}

export interface ProcessInfo {
  pid: number;
  name: string;
  unit: string | null;
  state: string;
  threads: number;
  rss_bytes: number;
  nice: number;
  cpu_time_ms: number;
  started_after_boot_ms: number;
  cpus_allowed: string | null;
}

export interface LogLine {
  at_ms: number;
  level: "error" | "warn" | "info" | "debug";
  unit: string;
  message: string;
  pid?: number;
}

export interface CameraMount {
  x: number;
  y: number;
  z: number;
  roll: number;
  pitch: number;
  yaw: number;
}

export interface CaptureLive {
  name: string;
  backend: string;
  mode: string;
  fps_configured: number | null;
  fps_measured: number | null;
  frames_delivered: number;
  drops: number;
  latency_p50_ms: number | null;
  latency_p95_ms: number | null;
  cpu_per_frame_us: number | null;
  exposure_us: number | null;
  analogue_gain: number | null;
  digital_gain: number | null;
  ae_state: string | null;
}

export interface Camera {
  id: string;
  name: string;
  node_id: string;
  provider: string;
  health: string;
  availability: string;
  backend: string | null;
  labels: Record<string, string>;
  frames_endpoint: string | null;
  used_by: string[];
  mount: CameraMount | null;
  live: { sources: { name: string; keys: string[]; in_use: boolean }[]; clients: number; frames_sent: number; frames_skipped: number; restarts: number; captures: CaptureLive[] } | null;
  live_error: string | null;
  settings_writable: boolean;
  preview_available: boolean;
}

/** Styx's standard camera controls, by the API's key (same units whatever the camera). */
export type CameraStandardControl = "exposure_us" | "gain" | "ae" | "ev" | "fps" | "awb" | "colour_temperature" | "red_gain" | "blue_gain" | "af_mode" | "af_trigger" | "lens_position";

/** A rectangle control value. */
export interface ControlRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type ControlValue = null | boolean | number | ControlRect | ControlRect[];

/** A camera control as the camera service lists it (`GET /v1/cameras/{id}/settings`). */
export interface CameraControl {
  id: number;
  name: string;
  kind: "bool" | "int" | "uint" | "float" | "menu" | "int_menu" | "rectangle" | "none" | "unknown";
  read_only: boolean;
  min: ControlValue;
  max: ControlValue;
  default: ControlValue;
  step: ControlValue | null;
  menu: string[] | null;
  /** The value now; null when it cannot be read. */
  current: ControlValue | null;
  standard: CameraStandardControl | null;
  /** The camera service lets the API change it. */
  writable: boolean;
}

export interface CameraSettings {
  writable: boolean;
  controls: CameraControl[];
  mode: string | null;
  fps: number | null;
  exposure_us: number | null;
  analogue_gain: number | null;
  digital_gain: number | null;
  ae_state: string | null;
  live_error: string | null;
}

/** `PATCH /v1/cameras/{id}/settings` body: standard keys, or a control's `name` or `id`. */
export type CameraControlChanges = Record<string, boolean | number | string>;

export interface AppliedCameraControl {
  control: string;
  id: number;
  requested: ControlValue;
  value: ControlValue;
  clamped: boolean;
  deferred: boolean;
  restarted: boolean;
  frame: number | null;
}

/** `camera` event data for a control change (by any client of the camera). */
export interface CameraControlEvent {
  id: string;
  change: "control";
  control: { id: number; standard: CameraStandardControl | null; value: ControlValue; frame: number | null; by: number | null; frame_rate_restart: boolean };
}

/** A Daedalus value in its tagged form. */
export type DaedalusValue =
  | { type: "Unit" }
  | { type: "Bool"; value: boolean }
  | { type: "Int"; value: number }
  | { type: "Float"; value: number }
  | { type: "String"; value: string }
  | { type: "List" | "Tuple"; value: DaedalusValue[] }
  | { type: string; value?: unknown };

export interface DaedalusNode {
  id: string;
  label?: string | null;
  bundle?: string | null;
  inputs: string[];
  outputs: string[];
  const_inputs?: [string, DaedalusValue][];
  metadata?: Record<string, DaedalusValue>;
  compute?: "CpuOnly" | "GpuPreferred" | "GpuRequired";
}

/** A versioned Daedalus graph document (`format: "daedalus.graph"`). */
export interface DaedalusGraphDocument {
  format: "daedalus.graph";
  schema_version: number;
  requires: { id: string; version?: string | null }[];
  metadata?: Record<string, unknown>;
  graph: {
    nodes: DaedalusNode[];
    edges: { from: { node: number; port: string }; to: { node: number; port: string }; metadata?: Record<string, DaedalusValue> }[];
    metadata?: Record<string, unknown>;
  };
}

export interface Binding {
  resource_id: string;
  camera?: string;
  output_width?: number;
  output_height?: number;
  pyramid?: number;
}

export interface PipelineSpec {
  name: string;
  graph: DaedalusGraphDocument;
  bindings: Record<string, Binding>;
  enabled?: boolean;
}

export interface PipelineOutput {
  pipeline: string;
  port: string;
  value: unknown;
  observed_at_ms: number;
}

export interface Pipeline {
  id: string;
  workload_id: string;
  name: string;
  node_id: string | null;
  revision: number | null;
  enabled: boolean;
  state: "pending" | "assigned" | "starting" | "running" | "stopped" | "completed" | "failed" | string;
  graph: DaedalusGraphDocument | null;
  graph_error: string | null;
  bindings: Record<string, Binding>;
  plugins: string[];
  session: { status: string; message: string | null; observed_at_ms: number } | null;
  telemetry: { fps?: number; last_tick_ms?: number; frames_processed?: number; frames_failed?: number; source_connected?: boolean; last_error?: string | null; [key: string]: unknown } | null;
  outputs: PipelineOutput[];
  managed: boolean;
}

export interface Revision {
  revision: number;
  saved_at_ms: number;
  name: string | null;
}

export interface Resource {
  id: string;
  type: string;
  name: string;
  kind: string | null;
  provider: string;
  health: string;
  availability: string;
  ownership: string;
  lease_state: string;
  leased_by: string[];
  workload: string | null;
  capabilities: string[];
  labels: Record<string, string>;
  endpoints: string[];
  action_result: { action_kind: string; status: string; data: unknown; error: string | null; observed_at_ms: number } | null;
  state: Record<string, unknown> | null;
}

export interface Upload {
  id: string;
  filename: string;
  size_bytes: number;
  sha256: string;
  image_url: string;
  uploaded_at_ms: number;
  version: string | null;
}

export interface ApplyResponse {
  update_id: string;
  artifact_id: string;
  version: string;
  sha256: string;
  image_url: string;
  message: string;
}

export interface UpdateExecution {
  update_id: string;
  artifact_id: string | null;
  version: string | null;
  artifact_class: string | null;
  phase: string;
  message: string | null;
}

export interface UpdateStatus {
  phase: string;
  stage: string;
  progress_percent: number | null;
  last_error: string | null;
  orion_reachable: boolean;
  updater_running: boolean;
  active: UpdateExecution | null;
  executions: UpdateExecution[];
  slots: { active: string | null; reserve: string | null; pending: string | null };
  boot_confirm: { request_id: string | null; status: string | null; selector: string | null };
  repartition: Record<string, string | null>;
}

export interface ApiEvent<T = unknown> {
  type: string;
  at_ms: number;
  data: T;
}
