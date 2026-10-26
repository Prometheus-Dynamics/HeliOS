// The typed helios-api v1 client. Every screen reads and acts through the
// stores; in live mode the stores call this client, in mock mode
// (`?mock=1`) they run the simulated robot in mock.ts instead.

import { API_BASE, MOCK } from "./mode";
import { mockDevice } from "./mock-device";
import type {
  ApiErrorBody,
  ApiErrorCode,
  ApiEvent,
  ApiToken,
  ApplyResponse,
  AuthStatus,
  Binding,
  Camera,
  CameraControlChanges,
  CameraMount,
  CameraSettings,
  CameraSettingsApplied,
  Device,
  Health,
  Identity,
  LogLine,
  Metrics,
  NewApiToken,
  Pipeline,
  PipelineTemplate,
  DeviceCatalog,
  Nt4Settings,
  Nt4State,
  CalibrationBoard,
  CalibrationCapture,
  CalibrationSolve,
  CameraCalibration,
  CameraCalibrations,
  CameraStatus,
  PipelinePlan,
  PipelineOutput,
  PipelineSpec,
  ProcessInfo,
  RawIo,
  Resource,
  Revision,
  UnitStatus,
  UpdateStatus,
  Upload,
  PlayerView,
  PlaybackCreate,
  PlaybackRun,
  FieldLayoutDocument,
  FieldLayoutList,
  SetupRecord,
  StoredIdentity,
  PeerList,
} from "./types";
import type { LedSettings, LedState, LedsResponse } from "./leds";
import type { EidosExport, PartSummary, RecorderSettings, RecordingListing, RecordingStatus } from "./recording-types";

export function errorText(error: unknown): string {
  if (error instanceof ApiError) return error.needs ? `${error.message} (needs: ${error.needs})` : error.message;
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "Something went wrong";
}

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: ApiErrorCode | "network",
    message: string,
    readonly needs?: string,
  ) {
    super(message);
    this.name = "ApiError";
  }

  /** The feature has no backend on this device yet (501). */
  get notAvailable(): boolean {
    return this.code === "not_available";
  }
}

export function isNotAvailable(error: unknown): error is ApiError {
  return error instanceof ApiError && error.notAvailable;
}

type Query = Record<string, string | number | boolean | undefined | null>;

function url(path: string, query?: Query): string {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query ?? {})) if (value !== undefined && value !== null && value !== "") params.set(key, String(value));
  const qs = params.toString();
  return `${API_BASE}${path}${qs ? `?${qs}` : ""}`;
}

const enc = encodeURIComponent;

/** The camera's live MJPEG preview (`GET /v1/cameras/{id}/preview`), for an `<img>`. */
/** The live outputs' WebSocket (`GET /v1/live/ws`): every pipeline output of every tick. */
export function liveOutputsUrl(): string {
  const address = new URL(url("/v1/live/ws"), typeof location === "undefined" ? "http://localhost/" : location.href);
  address.protocol = address.protocol === "https:" ? "wss:" : "ws:";
  return address.toString();
}

export function previewUrl(cameraId: string, nodeId?: string): string {
  return url(nodeId === undefined ? `/v1/cameras/${enc(cameraId)}/preview` : peerPath(nodeId, `/v1/cameras/${enc(cameraId)}/preview`));
}

/** A stored recording's MCAP download (`GET /v1/recordings/{id}`), for a link. */
export function recordingDownloadUrl(id: string): string {
  return url(`/v1/recordings/${enc(id)}`);
}

// Device security. On a secured device the browser signs in with the device
// password and gets an HttpOnly session cookie; mutations must also carry the
// session's CSRF token, which the auth store keeps here. A 401 means the
// session ended (or the device was just secured), and the auth store shows
// the sign-in screen.
let csrfToken: string | null = null;
let onUnauthorized: (() => void) | null = null;

export const session = {
  setCsrf(token: string | null | undefined) {
    csrfToken = token ?? null;
  },
  onUnauthorized(handler: (() => void) | null) {
    onUnauthorized = handler;
  },
};

