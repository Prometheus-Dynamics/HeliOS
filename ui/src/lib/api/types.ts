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
  /** The device's auth file is unreadable; only `helios-api auth reset` on the device fixes it. */
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

/** The Raze board identity document plus the API's own section. */
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
  helios: { source: "board" | "helios-api"; api_version: string; version: string; node_id: string; endpoints: Record<string, string> };
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
  /** The API's control client is connected to the camera service; null when it keeps none. */
  service_online: boolean | null;
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
  /** The labels of an enumerated control's values, by index (the value is the index). */
  menu: string[] | null;
  /** The range it takes now, when narrower than min..max (exposure at the frame duration, the gain AE may use). */
  effective_min?: ControlValue | null;
  effective_max?: ControlValue | null;
  /** The value now; null when it cannot be read. */
  current: ControlValue | null;
  standard: CameraStandardControl | null;
  /** The camera service lets the API change it. */
  writable: boolean;
  /** A value set through the API is stored for it and re-applied after a reboot. */
  persisted: boolean;
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
  /** The values set through the API, kept across reboots (standard keys or control names). */
  persisted: Record<string, ControlValue>;
}

/** `PATCH`/`DELETE /v1/cameras/{id}/settings` answer: what was applied, and the values kept now. */
export interface CameraSettingsApplied {
  applied: AppliedCameraControl[];
  persisted: Record<string, ControlValue>;
}

/**
 * `PATCH /v1/cameras/{id}/settings` body: standard keys, or a control's `name` or `id`. A `null`
 * value sets that control back to its default and forgets the value kept for it.
 */
export type CameraControlChanges = Record<string, boolean | number | string | null | { x: number; y: number; width: number; height: number }>;

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
  /** Eidos detector groups: `tracked` (full search every `full_search_every` frames, default 8) or `full`. */
  search_mode?: SearchMode;
  full_search_every?: number;
  /** Tracked only: misses after which a track's recovery escalates to a full search (default 0, never). */
  loss_full_search_after?: number;
  /** Tracked only: misses over which a track's search window keeps growing (default 6). */
  margin_growth_misses?: number;
  /** The field layout (`/v1/field-layouts`) of the graph's multi-tag pose; unset follows the selected one. */
  field_layout?: string;
  /** The tag family the graph's Eidos detector groups decode (`apriltag_16h5`, ...); unset keeps the graph's. */
  dictionary?: string;
  /** The CPU core the graph thread is pinned to (0 to 63); unset leaves it to the scheduler. */
  cpu?: number;
}

/** `GET /v1/pipelines/templates`: a graph helios-engine ships, deployable with a camera on `camera_input`. */
export interface PipelineTemplate {
  id: string;
  /** A detector family, or `features` (a non-detector template, such as the feature tracks). */
  family: "apriltag" | "aruco" | "features";
  /** `default` (one node, starts on a sliced frame's first rows) or `staged` (the node-group, one node per stage, for profiling). */
  variant?: "default" | "staged";
  dictionary: string | null;
  camera_input: string;
  /** Half-size pyramid levels its camera binding asks for. */
  pyramid?: number;
  graph: DaedalusGraphDocument;
}

export type SearchMode = "full" | "tracked";

/** A rigid transform as Eidos's values show it (`a_from_b`). */
export interface Pose3 {
  translation: { x: number; y: number; z: number };
  rotation: { w: number; x: number; y: number; z: number };
}

/** A pipeline's tag and field poses (from its `pose_solutions` and `multi_tag_pose` outputs). */
export interface PoseSummary {
  /** `calibrated`; `uncalibrated` (no calibration for the camera at the pipeline's frame size) or `image_size_mismatch`: no poses. */
  status: "calibrated" | "uncalibrated" | "image_size_mismatch" | null;
  /** Each tag's best pose in the camera's optical frame (x right, y down, z forward). */
  tags: { id: number; translation: Pose3["translation"] | null; rotation: Pose3["rotation"] | null; error_px: number | null; ambiguity: number | null }[];
  field_valid: boolean | null;
  field_rms_px?: number | null;
  field_inlier_tags?: number | null;
  field_ambiguity?: number | null;
  /** The camera's optical frame in the field (WPILib blue-origin frame); `null` without a valid pose. */
  camera_in_field: Pose3 | null;
  /** WPILib's robot frame in the field, through the camera's mount; `null` without a mount or a valid pose. */
  robot_in_field: Pose3 | null;
  observed_at_ms: number | null;
}

