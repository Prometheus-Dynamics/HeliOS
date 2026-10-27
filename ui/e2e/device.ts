// What the sim's device says, read through helios-api (`/v1`). The UI shows what the device
// reports; these checks read the device itself, after each UI action, so a UI that shows one thing
// while it deploys another fails here. Shapes: backend/src/helios/api/src/routes/{pipelines,cameras}.rs.
import { expect, type APIRequestContext } from "@playwright/test";

/** A Daedalus graph document as the device stores it (`GET /v1/pipelines/{id}`, `graph`). */
export interface DaedalusNode {
  id: string;
  label: string;
  inputs: string[];
  outputs: string[];
  const_inputs: [string, DaedalusValue][];
  metadata: Record<string, DaedalusValue>;
  [key: string]: unknown;
}
export interface DaedalusValue {
  type: string;
  value: unknown;
}
export interface DaedalusGraph {
  format: string;
  schema_version: number;
  requires: { id: string; version?: string }[];
  metadata: Record<string, unknown>;
  graph: { nodes: DaedalusNode[]; edges: unknown[]; metadata?: unknown };
}
export interface DevicePipeline {
  id: string;
  name: string;
  enabled: boolean;
  state: string;
  revision: number | null;
  graph: DaedalusGraph | null;
  bindings: Record<string, { resource_id?: string; output_width?: number; output_height?: number; pyramid?: number; source?: string }>;
  dictionary: string | null;
  search_mode: "tracked" | "full" | null;
  full_search_every: number | null;
  loss_full_search_after: number | null;
  margin_growth_misses: number | null;
  field_layout: string | null;
  cpu: number | null;
  nice: number | null;
  managed: boolean;
}
/** One stage of a camera's status chain (`GET /v1/cameras`, `status.stages`). */
export interface CameraStage {
  stage: string;
  state: "ok" | "waiting" | "blocked" | "off";
  reason: string;
}
export interface DeviceCamera {
  id: string;
  name: string;
  settings_writable: boolean;
  /** The Styx frames socket, when the camera serves frames. */
  frames_endpoint?: string | null;
  backend?: string | null;
  service_online?: boolean | null;
  live_error?: string | null;
  live?: { captures: { mode: string; fps_measured: number | null; frames_delivered: number; last_error: string | null }[] } | null;
  status?: { stages: CameraStage[]; summary: string } | null;
}
/** A camera control as `GET /v1/cameras/{id}/settings` lists it. `standard` is its role (`exposure_us`, `gain`, `ae`...). */
export interface CameraControl {
  name: string;
  standard?: string | null;
  current?: unknown;
  [key: string]: unknown;
}
export interface DeviceCameraSettings {
  writable: boolean;
  controls: CameraControl[];
  /** The capture's mode (`MJPG 1280x960`), which the camera's pipelines' bindings choose. */
  mode?: string | null;
  /** The summary the libcamera devices report; null where the camera does not report it (a UVC camera). */
  exposure_us: number | null;
  analogue_gain: number | null;
  persisted: Record<string, unknown>;
  live_error: string | null;
}

/**
 * A camera's exposure or gain as the device reports it. The summary fields are the libcamera
 * devices' (exposure in µs); a camera without them answers through its control with the role.
 */
export function readingOf(settings: DeviceCameraSettings, role: "exposure_us" | "gain"): number | null {
  const summary = role === "exposure_us" ? settings.exposure_us : settings.analogue_gain;
  if (summary !== null) return summary;
  const current = settings.controls.find((c) => c.standard === role)?.current;
  return typeof current === "number" ? current : null;
}

async function getJson<T>(api: APIRequestContext, path: string): Promise<T> {
  const response = await api.get(path);
  expect(response.ok(), `GET ${path}: HTTP ${response.status()}`).toBeTruthy();
  return (await response.json()) as T;
}