async function request<T>(method: string, path: string, options: { query?: Query; body?: unknown; raw?: BodyInit; headers?: Record<string, string> } = {}): Promise<T> {
  // The simulated robot (`?mock=1`) has no device. A call that reaches the API directly (the LEDs,
  // a camera's calibrations) is "not available" there, without a request to a dev server that has no
  // device behind it (each one a failed-request error in the console).
  if (MOCK) {
    const answer = await mockDevice(method, path, options);
    if (answer) return answer.value as T;
    throw new ApiError(501, "not_available", `${method} ${path} is not on the simulated robot`);
  }
  const headers: Record<string, string> = { Accept: "application/json", ...(options.headers ?? {}) };
  if (csrfToken && method !== "GET" && method !== "HEAD") headers["X-Helios-CSRF"] = csrfToken;
  let body: BodyInit | undefined = options.raw;
  if (options.body !== undefined) {
    headers["Content-Type"] = "application/json";
    body = JSON.stringify(options.body);
  }
  let response: Response;
  try {
    response = await fetch(url(path, options.query), { method, headers, body, credentials: "same-origin" });
  } catch (error) {
    throw new ApiError(0, "network", `Cannot reach the device: ${(error as Error).message}`);
  }
  if (response.status === 204) return undefined as T;
  const text = await response.text();
  const json = text ? safeJson(text) : undefined;
  if (!response.ok) {
    const err = (json as ApiErrorBody | undefined)?.error;
    if (response.status === 401 && !path.startsWith("/v1/auth/")) onUnauthorized?.();
    throw new ApiError(response.status, err?.code ?? "internal", err?.message ?? `${method} ${path} failed with HTTP ${response.status}`, err?.needs);
  }
  return json as T;
}

function safeJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}

/** Subscribe to Server-Sent Events. Returns an unsubscribe function. */
function subscribe(path: string, query: Query | undefined, handlers: { onEvent: (event: ApiEvent) => void; onOpen?: () => void; onError?: () => void }, types: string[]): () => void {
  if (typeof EventSource === "undefined") return () => {};
  const source = new EventSource(url(path, query));
  const listener = (message: MessageEvent) => {
    const parsed = safeJson(message.data) as ApiEvent | undefined;
    if (parsed) handlers.onEvent(parsed);
  };
  for (const type of types) source.addEventListener(type, listener as EventListener);
  source.onopen = () => handlers.onOpen?.();
  source.onerror = () => handlers.onError?.();
  return () => source.close();
}

export const EVENT_TYPES = ["hello", "output", "peripheral", "nt4", "settings", "pipeline", "resource", "update", "camera", "metrics", "orion", "gpio", "recording", "recording_settings", "lagged"];

/** The request options a call takes (the body, query, raw body and headers). */
type CallOptions = Parameters<typeof request>[2];

/**
 * The device's API calls, each at `at(path)`: the device's own `/v1` for `api`, or one peer's
 * through the gateway for `scoped(nodeId)`. The local calls are the same paths as before.
 */