/** A camera calibration (`GET`/`PUT /v1/cameras/{id}/calibration`). */
export interface CameraCalibration {
  width: number;
  height: number;
  model: "pinhole" | "fisheye";
  fx: number;
  fy: number;
  cx: number;
  cy: number;
  distortion?: Partial<Record<"k1" | "k2" | "k3" | "k4" | "k5" | "k6" | "p1" | "p2", number>>;
  rms_px?: number;
  source?: string;
  /** The sensor orientation it was taken in (unflipped when absent); the API turns it into the camera's current one. */
  orientation?: { hflip?: boolean; vflip?: boolean };
  saved_at_ms?: number;
}

export interface CameraCalibrations {
  calibrations: CameraCalibration[];
  pipelines: { pipeline: string; input: string; calibration: { status: "calibrated" | "uncalibrated"; width?: number; height?: number; model?: string; scaled_from?: [number, number]; reason?: string } }[];
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
  telemetry: { fps?: number; last_tick_ms?: number; frames_processed?: number; frames_failed?: number; source_connected?: boolean; last_error?: string | null; cpu?: number | null; cpu_error?: string | null; [key: string]: unknown } | null;
  /** Daedalus per-node timings over the last window and since start (null while metrics are off). */
  metrics?: { window?: PipelineMetricsWindow; total?: PipelineMetricsWindow; [key: string]: unknown } | null;
  outputs: PipelineOutput[];
  search_mode: SearchMode | null;
  full_search_every: number | null;
  loss_full_search_after: number | null;
  margin_growth_misses: number | null;
  /** The field layout the multi-tag pose solves against; null without one. */
  field_layout: string | null;
  /** The CPU core the graph thread is pinned to (the spec's `cpu`); null for the scheduler's. */
  cpu: number | null;
  pose: PoseSummary | null;
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

/** A raw GPIO line or PWM channel claim through lemnosd (`GET /v1/peripherals/io`). */
export interface RawClaim {
  id: string;
  kind: "gpio" | "pwm";
  target: string;
  expires_at_ms: number;
  held: boolean;
  direction: "input" | "output" | null;
  value: boolean | null;
  edges: number | null;
  last_edge: { rising: boolean; timestamp_ns: number; seq: number } | null;
  period_ns: number | null;
  duty_ns: number | null;
  enabled: boolean | null;
}

export interface RawIo {
  resource: string;
  available: boolean;
  claims: RawClaim[];
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
  version: string;
  sha256: string;
  image_url: string;
  reboot: boolean;
  message: string;
}

/** What helios-api last asked the board update writer (A/B) to do. */
export interface UpdateTask {
  update_id: string;
  upload_id: string;
  version: string;
  sha256: string;
  /** staging, staged, applying (rebooting into the new slot on trial), failed */
  step: string;
  reboot: boolean;
  error: string | null;
  started_at_ms: number;
}

export interface UpdateStatus {
  /** The board update writer's state: idle, staging, staged, trying, confirmed, rolled-back, error, unknown. */
  phase: string;
  stage: string;
  progress_percent: number | null;
  last_error: string | null;
  updater_available: boolean;
  slots: {
    active: string | null;
    staged: string | null;
    /** `slots` on a dynamic-slots board, `ab` otherwise. */
    layout?: string;
    default?: string | null;
    previous?: string | null;
    trial?: string | null;
    free_bytes?: number | null;
    reserve_bytes?: number | null;
    installed?: { id: string; family?: string; version?: string; variant?: string; size?: number; flags?: string[]; good?: string }[];
  };
  version_active: string | null;
  version_staged: string | null;
  task: UpdateTask | null;
}

export interface ApiEvent<T = unknown> {
  type: string;
  at_ms: number;
  data: T;
}

export interface PipelineMetricsNode {
  index: number;
  label: string;
  id: string;
  calls: number;
  handler_us_mean: number;
  total_us_mean: number;
  handler_us_per_tick: number;
  /** Wall time per tick (recorded at every metrics level; handler time needs `detailed`). */
  total_us_per_tick?: number;
}

export interface PipelineMetricsWindow {
  ticks?: number;
  nodes?: PipelineMetricsNode[];
  [key: string]: unknown;
}

/** `GET /v1/catalog`: every node helios-engine's loaded plugins declare. */
export interface DeviceCatalog {
  format: "helios.node-catalog";
  schema_version: number;
  plugins: { id: string; version?: string | null }[];
  nodes: DeviceCatalogNode[];
}

export interface DeviceCatalogNode {
  id: string;
  plugin?: string;
  label?: string;
  summary?: string;
  /** A node-group (its stages are nodes of their own). */
  group?: boolean;
  /** The plugin's category (`fiducials`, `filter`, `mask`, ...). */
  category?: string;
  /** Node-groups this node is a stage of: listed under them, not on its own. */
  stage_of?: string[];
  inputs: DeviceCatalogPort[];
  outputs: DeviceCatalogPort[];
}

export interface DeviceCatalogPort {
  name: string;
  type_key: string;
  schema?: unknown;
  optional?: boolean;
  /** Set on constant inputs (parameters): their default. */
  default?: number | string | boolean | null;
  options?: (string | number)[];
  min?: number;
  max?: number;
  step?: number;
  description?: string;
}

/** The printed ChArUco board (`POST /v1/cameras/{id}/calibration/capture`). */
export interface CalibrationBoard {
  squares_x: number;
  squares_y: number;
  square_mm: number;
  marker_mm: number;
  dictionary: string;
}

export interface CalibrationCapture {
  /** Board corners found, pixels. */
  corners: [number, number][];
  width: number;
  height: number;
  kept: boolean;
  views: number;
  message?: string;
}

export interface CalibrationSolve {
  /** Not stored yet: `PUT /v1/cameras/{id}/calibration` stores it. */
  calibration: CameraCalibration;
  view_rms: number[];
  views: number;
  converged: boolean;
}

/** `GET`/`PUT /v1/device/nt4`: pipeline outputs on NetworkTables for robot code. */
export interface Nt4Settings {
  enabled: boolean;
  team: number | null;
  server: string | null;
  port: number;
  prefix: string;
}

export interface Nt4State {
  settings: Nt4Settings;
  /** Where publishing goes (`null`: off, or no team number or server). */
  server: string | null;
  connected: boolean;
  engine_reporting: boolean;
}

/** Playback (`/v1/playback`): a recording or an image folder, replayed as a camera. */
export type PlaybackSource = { recording: string } | { path: string };

/** `POST /v1/playback`: a source and how to start it (`fps` is for image folders). */
export interface PlaybackCreate {
  source: PlaybackSource;
  loop?: boolean;
  speed?: number;
  start_paused?: boolean;
  fps?: number;
}

/** A player's replay state from its camera service's replay controls. */
export interface PlaybackStatus {
  state: "playing" | "paused" | "ended" | "unknown" | string;
  position_ms: number;
  /** 0-based frame index, as the API counts frames. */
  frame: number;
  duration_ms: number;
  frame_count: number;
  speed: number | null;
  loop: boolean;
}

export interface PlayerView {
  id: string;
  /** The player's camera resource: its frames, controls and pipeline bindings. */
  resource_id: string;
  name: string;
  source: PlaybackSource;
  kind: "recording" | "images";
  /** A raw Bayer recording, processed by the software ISP and offered as NV12. */
  raw: boolean;
  note: string | null;
  frames_endpoint: string;
  speed: number;
  loop: boolean;
  fps: number | null;
  created_at_ms: number;
  status: PlaybackStatus | null;
  status_error: string | null;
}

/** Where a run's calibration or mount came from. */
export type RunContextFrom = { from: "recording" } | { from: "camera"; camera: string } | { from: "none" };

/** `POST /v1/playback/{id}/runs`: the copy as a pipeline, and what its calibration and mount came from. */
export interface PlaybackRun {
  pipeline: Pipeline;
  context: { calibration: RunContextFrom; mount: RunContextFrom };
}

// Recordings (`GET /v1/recordings`) are typed in recording-types.ts; playback lists them too.
export type { RecordingListing, StoredRecording } from "./recording-types";