export const device = {
  pipelines: (api: APIRequestContext) => getJson<DevicePipeline[]>(api, "/v1/pipelines"),
  pipeline: (api: APIRequestContext, id: string) => getJson<DevicePipeline>(api, `/v1/pipelines/${encodeURIComponent(id)}`),
  cameras: (api: APIRequestContext) => getJson<DeviceCamera[]>(api, "/v1/cameras"),
  cameraSettings: (api: APIRequestContext, id: string) => getJson<DeviceCameraSettings>(api, `/v1/cameras/${encodeURIComponent(id)}/settings`),
  templates: (api: APIRequestContext) => getJson<{ id: string }[]>(api, "/v1/pipelines/templates"),
};

/** The Eidos pipelines bound to a camera: the ones the default-pipeline chooser manages. */
export function eidosPipelinesOn(pipelines: DevicePipeline[], cameraId: string): DevicePipeline[] {
  return pipelines.filter((p) => p.graph?.requires.some((r) => r.id === "eidos") && Object.values(p.bindings).some((b) => b.resource_id === cameraId));
}

/** The one enabled pipeline bound to a camera (the chooser's pipeline); fails the test otherwise. */
export async function enabledPipelineOn(api: APIRequestContext, cameraId: string): Promise<DevicePipeline> {
  const found = eidosPipelinesOn(await device.pipelines(api), cameraId).filter((p) => p.enabled);
  expect(found.length, "one enabled Eidos pipeline on the camera").toBe(1);
  return found[0];
}

/** The node types of a pipeline's graph (Daedalus `id`s, such as `eidos:mapping.live_map`). */
export function nodeTypes(p: DevicePipeline): string[] {
  return (p.graph?.graph.nodes ?? []).map((n) => n.id);
}

/** A const input's value, by node label and input name. */
export function constOf(p: DevicePipeline, label: string, input: string): DaedalusValue | undefined {
  const node = p.graph?.graph.nodes.find((n) => n.label === label);
  return node?.const_inputs.find(([name]) => name === input)?.[1];
}

/**
 * The camera the device tests drive: `HELIOS_E2E_CAMERA` (an id, or part of a name) when set, else
 * the sim's simulated sensor, else the first camera that serves frames (a real board's own
 * camera), so the `sim` project runs against the simulator and against a real device alike.
 */
export async function simCamera(api: APIRequestContext): Promise<DeviceCamera> {
  const cameras = await device.cameras(api);
  const wanted = process.env.HELIOS_E2E_CAMERA;
  const camera = wanted
    ? cameras.find((c) => c.id === wanted || c.name.includes(wanted))
    : (cameras.find((c) => c.name.includes("simulated")) ?? cameras.find((c) => c.frames_endpoint));
  expect(camera, wanted ? `camera ${wanted} is listed` : "the device lists a camera that serves frames").toBeTruthy();
  return camera!;
}

/** How long a camera may take to be ready after the device starts (its service, then its frames). */
const CAMERA_READY_MS = 180_000;
/** How long a restored value may take to show on the device (a pipeline or camera restarts first). */
const RESTORE_MS = 90_000;

/**
 * Polls `read` until `done` holds, or fails with what it was last seen as (`describe`) and how long
 * it waited. A read that throws (a 503 while a service restarts) counts as not yet.
 */