function methods(at: (path: string) => string) {
  const call = <T>(method: string, path: string, options?: CallOptions) => request<T>(method, at(path), options);
  return {
  // Device security
  authStatus: () => call<AuthStatus>("GET", "/v1/auth/status"),
  login: (password: string) => call<AuthStatus>("POST", "/v1/auth/login", { body: { password } }),
  logout: () => call<void>("POST", "/v1/auth/logout"),
  enableAuth: (password: string) => call<AuthStatus>("POST", "/v1/auth/enable", { body: { password } }),
  disableAuth: (password?: string) => call<AuthStatus>("POST", "/v1/auth/disable", { body: password === undefined ? {} : { password } }),
  changePassword: (current_password: string, new_password: string) => call<AuthStatus>("POST", "/v1/auth/password", { body: { current_password, new_password } }),
  tokens: () => call<ApiToken[]>("GET", "/v1/auth/tokens"),
  createToken: (label: string) => call<NewApiToken>("POST", "/v1/auth/tokens", { body: { label } }),
  revokeToken: (id: string) => call<void>("DELETE", `/v1/auth/tokens/${enc(id)}`),

  // Device and system
  health: () => call<Health>("GET", "/v1/health"),
  identity: () => call<Identity>("GET", "/v1/identity"),
  device: () => call<Device>("GET", "/v1/device"),
  metrics: () => call<Metrics>("GET", "/v1/metrics"),
  services: () => call<UnitStatus[]>("GET", "/v1/system/services"),
  restartService: (unit: string) => call<UnitStatus>("POST", `/v1/system/services/${enc(unit)}/restart`),
  reboot: () => call<{ accepted: boolean; message: string }>("POST", "/v1/system/reboot"),
  safeMode: () => call<never>("POST", "/v1/system/safe-mode"),
  processes: () => call<ProcessInfo[]>("GET", "/v1/system/processes"),
  signal: (pid: number, signal: "TERM" | "KILL" | "STOP" | "CONT") => call<{ accepted: boolean; message: string }>("POST", `/v1/system/processes/${pid}/signal`, { body: { signal } }),
  setAffinity: (pid: number, core: number | null) => call<never>("PUT", `/v1/system/processes/${pid}/affinity`, { body: { core } }),
  setNice: (pid: number, nice: number) => call<never>("PUT", `/v1/system/processes/${pid}/nice`, { body: { nice } }),

  // Logs and events
  logs: (query: { unit?: string; lines?: number; since_ms?: number; level?: "error" | "warn" | "info" | "debug" } = {}) => call<{ lines: LogLine[] }>("GET", "/v1/logs", { query }),
  followLogs: (query: { unit?: string; level?: string }, onLine: (line: LogLine) => void) => subscribe(at("/v1/logs/stream"), query, { onEvent: (e) => onLine(e as unknown as LogLine) }, ["log"]),
  events: (handlers: { onEvent: (event: ApiEvent) => void; onOpen?: () => void; onError?: () => void }, types?: string[]) =>
    subscribe(at("/v1/events"), types ? { types: types.join(",") } : undefined, handlers, types ?? EVENT_TYPES),

  // Cameras
  cameras: () => call<Camera[]>("GET", "/v1/cameras"),
  camera: (id: string) => call<Camera>("GET", `/v1/cameras/${enc(id)}`),
  cameraSettings: (id: string) => call<CameraSettings>("GET", `/v1/cameras/${enc(id)}/settings`),
  setCameraSettings: (id: string, changes: CameraControlChanges) => call<CameraSettingsApplied>("PATCH", `/v1/cameras/${enc(id)}/settings`, { body: changes }),
  /** Every writable control back to its default; the values kept across reboots are forgotten. */
  resetCameraSettings: (id: string) => call<CameraSettingsApplied>("DELETE", `/v1/cameras/${enc(id)}/settings`),
  setMount: (id: string, mount: CameraMount) => call<CameraMount>("PUT", `/v1/cameras/${enc(id)}/mount`, { body: mount }),
  clearMount: (id: string) => call<void>("DELETE", `/v1/cameras/${enc(id)}/mount`),
  /** The camera's status chain alone (each camera in `cameras()` has it too). */
  cameraStatus: (id: string) => call<CameraStatus>("GET", `/v1/cameras/${enc(id)}/status`),
  calibrations: (id: string) => call<CameraCalibrations>("GET", `/v1/cameras/${enc(id)}/calibration`),
  putCalibration: (id: string, calibration: CameraCalibration) => call<CameraCalibrations>("PUT", `/v1/cameras/${enc(id)}/calibration`, { body: calibration }),
  captureCalibration: (id: string, body: { board: CalibrationBoard; width?: number; height?: number; keep?: boolean }) => call<CalibrationCapture>("POST", `/v1/cameras/${enc(id)}/calibration/capture`, { body }),
  solveCalibration: (id: string, body: { model?: "pinhole" | "fisheye"; drop?: number[] }) => call<CalibrationSolve>("POST", `/v1/cameras/${enc(id)}/calibration/solve`, { body }),
  resetCalibration: (id: string) => call<void>("DELETE", `/v1/cameras/${enc(id)}/calibration/views`),

  // Pipelines
  pipelines: () => call<Pipeline[]>("GET", "/v1/pipelines"),
  pipelineTemplates: () => call<PipelineTemplate[]>("GET", "/v1/pipelines/templates"),
  pipeline: (id: string) => call<Pipeline>("GET", `/v1/pipelines/${enc(id)}`),
  /** The pipeline's Daedalus plan: its nodes, the edges and any device transfers (`explain_plan`). */
  pipelinePlan: (id: string) => call<PipelinePlan>("GET", `/v1/pipelines/${enc(id)}/plan`),
  createPipeline: (spec: PipelineSpec & { id?: string }) => call<Pipeline>("POST", "/v1/pipelines", { body: spec }),
  putPipeline: (id: string, spec: PipelineSpec) => call<Pipeline>("PUT", `/v1/pipelines/${enc(id)}`, { body: spec }),
  deletePipeline: (id: string) => call<void>("DELETE", `/v1/pipelines/${enc(id)}`),
  startPipeline: (id: string) => call<Pipeline>("POST", `/v1/pipelines/${enc(id)}/start`),
  stopPipeline: (id: string) => call<Pipeline>("POST", `/v1/pipelines/${enc(id)}/stop`),
  restartPipeline: (id: string) => call<Pipeline>("POST", `/v1/pipelines/${enc(id)}/restart`),
  rollbackPipeline: (id: string) => call<Pipeline>("POST", `/v1/pipelines/${enc(id)}/rollback`),
  revisions: (id: string) => call<Revision[]>("GET", `/v1/pipelines/${enc(id)}/revisions`),
  bindInput: (id: string, input: string, binding: Binding) => call<Pipeline>("PUT", `/v1/pipelines/${enc(id)}/bindings/${enc(input)}`, { body: binding }),
  outputs: () => call<PipelineOutput[]>("GET", "/v1/outputs"),
  plugins: () => call<{ engine_running: boolean; plugins: string[] }>("GET", "/v1/plugins"),
  catalog: () => call<DeviceCatalog>("GET", "/v1/catalog"),
  nt4: () => call<Nt4State>("GET", "/v1/device/nt4"),
  setNt4: (settings: Nt4Settings) => call<Nt4State>("PUT", "/v1/device/nt4", { body: settings }),
  /** The camera and object identities (names, icons, colours) the device keeps, by object id. */
  identities: () => call<Record<string, StoredIdentity>>("GET", "/v1/device/identities"),
  putIdentity: (id: string, identity: StoredIdentity) => call<Record<string, StoredIdentity>>("PUT", `/v1/device/identities/${enc(id)}`, { body: identity }),
  deleteIdentity: (id: string) => call<void>("DELETE", `/v1/device/identities/${enc(id)}`),
  setup: () => call<SetupRecord>("GET", "/v1/device/setup"),
  setSetup: (record: SetupRecord) => call<SetupRecord>("PUT", "/v1/device/setup", { body: record }),
  fieldLayouts: () => call<FieldLayoutList>("GET", "/v1/field-layouts"),
  fieldLayout: (id: string) => call<FieldLayoutDocument>("GET", `/v1/field-layouts/${enc(id)}`),
  /** Keep a WPILib AprilTag field layout (JSON text) as `id`, replacing an uploaded layout of that id. */
  uploadFieldLayout: (id: string, name: string, wpilibJson: string, tagSideM: number) =>
    call<{ id: string }>("POST", "/v1/field-layouts", { query: { id, name, tag_side_m: tagSideM }, raw: wpilibJson, headers: { "Content-Type": "application/json" } }),
  selectFieldLayout: (id: string) => call<{ id: string }>("PUT", "/v1/field-layouts/selected", { body: { id } }),

  // Playback: recordings and image folders replayed as cameras. Transport is the player's camera settings
  // (the recordings it lists come from `recordings()` below).
  playbackPlayers: () => call<PlayerView[]>("GET", "/v1/playback"),
  openPlayback: (body: PlaybackCreate) => call<PlayerView>("POST", "/v1/playback", { body }),
  closePlayback: (id: string) => call<void>("DELETE", `/v1/playback/${enc(id)}`),
  runOnPlayback: (id: string, pipeline: string) => call<PlaybackRun>("POST", `/v1/playback/${enc(id)}/runs`, { body: { pipeline } }),

  // The LED ring (settings, locate, and previewing a state)
  leds: () => call<LedsResponse>("GET", "/v1/device/leds"),
  setLeds: (settings: LedSettings) => call<LedsResponse>("PUT", "/v1/device/leds", { body: settings }),
  locateLeds: (seconds = 10) => call<LedsResponse>("POST", "/v1/device/leds/locate", { body: { seconds } }),
  /** Shows `state` for `seconds`; `null` ends the preview. */
  previewLeds: (state: LedState | null, seconds?: number) => call<LedsResponse>("POST", "/v1/device/leds/preview", { body: seconds === undefined ? { state } : { state, seconds } }),

  // Session recordings (helios-recorder): record a camera's frames, keep and export them
  recordings: () => call<RecordingListing>("GET", "/v1/recordings"),
  startRecording: (camera: string) => call<RecordingStatus>("POST", "/v1/recordings", { body: { camera } }),
  stopRecording: (id: string) => call<RecordingStatus>("POST", `/v1/recordings/${enc(id)}/stop`),
  deleteRecording: (id: string) => call<void>("DELETE", `/v1/recordings/${enc(id)}`),
  /** The last seconds of a camera's ring, written as a new stored recording. */
  captureLast: (body: { seconds?: number; camera?: string }) => call<PartSummary>("POST", "/v1/recordings/capture-last", { body }),
  exportEidos: (id: string) => call<EidosExport>("POST", `/v1/recordings/${enc(id)}/export/eidos`),
  recordingSettings: () => call<RecorderSettings>("GET", "/v1/recordings/settings"),
  setRecordingSettings: (settings: RecorderSettings) => call<RecorderSettings>("PUT", "/v1/recordings/settings", { body: settings }),

  // Resources and peripherals
  resources:(type?: string) => call<Resource[]>("GET", "/v1/resources", { query: { type } }),
  peripherals: () => call<Resource[]>("GET", "/v1/peripherals"),
  peripheralAction: (id: string, kind: string, arg: Record<string, unknown> = {}) =>
    call<{ action_id: string; resource: string; done: boolean; result: Resource["action_result"] }>("POST", `/v1/peripherals/${enc(id)}/actions`, { body: { kind, arg } }),
  /** Raw GPIO/PWM/I2C/SPI through lemnosd: the live claims, and a raw action (`gpio.claim`, ...). */
  rawIo: () => call<RawIo>("GET", "/v1/peripherals/io"),
  rawIoAction: (kind: string, arg: Record<string, unknown> = {}) =>
    call<{ action_id: string; resource: string; done: boolean; result: Resource["action_result"] }>("POST", "/v1/peripherals/io/actions", { body: { kind, arg } }),

  // Updates
  uploadImage: (file: Blob, options: { filename?: string; sha256?: string; version?: string } = {}) =>
    call<Upload>("POST", "/v1/update/uploads", { query: { filename: options.filename ?? (file instanceof File ? file.name : undefined), sha256: options.sha256, version: options.version }, raw: file, headers: { "Content-Type": "application/octet-stream" } }),
  uploads: () => call<Upload[]>("GET", "/v1/update/uploads"),
  deleteUpload: (id: string) => call<void>("DELETE", `/v1/update/uploads/${enc(id)}`),
  applyUpdate: (body: { upload_id?: string; image_url?: string; version?: string; sha256?: string; reboot?: boolean }) => call<ApplyResponse>("POST", "/v1/update/apply", { body }),
  updateStatus: () => call<UpdateStatus>("GET", "/v1/update/status"),
  switchSlot: () => call<never>("POST", "/v1/update/slots/switch"),
  };
}

/** A peer's path through the gateway: the device's own `/v1` path, under `/v1/peers/{node_id}`. */
export function peerPath(nodeId: string, path: string): string {
  return `/v1/peers/${enc(nodeId)}${path.replace(/^\/v1/, "")}`;
}

export const api = {
  ...methods((path) => path),
  /** The HeliOS devices on this robot (`GET /v1/peers`): the cluster's name and each peer's state. */
  peers: () => request<PeerList>("GET", "/v1/peers"),
};

export type HeliosApi = typeof api;

const scopes = new Map<string, HeliosApi>();

/**
 * The same calls as `api`, for one peer of the robot: each path goes to that peer's own API through
 * the gateway. A peer that is not routed (listed, not enrolled) gets no request from the UI.
 */
export function scoped(nodeId: string): HeliosApi {
  let scope = scopes.get(nodeId);
  if (!scope) {
    scope = { ...methods((path) => peerPath(nodeId, path)), peers: api.peers };
    scopes.set(nodeId, scope);
  }
  return scope;
}
