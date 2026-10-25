// The typed helios-api v1 client. Every screen reads and acts through the
// stores; in live mode the stores call this client, in mock mode
// (`?mock=1`) they run the simulated robot in mock.ts instead.

import { API_BASE, MOCK } from "./mode";
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

export function previewUrl(cameraId: string): string {
  return url(`/v1/cameras/${enc(cameraId)}/preview`);
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
  if (MOCK) throw new ApiError(501, "not_available", `${method} ${path} is not on the simulated robot`);
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

export const EVENT_TYPES = ["hello", "output", "peripheral", "nt4", "pipeline", "resource", "update", "camera", "metrics", "orion", "gpio", "recording", "recording_settings", "lagged"];

export const api = {
  // Device security
  authStatus: () => request<AuthStatus>("GET", "/v1/auth/status"),
  login: (password: string) => request<AuthStatus>("POST", "/v1/auth/login", { body: { password } }),
  logout: () => request<void>("POST", "/v1/auth/logout"),
  enableAuth: (password: string) => request<AuthStatus>("POST", "/v1/auth/enable", { body: { password } }),
  disableAuth: (password?: string) => request<AuthStatus>("POST", "/v1/auth/disable", { body: password === undefined ? {} : { password } }),
  changePassword: (current_password: string, new_password: string) => request<AuthStatus>("POST", "/v1/auth/password", { body: { current_password, new_password } }),
  tokens: () => request<ApiToken[]>("GET", "/v1/auth/tokens"),
  createToken: (label: string) => request<NewApiToken>("POST", "/v1/auth/tokens", { body: { label } }),
  revokeToken: (id: string) => request<void>("DELETE", `/v1/auth/tokens/${enc(id)}`),

  // Device and system
  health: () => request<Health>("GET", "/v1/health"),
  identity: () => request<Identity>("GET", "/v1/identity"),
  device: () => request<Device>("GET", "/v1/device"),
  metrics: () => request<Metrics>("GET", "/v1/metrics"),
  services: () => request<UnitStatus[]>("GET", "/v1/system/services"),
  restartService: (unit: string) => request<UnitStatus>("POST", `/v1/system/services/${enc(unit)}/restart`),
  reboot: () => request<{ accepted: boolean; message: string }>("POST", "/v1/system/reboot"),
  safeMode: () => request<never>("POST", "/v1/system/safe-mode"),
  processes: () => request<ProcessInfo[]>("GET", "/v1/system/processes"),
  signal: (pid: number, signal: "TERM" | "KILL" | "STOP" | "CONT") => request<{ accepted: boolean; message: string }>("POST", `/v1/system/processes/${pid}/signal`, { body: { signal } }),
  setAffinity: (pid: number, core: number | null) => request<never>("PUT", `/v1/system/processes/${pid}/affinity`, { body: { core } }),
  setNice: (pid: number, nice: number) => request<never>("PUT", `/v1/system/processes/${pid}/nice`, { body: { nice } }),

  // Logs and events
  logs: (query: { unit?: string; lines?: number; since_ms?: number; level?: "error" | "warn" | "info" | "debug" } = {}) => request<{ lines: LogLine[] }>("GET", "/v1/logs", { query }),
  followLogs: (query: { unit?: string; level?: string }, onLine: (line: LogLine) => void) => subscribe("/v1/logs/stream", query, { onEvent: (e) => onLine(e as unknown as LogLine) }, ["log"]),
  events: (handlers: { onEvent: (event: ApiEvent) => void; onOpen?: () => void; onError?: () => void }, types?: string[]) =>
    subscribe("/v1/events", types ? { types: types.join(",") } : undefined, handlers, types ?? EVENT_TYPES),

  // Cameras
  cameras: () => request<Camera[]>("GET", "/v1/cameras"),
  camera: (id: string) => request<Camera>("GET", `/v1/cameras/${enc(id)}`),
  cameraSettings: (id: string) => request<CameraSettings>("GET", `/v1/cameras/${enc(id)}/settings`),
  setCameraSettings: (id: string, changes: CameraControlChanges) => request<CameraSettingsApplied>("PATCH", `/v1/cameras/${enc(id)}/settings`, { body: changes }),
  /** Every writable control back to its default; the values kept across reboots are forgotten. */
  resetCameraSettings: (id: string) => request<CameraSettingsApplied>("DELETE", `/v1/cameras/${enc(id)}/settings`),
  setMount: (id: string, mount: CameraMount) => request<CameraMount>("PUT", `/v1/cameras/${enc(id)}/mount`, { body: mount }),
  clearMount: (id: string) => request<void>("DELETE", `/v1/cameras/${enc(id)}/mount`),
  calibrations: (id: string) => request<CameraCalibrations>("GET", `/v1/cameras/${enc(id)}/calibration`),
  putCalibration: (id: string, calibration: CameraCalibration) => request<CameraCalibrations>("PUT", `/v1/cameras/${enc(id)}/calibration`, { body: calibration }),
  captureCalibration: (id: string, body: { board: CalibrationBoard; width?: number; height?: number; keep?: boolean }) => request<CalibrationCapture>("POST", `/v1/cameras/${enc(id)}/calibration/capture`, { body }),
  solveCalibration: (id: string, body: { model?: "pinhole" | "fisheye"; drop?: number[] }) => request<CalibrationSolve>("POST", `/v1/cameras/${enc(id)}/calibration/solve`, { body }),
  resetCalibration: (id: string) => request<void>("DELETE", `/v1/cameras/${enc(id)}/calibration/views`),

  // Pipelines
  pipelines: () => request<Pipeline[]>("GET", "/v1/pipelines"),
  pipelineTemplates: () => request<PipelineTemplate[]>("GET", "/v1/pipelines/templates"),
  pipeline: (id: string) => request<Pipeline>("GET", `/v1/pipelines/${enc(id)}`),
  createPipeline: (spec: PipelineSpec & { id?: string }) => request<Pipeline>("POST", "/v1/pipelines", { body: spec }),
  putPipeline: (id: string, spec: PipelineSpec) => request<Pipeline>("PUT", `/v1/pipelines/${enc(id)}`, { body: spec }),
  deletePipeline: (id: string) => request<void>("DELETE", `/v1/pipelines/${enc(id)}`),
  startPipeline: (id: string) => request<Pipeline>("POST", `/v1/pipelines/${enc(id)}/start`),
  stopPipeline: (id: string) => request<Pipeline>("POST", `/v1/pipelines/${enc(id)}/stop`),
  restartPipeline: (id: string) => request<Pipeline>("POST", `/v1/pipelines/${enc(id)}/restart`),
  rollbackPipeline: (id: string) => request<Pipeline>("POST", `/v1/pipelines/${enc(id)}/rollback`),
  revisions: (id: string) => request<Revision[]>("GET", `/v1/pipelines/${enc(id)}/revisions`),
  bindInput: (id: string, input: string, binding: Binding) => request<Pipeline>("PUT", `/v1/pipelines/${enc(id)}/bindings/${enc(input)}`, { body: binding }),
  outputs: () => request<PipelineOutput[]>("GET", "/v1/outputs"),
  plugins: () => request<{ engine_running: boolean; plugins: string[] }>("GET", "/v1/plugins"),
  catalog: () => request<DeviceCatalog>("GET", "/v1/catalog"),
  nt4: () => request<Nt4State>("GET", "/v1/device/nt4"),
  setNt4: (settings: Nt4Settings) => request<Nt4State>("PUT", "/v1/device/nt4", { body: settings }),

  // Playback: recordings and image folders replayed as cameras. Transport is the player's camera settings
  // (the recordings it lists come from `recordings()` below).
  playbackPlayers: () => request<PlayerView[]>("GET", "/v1/playback"),
  openPlayback: (body: PlaybackCreate) => request<PlayerView>("POST", "/v1/playback", { body }),
  closePlayback: (id: string) => request<void>("DELETE", `/v1/playback/${enc(id)}`),
  runOnPlayback: (id: string, pipeline: string) => request<PlaybackRun>("POST", `/v1/playback/${enc(id)}/runs`, { body: { pipeline } }),

  // The LED ring (settings, locate, and previewing a state)
  leds: () => request<LedsResponse>("GET", "/v1/device/leds"),
  setLeds: (settings: LedSettings) => request<LedsResponse>("PUT", "/v1/device/leds", { body: settings }),
  locateLeds: (seconds = 10) => request<LedsResponse>("POST", "/v1/device/leds/locate", { body: { seconds } }),
  /** Shows `state` for `seconds`; `null` ends the preview. */
  previewLeds: (state: LedState | null, seconds?: number) => request<LedsResponse>("POST", "/v1/device/leds/preview", { body: seconds === undefined ? { state } : { state, seconds } }),

  // Session recordings (helios-recorder): record a camera's frames, keep and export them
  recordings: () => request<RecordingListing>("GET", "/v1/recordings"),
  startRecording: (camera: string) => request<RecordingStatus>("POST", "/v1/recordings", { body: { camera } }),
  stopRecording: (id: string) => request<RecordingStatus>("POST", `/v1/recordings/${enc(id)}/stop`),
  deleteRecording: (id: string) => request<void>("DELETE", `/v1/recordings/${enc(id)}`),
  /** The last seconds of a camera's ring, written as a new stored recording. */
  captureLast: (body: { seconds?: number; camera?: string }) => request<PartSummary>("POST", "/v1/recordings/capture-last", { body }),
  exportEidos: (id: string) => request<EidosExport>("POST", `/v1/recordings/${enc(id)}/export/eidos`),
  recordingSettings: () => request<RecorderSettings>("GET", "/v1/recordings/settings"),
  setRecordingSettings: (settings: RecorderSettings) => request<RecorderSettings>("PUT", "/v1/recordings/settings", { body: settings }),

  // Resources and peripherals
  resources:(type?: string) => request<Resource[]>("GET", "/v1/resources", { query: { type } }),
  peripherals: () => request<Resource[]>("GET", "/v1/peripherals"),
  peripheralAction: (id: string, kind: string, arg: Record<string, unknown> = {}) =>
    request<{ action_id: string; resource: string; done: boolean; result: Resource["action_result"] }>("POST", `/v1/peripherals/${enc(id)}/actions`, { body: { kind, arg } }),
  /** Raw GPIO/PWM/I2C/SPI through lemnosd: the live claims, and a raw action (`gpio.claim`, ...). */
  rawIo: () => request<RawIo>("GET", "/v1/peripherals/io"),
  rawIoAction: (kind: string, arg: Record<string, unknown> = {}) =>
    request<{ action_id: string; resource: string; done: boolean; result: Resource["action_result"] }>("POST", "/v1/peripherals/io/actions", { body: { kind, arg } }),

  // Updates
  uploadImage: (file: Blob, options: { filename?: string; sha256?: string; version?: string } = {}) =>
    request<Upload>("POST", "/v1/update/uploads", { query: { filename: options.filename ?? (file instanceof File ? file.name : undefined), sha256: options.sha256, version: options.version }, raw: file, headers: { "Content-Type": "application/octet-stream" } }),
  uploads: () => request<Upload[]>("GET", "/v1/update/uploads"),
  deleteUpload: (id: string) => request<void>("DELETE", `/v1/update/uploads/${enc(id)}`),
  applyUpdate: (body: { upload_id?: string; image_url?: string; version?: string; sha256?: string; reboot?: boolean }) => request<ApplyResponse>("POST", "/v1/update/apply", { body }),
  updateStatus: () => request<UpdateStatus>("GET", "/v1/update/status"),
  switchSlot: () => request<never>("POST", "/v1/update/slots/switch"),
};

export type HeliosApi = typeof api;