export async function eventually<T>(what: string, read: () => Promise<T>, done: (value: T) => boolean, options: { timeoutMs: number; describe: (value: T | undefined, error: string | null) => string }): Promise<T> {
  const end = Date.now() + options.timeoutMs;
  for (;;) {
    let value: T | undefined;
    let error: string | null = null;
    try {
      value = await read();
      if (done(value)) return value;
    } catch (caught) {
      error = caught instanceof Error ? caught.message.split("\n")[0] : String(caught);
    }
    if (Date.now() >= end) {
      throw new Error(`timed out after ${Math.round(options.timeoutMs / 1000)} s waiting for ${what}: ${options.describe(value, error)}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
}

/**
 * Whether a camera can take a test now: its service answers, and its frames stage is flowing, or
 * no capture has been asked for yet (the capture starts with the first pipeline or view that uses it).
 */
export function cameraReadiness(camera: DeviceCamera): { ready: boolean; why: string } {
  if (camera.service_online !== true) return { ready: false, why: `its camera service is not online${camera.live_error ? ` (${camera.live_error})` : ""}` };
  const stages = camera.status?.stages ?? [];
  const service = stages.find((s) => s.stage === "service");
  if (service && service.state !== "ok") return { ready: false, why: service.reason };
  const frames = stages.find((s) => s.stage === "frames");
  if (!frames) return { ready: false, why: "the status chain has no frames stage yet" };
  if (frames.state === "ok" || (frames.state === "waiting" && frames.reason === "No capture yet")) return { ready: true, why: frames.reason };
  return { ready: false, why: `frames: ${frames.reason}` };
}

/** Whether the camera's capture is delivering frames now (`live.captures[0].fps_measured` above zero). */
function flowing(camera: DeviceCamera): boolean {
  return (camera.live?.captures[0]?.fps_measured ?? 0) > 0;
}

/**
 * Waits until the device has booted far enough for `camera` to be used: the camera service is online
 * and its frames stage is flowing (see `cameraReadiness`). Fails after `CAMERA_READY_MS` with the
 * stage that is holding it up, which is what a reader needs when the device was just started.
 */
export async function waitForCameraReady(api: APIRequestContext, camera: { id: string; name: string }) {
  await eventually(`camera ${camera.name} to be ready (the device may still be starting)`, async () => (await device.cameras(api)).find((c) => c.id === camera.id), (c) => c !== undefined && cameraReadiness(c).ready, {
    timeoutMs: CAMERA_READY_MS,
    describe: (c, error) => (c ? cameraReadiness(c).why : (error ?? `the device does not list ${camera.id}`)),
  });
}

/** What a sim test may change, as the device holds it before the test. */
export interface DeviceSnapshot {
  onboarded: boolean;
  pipelines: DevicePipeline[];
  cameras: { id: string; name: string; persisted: Record<string, unknown>; mode: string | null; flowing: boolean }[];
}

/** A pipeline's spec: what `PUT /v1/pipelines/{id}` takes, read back from the device. */
const SPEC_OPTIONAL = ["search_mode", "full_search_every", "loss_full_search_after", "margin_growth_misses", "field_layout", "dictionary", "cpu", "nice"] as const;
function specOf(p: DevicePipeline): Record<string, unknown> {
  const spec: Record<string, unknown> = { name: p.name, graph: p.graph, bindings: p.bindings, enabled: p.enabled };
  for (const key of SPEC_OPTIONAL) {
    if (p[key] !== null && p[key] !== undefined) spec[key] = p[key];
  }
  return spec;
}

/**
 * The spec as text, for comparing two of them: keys in order, and each node's const inputs by name.
 * The device keeps a node's const inputs in its own order, which a write does not change.
 */
function canonical(value: unknown, key = ""): string {
  return JSON.stringify(sortedForCompare(value, key));
}
function sortedForCompare(value: unknown, key: string): unknown {
  if (Array.isArray(value)) {
    const items = value.map((item) => sortedForCompare(item, ""));
    return key === "const_inputs" ? items.sort((a, b) => String((a as [string])[0]).localeCompare(String((b as [string])[0]))) : items;
  }
  if (value !== null && typeof value === "object") {
    const record = value as Record<string, unknown>;
    return Object.fromEntries(Object.keys(record).sort().map((name) => [name, sortedForCompare(record[name], name)]));
  }
  return value;
}

/** Reads what a sim test may change: each pipeline, each frame camera's kept settings and mode, and the setup flag. */
export async function snapshotDevice(api: APIRequestContext): Promise<DeviceSnapshot> {
  const setup = await getJson<{ onboarded: boolean }>(api, "/v1/device/setup");
  const pipelines = await device.pipelines(api);
  const cameras: DeviceSnapshot["cameras"] = [];
  for (const camera of (await device.cameras(api)).filter((c) => c.frames_endpoint)) {
    const settings = await device.cameraSettings(api, camera.id);
    cameras.push({ id: camera.id, name: camera.name, persisted: settings.persisted, mode: settings.mode ?? null, flowing: flowing(camera) });
  }
  return { onboarded: setup.onboarded, pipelines, cameras };
}

async function send(api: APIRequestContext, method: "POST" | "PUT" | "PATCH" | "DELETE", path: string, data?: unknown) {
  const response = await api.fetch(path, { method, data });
  const body = response.ok() ? "" : (await response.text()).slice(0, 300);
  expect(response.ok(), `${method} ${path}: HTTP ${response.status()} ${body}`).toBeTruthy();
}

/**
 * Puts the device back as `before` found it: the setup flag, each pipeline's spec (recreated when
 * it is gone, and a pipeline a test added is removed), and each camera's kept settings, then waits
 * until the pipelines and the cameras show it. A camera that was delivering frames must deliver
 * them again.
 */
export async function restoreDevice(api: APIRequestContext, before: DeviceSnapshot) {
  const setup = await getJson<{ onboarded: boolean }>(api, "/v1/device/setup");
  if (setup.onboarded !== before.onboarded) await send(api, "PUT", "/v1/device/setup", { onboarded: before.onboarded });

  const now = await device.pipelines(api);
  for (const p of before.pipelines) {
    // A pipeline whose graph does not load has no spec to put back, and no test edits one.
    if (!p.graph) continue;
    const current = now.find((c) => c.id === p.id);
    if (!current) await send(api, "POST", "/v1/pipelines", { id: p.id, ...specOf(p) });
    else if (canonical(specOf(current)) !== canonical(specOf(p))) await send(api, "PUT", `/v1/pipelines/${encodeURIComponent(p.id)}`, specOf(p));
  }
  for (const p of now) {
    if (!before.pipelines.some((b) => b.id === p.id)) await send(api, "DELETE", `/v1/pipelines/${encodeURIComponent(p.id)}`);
  }
  for (const p of before.pipelines) {
    if (!p.graph) continue;
    await eventually(`pipeline ${p.id} to show its spec as found`, () => device.pipeline(api, p.id), (c) => canonical(specOf(c)) === canonical(specOf(p)), {
      timeoutMs: RESTORE_MS,
      describe: (c, error) => (c ? `it has ${JSON.stringify(specOf(c)).slice(0, 200)}` : (error ?? "no answer")),
    });
  }
  const gone = (await device.pipelines(api)).filter((p) => !before.pipelines.some((b) => b.id === p.id)).map((p) => p.id);
  expect(gone, "pipelines a test added are removed").toEqual([]);

  for (const camera of before.cameras) {
    // Restarting a pipeline restarts the camera's service: its settings answer 503 until it is back.
    await waitForCameraReady(api, camera);
    const settings = await device.cameraSettings(api, camera.id);
    if (canonical(settings.persisted) !== canonical(camera.persisted)) {
      await send(api, "DELETE", `/v1/cameras/${encodeURIComponent(camera.id)}/settings`);
      if (Object.keys(camera.persisted).length > 0) await send(api, "PATCH", `/v1/cameras/${encodeURIComponent(camera.id)}/settings`, camera.persisted);
    }
    await eventually(`${camera.name} to keep its settings as found`, () => device.cameraSettings(api, camera.id), (s) => canonical(s.persisted) === canonical(camera.persisted), {
      timeoutMs: RESTORE_MS,
      describe: (s, error) => (s ? `it keeps ${JSON.stringify(s.persisted)}` : (error ?? "no answer")),
    });
    if (camera.mode !== null) {
      await eventually(`${camera.name} in its mode as found (${camera.mode})`, () => device.cameraSettings(api, camera.id), (s) => s.mode === camera.mode, {
        timeoutMs: RESTORE_MS,
        describe: (s, error) => (s ? `it is in ${s.mode}` : (error ?? "no answer")),
      });
    }
    if (camera.flowing) {
      await eventually(`${camera.name} to deliver frames again`, async () => (await device.cameras(api)).find((c) => c.id === camera.id), (c) => c !== undefined && flowing(c), {
        timeoutMs: RESTORE_MS,
        describe: (c, error) => (c ? (c.live?.captures[0]?.last_error ?? cameraReadiness(c).why) : (error ?? "not listed")),
      });
    }
  }
}
