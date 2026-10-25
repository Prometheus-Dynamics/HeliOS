// The live side of the cluster store: loads the device from helios-api, keeps
// it current from the event stream (with a slow poll as a safety net) and
// turns the screens' actions into API calls. Features the device has no
// backend for yet (501) are reported once, as "not available".

import { dictionaryLabel } from "#lib/format.js";
import { applyCameraSettings, applyControlValue, applyPersisted, fromDaedalus, toCamera, toDaedalus, toLogLine, toNode, toResource, toStreams, toWorkload, type DeviceSnapshot } from "#lib/api/adapt.js";
import type { Detection } from "#lib/api/model.js";
import { api, ApiError, errorText, isNotAvailable, liveOutputsUrl } from "#lib/api/client.js";
import { LiveOutputs } from "./live-outputs";
import type { Camera, CameraSettings, ClusterNode, GraphDocument, LogLine, PipelineInput, Resource, Stream, Workload } from "#lib/api/model.js";
import { defaultsFrom, type DefaultTemplate } from "#lib/core/defaults.js";

/** The live mapper's outputs: Eidos's template names and the older ones. */
const MAP_PORTS = new Set(["map_pose", "map_snapshot", "map_timing", "pose", "snapshot", "timing"]);
/** The dense mapper's outputs (`eidos:dense.map`). */
const DENSE_PORTS = new Set(["dense_mesh", "dense_depth", "dense_timing"]);
/** The tag poses and the field pose of a pose graph. */
const POSE_PORTS = new Set(["pose_solutions", "multi_tag_pose"]);
/** A camera's intrinsics are read again after this long (a recalibration shows within a minute). */
const INTRINSICS_TTL_MS = 60_000;
import type * as W from "#lib/api/types.js";
import { mergeDeviceCatalog } from "#lib/api/catalog.js";
import { toFeatureTracks, TrailBook, type CameraFeatures } from "#lib/vision/features.js";
import { parseMapPose, parseMapSnapshot, parseMapTiming, TimingWindow, type MapView } from "#lib/vision/map.js";
import { parseDenseDepth, parseDenseMesh, parseDenseTiming, type DenseView } from "#lib/vision/dense.js";
import { parseFieldPose, parseTagPoses, type Intrinsics, type PoseView } from "#lib/vision/pose.js";
import { pickFile } from "#lib/files.js";
import { clampedNote } from "#lib/camera/controls.js";
import { toasts } from "./toasts.svelte";
import { recordings } from "./recordings.svelte";
import { emitPeripheral, type PeripheralEvent } from "./peripheral-events";
import type { RecorderSettings, RecordingStatus } from "#lib/api/recording-types.js";

/** The parts of the cluster store the live backend fills in. */
export interface ClusterData {
  nodes: ClusterNode[];
  cameras: Camera[];
  workloads: Workload[];
  streams: Stream[];
  resources: Resource[];
  logs: LogLine[];
}

const DERIVED_TYPES = new Set(["execution.session", "execution.artifact", "system.update.execution"]);
/** The camera pane's settings fields that are camera controls on the device, by API key. */
const CONTROL_KEYS: Record<string, string> = { exposureUs: "exposure_us", autoExposure: "ae", gain: "gain", fps: "fps", colourTemp: "colour_temperature" };
/** Styx's standard controls, which the API takes by these keys. */
const STANDARD_KEYS = new Set(["exposure_us", "gain", "ae", "ev", "fps", "awb", "colour_temperature", "red_gain", "blue_gain", "af_mode", "af_trigger", "lens_position"]);
/** Set per pipeline (its camera binding), not on the camera. */
const PER_PIPELINE = new Set(["width", "height", "format", "pyramid", "roi"]);
/** Control changes made while dragging are sent at most this often per camera. */
const CONTROL_FLUSH_MS = 120;

/** The device key a camera field writes to: its camera key, a standard key or a listed control's name; null when it is none. */
function deviceKeyOf(key: string, camera: Camera | undefined): string | null {
  if (CONTROL_KEYS[key]) return CONTROL_KEYS[key];
  if (STANDARD_KEYS.has(key) || camera?.controls?.some((c) => c.name === key)) return key;
  return null;
}

const scalarOf = (value: W.ControlValue): number | boolean | null => (typeof value === "number" || typeof value === "boolean" ? value : null);
/** A rectangle written to a region control (`ae_metering_region`). */
const isRectValue = (value: unknown): value is { x: number; y: number; width: number; height: number } => typeof value === "object" && value !== null && "width" in value && "height" in value;
const LOG_CAP = 600;

class LiveState {
  connected = $state(false);
  error = $state<string | null>(null);
  update = $state<W.UpdateStatus | null>(null);
  identity = $state<W.Identity | null>(null);
  plugins = $state<string[]>([]);
  /** Where pipelines publish for robot code, and whether that is connected. */
  nt4 = $state<W.Nt4State | null>(null);
  /** Bumped when the device's node catalog is merged into `CATALOG`, for what lists it. */
  catalogVersion = $state(0);
  /** Camera mounts stored on the device (robot frame, metres, degrees). */
  mounts = $state<Record<string, W.CameraMount>>({});
}

export const liveState = new LiveState();

/**
 * Pipeline outputs arrive as `output` events (helios-engine publishes them at most every
 * 250 ms); while none have come for this long (no event stream, an older device), they are
 * read with `GET /v1/outputs` instead.
 */
const OUTPUTS_FALLBACK_MS = 1000;

/** A `detections` output's value as overlay detections. */
function toDetections(value: unknown): Detection[] {
  type Point = { x: number; y: number };
  const out = value as { detections?: { id: number; corners: Point[] }[]; subpixel?: Point[][]; frame_width?: number; frame_height?: number } | undefined;
  // Corners are in the pipeline's frame (frame_width x frame_height, usually the sensor mode),
  // not the preview's: the feed scales them. Sub-pixel corners, when present, are the refined ones.
  const frame = out?.frame_width && out?.frame_height ? { w: out.frame_width, h: out.frame_height } : undefined;
  return (out?.detections ?? []).map((d, i) => {
    const refined = out?.subpixel?.[i];
    const points = refined?.length === d.corners.length ? refined : d.corners;
    return { id: d.id, corners: points.map((c) => [c.x, c.y] as [number, number]), frame };
  });
}

export class LiveCluster {
  private pipelines = new Map<string, W.Pipeline>();
  /** The latest detections per camera resource, from its running pipeline's `detections` output. */
  private detections = $state<Record<string, Detection[]>>({});
  /** The latest feature tracks per camera resource, from its running pipeline's `tracks` output. */
  private features = $state<Record<string, CameraFeatures>>({});
  /** One trail book per camera resource: its tracks' trails, advanced once per new tick. */
  private trailBooks = new Map<string, TrailBook>();
  /** The latest live-map outputs per pipeline (`pose`, `snapshot`, `timing`), for the Map pane. */
  private maps = $state<Record<string, MapView>>({});
  /** The frame timings of each live-map pipeline over the last second, for the Map pane's p50s. */
  private mapTimings = new Map<string, TimingWindow>();
  /** The dense mapper's latest mesh, depth and timing per pipeline (`dense_*`). */
  private dense = $state<Record<string, DenseView>>({});
  /** The latest tag poses and field pose per pipeline (`pose_solutions`, `multi_tag_pose`). */
  private poses = $state<Record<string, PoseView>>({});
  /** Each camera's intrinsics from its calibration (null: none yet); read again after INTRINSICS_TTL_MS. */
  private intrinsics = $state<Record<string, Intrinsics | null>>({});
  private intrinsicsAt = new Map<string, number>();
  /** The observed time of each dense or pose output last applied, so the same one is not applied twice. */
  private seenAt = new Map<string, number>();
  private snapshot: Partial<DeviceSnapshot> = {};
  private timers: number[] = [];
  private unsubscribers: (() => void)[] = [];
  private pending = new Map<string, number>();
  private warned = new Set<string>();
  private controlChanges = new Map<string, W.CameraControlChanges>();
  private lastOutputEvent = 0;

  constructor(private data: ClusterData) {}

  get nodeId(): string {
    return this.snapshot.device?.node_id ?? "device";
  }

  /** Every output of every tick (`/v1/live/ws`): the latest applied once per frame, a short history per output for frame matching. */
  readonly liveOutputs = new LiveOutputs(liveOutputsUrl(), (outputs) => {
    for (const output of outputs) this.applyOutput({ id: output.id, change: "updated", value: output.value }, true);
  });

  start(): () => void {
    void this.refreshAll();
    this.unsubscribers.push(this.liveOutputs.start());
    this.unsubscribers.push(
      api.events({
        onOpen: () => {
          liveState.connected = true;
          liveState.error = null;
        },
        onError: () => {
          liveState.connected = false;
        },
        onEvent: (event) => this.onEvent(event),
      }),
    );
    this.unsubscribers.push(api.followLogs({}, (line) => this.appendLog(toLogLine(line, this.nodeId))));
    this.timers.push(window.setInterval(() => void this.refreshDevice(), 10_000));
    this.timers.push(window.setInterval(() => void this.refreshCameras(), 15_000));
    this.timers.push(window.setInterval(() => void this.refreshPipelines(), 15_000));
    this.timers.push(
      window.setInterval(() => {
        if (performance.now() - this.lastOutputEvent > OUTPUTS_FALLBACK_MS) void this.refreshOutputs();
      }, OUTPUTS_FALLBACK_MS / 4),
    );
    return () => {
      for (const t of this.timers) clearInterval(t);
      for (const u of this.unsubscribers) u();
      this.timers = [];
      this.unsubscribers = [];
    };
  }

  // --- Loading -----------------------------------------------------------

  private async guard<T>(what: string, run: () => Promise<T>): Promise<T | undefined> {
    try {
      const value = await run();
      liveState.error = null;
      return value;
    } catch (error) {
      if (error instanceof ApiError && error.code === "network") liveState.connected = false;
      liveState.error = `${what}: ${errorText(error)}`;
      return undefined;
    }
  }

  async refreshAll() {
    await this.refreshDevice();
    await Promise.all([this.refreshCameras(), this.refreshPipelines(), this.refreshResources(), this.refreshLogs(), recordings.refresh()]);
  }

  async refreshDevice() {
    const device = await this.guard("device", () => api.device());
    if (!device) return;
    this.snapshot.device = device;
    void this.refreshNt4();
    if (!this.snapshot.identity) this.snapshot.identity = (await this.guard("identity", () => api.identity())) ?? undefined;
    liveState.identity = this.snapshot.identity ?? null;
    const [metrics, services, update] = await Promise.all([
      this.guard("metrics", () => api.metrics()),
      this.guard("services", () => api.services()),
      this.guard("update status", () => api.updateStatus()),
    ]);
    if (metrics) this.snapshot.metrics = metrics;
    if (services) this.snapshot.services = services;
    if (update) {
      this.snapshot.update = update;
      liveState.update = update;
    }
    this.applyNode();
  }

  private applyNode() {
    const s = this.snapshot;
    if (!s.device) return;
    const previous = this.data.nodes.find((n) => n.id === s.device!.node_id);
    const node = toNode({ device: s.device, identity: s.identity ?? null, metrics: s.metrics ?? null, services: s.services ?? [], update: s.update ?? null }, previous);
    this.data.nodes = [node];
  }

  async refreshCameras() {
    const cameras = await this.guard("cameras", () => api.cameras());
    if (!cameras) return;
    const previous = new Map(this.data.cameras.map((c) => [c.resourceId, c]));
    this.data.cameras = cameras.map((wire) => {
      const camera = toCamera(wire);
      const before = previous.get(camera.resourceId);
      if (before?.controls) {
        camera.controls = before.controls;
        for (const control of camera.controls) if (control.standard) applyControlValue(camera, { standard: control.standard }, control.value);
      }
      return camera;
    });
    liveState.mounts = Object.fromEntries(cameras.filter((c) => c.mount).map((c) => [c.id, c.mount!]));
    await Promise.all(cameras.filter((c) => c.settings_writable).map((c) => this.refreshCameraControls(c.id)));
  }

  /** The camera's controls as its camera service lists them. */
  async refreshCameraControls(id: string) {
    try {
      const settings = await api.cameraSettings(id);
      const camera = this.data.cameras.find((c) => c.resourceId === id);
      if (camera) applyCameraSettings(camera, settings);
    } catch (error) {
      const camera = this.data.cameras.find((c) => c.resourceId === id);
      if (camera) camera.controlsError = errorText(error);
    }
  }

  async refreshPipelines() {
    const pipelines = await this.guard("pipelines", () => api.pipelines());
    if (!pipelines) return;
    this.pipelines = new Map(pipelines.map((p) => [p.id, p]));
    const previous = new Map(this.data.workloads.map((w) => [w.id, w]));
    this.data.workloads = pipelines.map((p) => toWorkload(p, this.nodeId, previous.get(p.id)));
    this.data.streams = toStreams(pipelines);
    const plugins = await this.guard("plugins", () => api.plugins());
    if (plugins) {
      const changed = plugins.plugins.join() !== liveState.plugins.join();
      liveState.plugins = plugins.plugins;
      if (changed || !liveState.catalogVersion) await this.refreshCatalog();
    }
  }

  async refreshNt4() {
    try {
      liveState.nt4 = await api.nt4();
    } catch {
      liveState.nt4 = null;
    }
  }

  /** The nodes helios-engine's plugins declare, as the palette's catalog. */
  async refreshCatalog() {
    // New plugins may ship new templates: read them again next time.
    this.templates = undefined;
    try {
      mergeDeviceCatalog(await api.catalog());
      liveState.catalogVersion++;
    } catch {
      // Not written yet (engine starting) or an older device: the UI's own list stays.
    }
  }

  /** The device's pipeline templates, read once (`GET /v1/pipelines/templates`); undefined while
   * the device cannot answer (the error is in `liveState.error`), and asked again next time. */
  private templates?: W.PipelineTemplate[];
  /** The device's templates have been read (and offered lists are its own). */
  get templatesKnown(): boolean {
    return this.templates !== undefined;
  }
  private async loadTemplates(): Promise<W.PipelineTemplate[] | undefined> {
    if (this.templates) return this.templates;
    const templates = await this.guard("templates", () => api.pipelineTemplates());
    if (templates) this.templates = templates;
    return templates;
  }

  /** Detections for each camera with a running pipeline (one small request for all of them). */
  async refreshOutputs() {
    if (typeof document !== "undefined" && document.hidden) return;
    const running = [...this.pipelines.values()].filter((p) => p.state === "running");
    if (!running.length) {
      if (Object.keys(this.detections).length) this.detections = {};
      this.features = {};
      this.trailBooks.clear();
      this.maps = {};
      this.dense = {};
      this.poses = {};
      this.seenAt.clear();
      return;
    }
    let outputs: W.PipelineOutput[];
    try {
      outputs = await api.outputs();
    } catch {
      return;
    }
    const next: Record<string, Detection[]> = {};
    const nextFeatures: Record<string, CameraFeatures> = {};
    for (const p of running) {
      const camera = this.cameraOfPipeline(p);
      const out = outputs.find((o) => o.pipeline === p.id && o.port === "detections");
      if (camera && out) next[camera] = toDetections(out.value);
      const tracks = outputs.find((o) => o.pipeline === p.id && o.port === "tracks");
      const features = camera && tracks ? this.featuresOf(camera, p, tracks.value) : null;
      if (camera && features) nextFeatures[camera] = features;
    }
    this.detections = next;
    this.features = nextFeatures;
    for (const camera of this.trailBooks.keys()) if (!(camera in nextFeatures)) this.trailBooks.delete(camera);
    const nextMaps: Record<string, MapView> = {};
    for (const p of running) {
      const view = this.mapFromOutputs(p.id, outputs);
      if (view) nextMaps[p.id] = view;
    }
    this.maps = nextMaps;
    // The dense and pose outputs from the same list. A null value (nothing new this tick) keeps the latest.
    for (const p of running) {
      for (const port of [...DENSE_PORTS, ...POSE_PORTS]) {
        const out = outputs.find((o) => o.pipeline === p.id && o.port === port);
        if (!out || out.value === null || out.value === undefined) continue;
        if (DENSE_PORTS.has(port)) this.applyDense(p.id, port, { change: "updated", value: out });
        else this.applyPose(p.id, port, { change: "updated", value: out });
      }
    }
    const runningIds = new Set(running.map((p) => p.id));
    if (Object.keys(this.dense).some((id) => !runningIds.has(id))) this.dense = Object.fromEntries(Object.entries(this.dense).filter(([id]) => runningIds.has(id)));
    if (Object.keys(this.poses).some((id) => !runningIds.has(id))) this.poses = Object.fromEntries(Object.entries(this.poses).filter(([id]) => runningIds.has(id)));
  }

  /** The live-map view of a pipeline from a list of its outputs (null when it has none of them). */
  private mapFromOutputs(pipelineId: string, outputs: W.PipelineOutput[]): MapView | null {
    const value = (port: string) => outputs.find((o) => o.pipeline === pipelineId && o.port === port)?.value;
    // Eidos's templates name them map_pose, map_snapshot, map_timing; older graphs pose, snapshot, timing.
    const pose = parseMapPose(value("map_pose") ?? value("pose"));
    const snapshot = parseMapSnapshot(value("map_snapshot") ?? value("snapshot"));
    const timing = parseMapTiming(value("map_timing") ?? value("timing"));
    if (!pose && !snapshot && !timing) return null;
    if (timing) this.mapWindow(pipelineId).push(performance.now(), timing.frame_ms, timing.map_ms);
    return { pose, snapshot, timing };
  }

  private mapWindow(pipelineId: string): TimingWindow {
    let window = this.mapTimings.get(pipelineId);
    if (!window) {
      window = new TimingWindow(1000);
      this.mapTimings.set(pipelineId, window);
    }
    return window;
  }

  /** A live-map `pose`, `snapshot` or `timing` event: that output alone changes. */
  private applyMap(pipelineId: string, port: string, data: { change: string; value?: W.PipelineOutput }) {
    const pipeline = this.pipelines.get(pipelineId);
    if (data.change === "removed" || !data.value || !pipeline || pipeline.state !== "running") {
      if (pipelineId in this.maps) {
        const { [pipelineId]: _, ...rest } = this.maps;
        this.maps = rest;
      }
      this.mapTimings.delete(pipelineId);
      return;
    }
    const current = this.maps[pipelineId] ?? { pose: null, snapshot: null, timing: null };
    const raw = data.value.value;
    port = port.replace(/^map_/, "");
    if (port === "pose") current.pose = parseMapPose(raw);
    else if (port === "snapshot") current.snapshot = parseMapSnapshot(raw) ?? current.snapshot;
    else if (port === "timing") {
      current.timing = parseMapTiming(raw);
      if (current.timing) this.mapWindow(pipelineId).push(performance.now(), current.timing.frame_ms, current.timing.map_ms);
    }
    this.maps = { ...this.maps, [pipelineId]: { ...current } };
  }

  /** The live-map view of a pipeline: its latest pose, snapshot and timing. */
  mapFor(pipelineId: string): MapView | null {
    return this.maps[pipelineId] ?? null;
  }

  /** Whether an output with this observed time is one already applied (and so is not applied again). */
  private seen(key: string, observed: number): boolean {
    if (!observed) return false;
    if (this.seenAt.get(key) === observed) return true;
    this.seenAt.set(key, observed);
    return false;
  }

  /** Drops a pipeline's dense and pose state (it stopped, or its output went). */
  private forgetDense(pipelineId: string) {
    if (pipelineId in this.dense) {
      const { [pipelineId]: _, ...rest } = this.dense;
      this.dense = rest;
    }
    if (pipelineId in this.poses) {
      const { [pipelineId]: _, ...rest } = this.poses;
      this.poses = rest;
    }
    for (const key of [...this.seenAt.keys()]) if (key.startsWith(`${pipelineId}/`)) this.seenAt.delete(key);
  }

  /**
   * A `dense_mesh`, `dense_depth` or `dense_timing` event. The mesh comes at most once a second and
   * the depth at most twice a second, so a null value (nothing new this tick) keeps the latest of each.
   */
  private applyDense(pipelineId: string, port: string, data: { change: string; value?: W.PipelineOutput }) {
    const pipeline = this.pipelines.get(pipelineId);
    if (data.change === "removed" || !pipeline || pipeline.state !== "running") return this.forgetDense(pipelineId);
    const raw = data.value?.value;
    if (raw === null || raw === undefined) return;
    if (this.seen(`${pipelineId}/${port}`, data.value?.observed_at_ms ?? 0)) return;
    const now = performance.now();
    const current: DenseView = this.dense[pipelineId] ?? { mesh: null, depth: null, timing: null, depthAt: 0, meshAt: 0 };
    let next: DenseView = current;
    if (port === "dense_mesh") {
      const mesh = parseDenseMesh(raw);
      if (mesh) next = { ...current, mesh, meshAt: now };
    } else if (port === "dense_depth") {
      const depth = parseDenseDepth(raw);
      if (depth) next = { ...current, depth, depthAt: now };
    } else {
      next = { ...current, timing: parseDenseTiming(raw) };
    }
    if (next !== current) this.dense = { ...this.dense, [pipelineId]: next };
  }

  /** A `pose_solutions` (tag poses) or `multi_tag_pose` (the camera in the field) event: that output alone changes. */
  private applyPose(pipelineId: string, port: string, data: { change: string; value?: W.PipelineOutput }) {
    const pipeline = this.pipelines.get(pipelineId);
    if (data.change === "removed" || !pipeline || pipeline.state !== "running") {
      if (pipelineId in this.poses) {
        const { [pipelineId]: _, ...rest } = this.poses;
        this.poses = rest;
      }
      return;
    }
    const raw = data.value?.value;
    if (raw === null || raw === undefined) return;
    if (this.seen(`${pipelineId}/${port}`, data.value?.observed_at_ms ?? 0)) return;
    const current: PoseView = this.poses[pipelineId] ?? { tags: null, field: null };
    const next: PoseView = port === "pose_solutions" ? { ...current, tags: parseTagPoses(raw) } : { ...current, field: parseFieldPose(raw) };
    this.poses = { ...this.poses, [pipelineId]: next };
  }

  /** A pipeline's dense mapper outputs (null before its first one). */
  denseFor(pipelineId: string): DenseView | null {
    return this.dense[pipelineId] ?? null;
  }

  /** A pipeline's tag and field poses (null before its first output). */
  posesFor(pipelineId: string): PoseView | null {
    return this.poses[pipelineId] ?? null;
  }

  /** The running pipeline feeding a camera that has `wanted` (the first one that does), with its id. */
  private pipelineWith(cameraId: string, wanted: (pipelineId: string) => boolean): string | undefined {
    for (const p of this.pipelines.values()) {
      if (p.state !== "running" || this.cameraOfPipeline(p) !== cameraId) continue;
      if (wanted(p.id)) return p.id;
    }
    return undefined;
  }

  /** The dense outputs of the pipeline a camera feeds. */
  denseOfCamera(cameraId: string): DenseView | null {
    const id = this.pipelineWith(cameraId, (p) => p in this.dense);
    return id ? this.dense[id] : null;
  }

  /** The tag and field poses of the pipeline a camera feeds. */
  posesOfCamera(cameraId: string): PoseView | null {
    const id = this.pipelineWith(cameraId, (p) => p in this.poses);
    return id ? this.poses[id] : null;
  }

  /**
   * A camera's intrinsics from its calibration, or null when it has none. Read from the device the
   * first time and again after INTRINSICS_TTL_MS; the value arrives reactively when it is read.
   */
  intrinsicsFor(cameraId: string): Intrinsics | null {
    const at = this.intrinsicsAt.get(cameraId);
    const now = performance.now();
    if (at === undefined || now - at > INTRINSICS_TTL_MS) {
      this.intrinsicsAt.set(cameraId, now);
      void this.loadIntrinsics(cameraId);
    }
    return this.intrinsics[cameraId] ?? null;
  }

  private async loadIntrinsics(cameraId: string) {
    try {
      const { calibrations } = await api.calibrations(cameraId);
      const k = calibrations.find((c) => c.fx > 0 && c.fy > 0 && c.width > 0 && c.height > 0);
      this.intrinsics = { ...this.intrinsics, [cameraId]: k ? { fx: k.fx, fy: k.fy, cx: k.cx, cy: k.cy, width: k.width, height: k.height, model: k.model, distortion: k.distortion } : null };
    } catch {
      // The device has no answer: keep what was read before.
    }
  }

  /** The p50 frame and map milliseconds of a live-map pipeline over the last second. */
  mapTimingP50(pipelineId: string): { frameMs: number; mapMs: number } | null {
    const p50 = this.mapTimings.get(pipelineId)?.p50(performance.now());
    return p50 ? { frameMs: p50.frameMs, mapMs: p50.mapMs } : null;
  }

  /** The camera resource feeding a pipeline. */
  private cameraOfPipeline(p: W.Pipeline): string | undefined {
    return Object.values(p.bindings).find((b) => this.data.cameras.some((c) => c.resourceId === b.resource_id))?.resource_id;
  }

  /** An `output` event: a pipeline published a new value of a host output. */
  /** Cameras whose preview arrives with its overlays frame by frame (refcounted by viewer). */
  private frameOverlays = new Map<string, number>();

  /** A camera view now draws its overlays with each preview frame; returns the stop. */
  beginFrameOverlay(cameraId: string): () => void {
    this.frameOverlays.set(cameraId, (this.frameOverlays.get(cameraId) ?? 0) + 1);
    return () => {
      const left = (this.frameOverlays.get(cameraId) ?? 1) - 1;
      if (left > 0) this.frameOverlays.set(cameraId, left);
      else this.frameOverlays.delete(cameraId);
    };
  }

  /** Draw camera `cameraId`'s overlays as they were at the frame captured at `frameTs` (ns). */
  applyFrameOverlay(cameraId: string, frameTs: number) {
    for (const pipeline of this.pipelines.values()) {
      if (this.cameraOfPipeline(pipeline) !== cameraId) continue;
      for (const port of ["detections", "tracks"]) {
        const output = this.liveOutputs.at(`${pipeline.id}/${port}`, frameTs);
        if (output) this.applyOutput({ id: output.id, change: "updated", value: output.value }, true, true);
      }
    }
  }

  private applyOutput(data: { id: string; change: string; value?: W.PipelineOutput }, live = false, frame = false) {
    this.lastOutputEvent = performance.now();
    const [pipelineId, port] = data.id.split("/");
    // With the live stream connected, the event stream's (older, slower) copies are ignored;
    // removals still come from there.
    if (!live && this.liveOutputs.connected && data.change !== "removed") return;
    // A camera view drawing its overlays frame by frame takes them from its frames only.
    if (!frame && (port === "tracks" || port === "detections") && data.change !== "removed") {
      const pipeline = this.pipelines.get(pipelineId);
      const camera = pipeline && this.cameraOfPipeline(pipeline);
      if (camera && this.frameOverlays.has(camera)) return;
    }
    if (port === "tracks") return this.applyTracks(pipelineId, data);
    if (MAP_PORTS.has(port)) return this.applyMap(pipelineId, port, data);
    if (DENSE_PORTS.has(port)) return this.applyDense(pipelineId, port, data);
    if (POSE_PORTS.has(port)) return this.applyPose(pipelineId, port, data);
    if (port !== "detections") return;
    const pipeline = this.pipelines.get(pipelineId);
    const camera = pipeline && this.cameraOfPipeline(pipeline);
    if (!camera) return;
    if (data.change === "removed" || !data.value || pipeline.state !== "running") {
      if (camera in this.detections) {
        const { [camera]: _, ...rest } = this.detections;
        this.detections = rest;
      }
      return;
    }
    this.detections = { ...this.detections, [camera]: toDetections(data.value.value) };
  }

  detectionsFor(cameraId: string): Detection[] {
    return this.detections[cameraId] ?? [];
  }

  /** A `tracks` output event: the camera's tracks, or none once its pipeline stops or the output goes. */
  private applyTracks(pipelineId: string, data: { change: string; value?: W.PipelineOutput }) {
    const pipeline = this.pipelines.get(pipelineId);
    if (!pipeline) return;
    const camera = this.cameraOfPipeline(pipeline);
    if (!camera) return;
    const features = data.change === "removed" || !data.value || pipeline.state !== "running" ? null : this.featuresOf(camera, pipeline, data.value.value);
    if (features) this.features = { ...this.features, [camera]: features };
    else {
      this.trailBooks.delete(camera);
      if (camera in this.features) {
        const { [camera]: _, ...rest } = this.features;
        this.features = rest;
      }
    }
  }

  /** A camera's features from its `tracks` value. An unchanged value is the same tick, so its trails are kept as they are. */
  private featuresOf(camera: string, pipeline: W.Pipeline, value: unknown): CameraFeatures | null {
    const signature = JSON.stringify(value);
    const previous = this.features[camera];
    if (previous?.signature === signature) return previous;
    const tracks = toFeatureTracks(value, this.frameOf(pipeline, camera));
    if (!tracks) return null;
    let book = this.trailBooks.get(camera);
    if (!book) {
      book = new TrailBook();
      this.trailBooks.set(camera, book);
    }
    return { tracks, trails: book.advance(tracks.tracks), signature };
  }

  /** The frame a pipeline's tracks are in: its binding's output size, else the camera's own size. */
  private frameOf(pipeline: W.Pipeline, camera: string): { w: number; h: number } | undefined {
    const binding = Object.values(pipeline.bindings).find((b) => b.resource_id === camera);
    if (binding?.output_width && binding.output_height) return { w: binding.output_width, h: binding.output_height };
    const settings = this.data.cameras.find((c) => c.resourceId === camera)?.settings;
    return settings ? { w: settings.width, h: settings.height } : undefined;
  }

  /** The latest feature tracks of a camera's running pipeline (`tracks`), with their trails. */
  featureTracksFor(cameraId: string): CameraFeatures | null {
    return this.features[cameraId] ?? null;
  }

  async refreshResources() {
    const resources = await this.guard("resources", () => api.resources());
    if (resources) this.data.resources = resources.filter((r) => !DERIVED_TYPES.has(r.type)).map((r) => toResource(r, this.nodeId));
  }

  async refreshLogs() {
    const page = await this.guard("logs", () => api.logs({ lines: 300 }));
    if (page) this.data.logs = page.lines.map((l) => toLogLine(l, this.nodeId));
  }

  private appendLog(line: LogLine) {
    this.data.logs.push(line);
    if (this.data.logs.length > LOG_CAP) this.data.logs.splice(0, this.data.logs.length - LOG_CAP);
  }

  /** Coalesce bursts of events into one refresh. */
  private soon(key: string, run: () => Promise<void>, ms = 250) {
    if (this.pending.has(key)) return;
    this.pending.set(
      key,
      window.setTimeout(() => {
        this.pending.delete(key);
        void run();
      }, ms),
    );
  }

  private onEvent(event: W.ApiEvent) {
    switch (event.type) {
      case "nt4":
        this.soon("nt4", () => this.refreshNt4(), 2500);
        break;
      case "output":
        this.applyOutput(event.data as { id: string; change: string; value?: W.PipelineOutput });
        break;
      case "peripheral":
        emitPeripheral(event.data as PeripheralEvent);
        break;
      case "metrics":
        this.snapshot.metrics = event.data as W.Metrics;
        this.applyNode();
        break;
      case "pipeline":
        this.soon("pipelines", () => this.refreshPipelines());
        break;
      case "resource": {
        const type = ((event.data as { value?: { type?: string } }).value?.type ?? "") as string;
        this.soon("resources", () => this.refreshResources());
        if (type === "camera.device") this.soon("cameras", () => this.refreshCameras());
        break;
      }
      case "camera": {
        const data = event.data as W.CameraControlEvent | { id: string; change: string };
        const camera = this.data.cameras.find((c) => c.resourceId === data.id);
        if (data.change === "control" && camera?.controls) applyControlValue(camera, { id: (data as W.CameraControlEvent).control.id, standard: (data as W.CameraControlEvent).control.standard }, (data as W.CameraControlEvent).control.value);
        else this.soon("cameras", () => this.refreshCameras());
        break;
      }
      case "recording":
        recordings.applyStatus(event.data as RecordingStatus);
        break;
      case "recording_settings":
        recordings.applySettings(event.data as RecorderSettings);
        break;
      case "update":
        if ((event.data as W.UpdateStatus).phase !== undefined) {
          this.snapshot.update = event.data as W.UpdateStatus;
          liveState.update = this.snapshot.update;
          this.applyNode();
        } else this.soon("device", () => this.refreshDevice());
        break;
      case "orion":
      case "lagged":
        this.soon("all", () => this.refreshAll(), 500);
        break;
    }
  }

  // --- Actions -----------------------------------------------------------

  /** Run an action; a 501 becomes one "not available" notice per feature. */
  private async act<T>(feature: string, run: () => Promise<T>, success?: (value: T) => string): Promise<T | undefined> {
    try {
      const value = await run();
      if (success) toasts.success(success(value));
      return value;
    } catch (error) {
      if (isNotAvailable(error)) {
        if (!this.warned.has(feature)) {
          this.warned.add(feature);
          toasts.info(`${error.message}. Not available on this device yet.`);
        }
      } else toasts.error(`${feature}: ${errorText(error)}`);
      return undefined;
    }
  }

  private name(id: string) {
    return this.data.workloads.find((w) => w.id === id)?.name ?? id;
  }

  private specFor(id: string, graph: GraphDocument, enabled = true): W.PipelineSpec {
    const current = this.pipelines.get(id);
    return { name: current?.name ?? this.name(id), graph: toDaedalus(graph), bindings: current?.bindings ?? {}, enabled };
  }

  async restartWorkload(id: string) {
    await this.act("Restart", () => api.restartPipeline(id), () => `${this.name(id)} restarted`);
    await this.refreshPipelines();
  }

  async stopWorkload(id: string) {
    await this.act("Stop", () => api.stopPipeline(id), () => `${this.name(id)} stopped`);
    await this.refreshPipelines();
  }

  async saveGraph(id: string, graph: GraphDocument): Promise<boolean> {
    const deployed = await this.act("Deploy", () => api.putPipeline(id, this.specFor(id, graph)), (p) => `Deployed revision ${p.revision ?? "?"} of ${p.name}`);
    await this.refreshPipelines();
    return Boolean(deployed);
  }

  canRollBack(id: string): boolean {
    const p = this.pipelines.get(id);
    return !!p && p.managed && (p.revision ?? 0) > 1;
  }

  async rollBackWorkload(id: string) {
    await this.act("Roll back", () => api.rollbackPipeline(id), (p) => `${p.name} is back on revision ${p.revision ?? "?"}`);
    await this.refreshPipelines();
  }

  async bindInput(id: string, input: string, resourceId: string) {
    const existing = this.pipelines.get(id)?.bindings[input];
    await this.act("Bind input", () => api.bindInput(id, input, { ...(existing ?? {}), resource_id: resourceId }), () => `${this.name(id)}: ${input} bound to ${resourceId}`);
    await this.refreshPipelines();
  }

  async defaultTemplates(): Promise<DefaultTemplate[]> {
    return defaultsFrom(await this.loadTemplates());
  }

  /** Deploy a default template as the camera's one pipeline: in place of its Eidos pipeline (same
   * id, so robot code keeps its NetworkTables path), any other Eidos pipeline on it disabled;
   * `null` disables them all (video only). */
  async applyDefault(cameraId: string, template: DefaultTemplate | null, name: string) {
    const mine = [...this.pipelines.values()].filter((p) => p.graph?.requires.some((r) => r.id === "eidos") && Object.values(p.bindings).some((b) => b.resource_id === cameraId));
    const keep = mine.find((p) => p.enabled) ?? mine[0];
    for (const p of mine) {
      if (p.enabled && (p !== keep || !template)) await this.act("Stop", () => api.putPipeline(p.id, { name: p.name, graph: p.graph!, bindings: p.bindings, enabled: false }));
    }
    if (!template) {
      await this.refreshPipelines();
      toasts.info(`${name}: video only`);
      return;
    }
    const templates = await this.loadTemplates();
    const t = templates?.find((x) => x.id === template.id);
    if (!t) {
      toasts.error(templates ? `This device has no ${template.title} template` : `Cannot read the device's templates: ${liveState.error ?? "no answer"}`);
      return;
    }
    const previous = keep ? Object.values(keep.bindings).find((b) => b.resource_id === cameraId) : undefined;
    const bindings = { [t.camera_input]: { ...(previous ?? {}), resource_id: cameraId, pyramid: t.pyramid ?? 1 } };
    const title = `${template.title} · ${name}`;
    // The camera's tag family stays when the new template decodes the same kind of tags.
    const current = keep ? this.data.workloads.find((w) => w.id === keep.id) : undefined;
    const family = (d: string) => d.split("_")[0];
    const was = String(current?.graph.nodes.find((n) => n.params.dictionary !== undefined)?.params.dictionary ?? "");
    const kept = was && t.dictionary && family(was) === family(t.dictionary) ? was : undefined;
    const spec: W.PipelineSpec = { name: title, graph: t.graph, bindings, enabled: true, dictionary: kept ?? t.dictionary ?? undefined, field_layout: keep?.field_layout ?? undefined };
    if (keep) await this.act("Pipeline", () => api.putPipeline(keep.id, spec), () => `${name} now runs ${template.title}`);
    else await this.act("Create pipeline", () => api.createPipeline(spec), () => `${name} now runs ${template.title}`);
    await this.refreshPipelines();
  }

  /** Change one input's binding: only the given fields, the rest as deployed. */
  async setInput(id: string, input: string, change: Partial<PipelineInput>) {
    const existing = this.pipelines.get(id)?.bindings[input];
    const binding: W.Binding =
      (change.source ?? (existing?.source === "pipeline" ? "pipeline" : "camera")) === "pipeline"
        ? { resource_id: "", source: "pipeline", pipeline: change.pipeline ?? existing?.pipeline, port: change.port ?? existing?.port, max_wait_ms: change.maxWaitMs ?? existing?.max_wait_ms }
        : {
            ...(existing?.source === "pipeline" ? { resource_id: "" } : existing),
            resource_id: change.resourceId ?? existing?.resource_id ?? "",
            ...("outputWidth" in change ? { output_width: change.outputWidth } : {}),
            ...("outputHeight" in change ? { output_height: change.outputHeight } : {}),
            ...("pyramid" in change ? { pyramid: change.pyramid } : {}),
          };
    await this.act("Input", () => api.bindInput(id, input, binding), () => `${this.name(id)}: ${input} updated`);
    await this.refreshPipelines();
  }

  /** Redeploy a pipeline with another detector search (the same graph, bindings and settings). */
  async setSearch(id: string, mode: "tracked" | "full", every: number | null) {
    const p = this.pipelines.get(id);
    if (!p?.graph) return;
    const spec: W.PipelineSpec = { name: p.name, graph: p.graph, bindings: p.bindings, enabled: p.enabled, field_layout: p.field_layout ?? undefined, cpu: p.cpu ?? undefined, search_mode: mode, full_search_every: mode === "tracked" ? (every ?? 8) : undefined };
    await this.act("Search", () => api.putPipeline(id, spec), (r) => (mode === "full" ? `${r.name} searches every frame` : `${r.name} tracks, with a full search every ${every ?? 8} frames`));
    await this.refreshPipelines();
  }

  /** Redeploy a pipeline with another tag family (the same graph, bindings and settings). */
  async setDictionary(id: string, dictionary: string) {
    const p = this.pipelines.get(id);
    if (!p?.graph) return;
    await this.act("Tag family", () => api.putPipeline(id, { name: p.name, graph: p.graph!, bindings: p.bindings, enabled: p.enabled, dictionary }), (r) => `${r.name} now finds ${dictionaryLabel(dictionary)}`);
    await this.refreshPipelines();
  }

  /** Pin a pipeline's graph thread to a core (`null`: the scheduler's choice). The spec is the
   * pipeline's own graph, bindings and settings, so only the core changes. */
  async setCpu(id: string, cpu: number | null) {
    const p = this.pipelines.get(id);
    if (!p?.graph) return;
    const spec: W.PipelineSpec = { name: p.name, graph: p.graph, bindings: p.bindings, enabled: p.enabled, field_layout: p.field_layout ?? undefined, cpu: cpu ?? undefined };
    await this.act("CPU core", () => api.putPipeline(id, spec), (r) => (cpu === null ? `${r.name} is placed by the scheduler` : `${r.name} pinned to core ${cpu}`));
    await this.refreshPipelines();
  }

  async createWorkload(name: string, _nodeId: string, cameraId: string, graph: GraphDocument): Promise<string> {
    const created = await this.act("Create pipeline", () => api.createPipeline({ name, graph: toDaedalus(graph), bindings: { camera: { resource_id: cameraId } } }), (p) => `${p.name} deployed`);
    await this.refreshPipelines();
    return created?.id ?? "";
  }

  /**
   * Change camera controls from the camera pane's fields (and extra fields named like the
   * device's controls). Changes made while dragging are merged and sent at most every
   * CONTROL_FLUSH_MS; resolution, format, pyramid and ROI are per pipeline and are not sent.
   */
  async setCamera(id: string, settings: Partial<CameraSettings> & Record<string, unknown>) {
    const camera = this.data.cameras.find((c) => c.resourceId === id);
    const changes: W.CameraControlChanges = {};
    let perPipeline = false;
    for (const [key, value] of Object.entries(settings)) {
      if (value === undefined) continue;
      // `null` forgets a camera control (it goes to its default); it means nothing for a field
      // that is per pipeline.
      if (value === null) {
        const forgotten = deviceKeyOf(key, camera);
        if (forgotten && !PER_PIPELINE.has(key)) changes[forgotten] = null;
        continue;
      }
      if (PER_PIPELINE.has(key)) {
        perPipeline = true;
        continue;
      }
      if (key === "awb" && typeof value === "string") changes.awb = value === "auto";
      else if (CONTROL_KEYS[key] && (typeof value === "number" || typeof value === "boolean")) changes[CONTROL_KEYS[key]] = value;
      else if (STANDARD_KEYS.has(key) && (typeof value === "number" || typeof value === "boolean" || typeof value === "string")) changes[key] = value;
      else if (camera?.controls?.some((c) => c.name === key) && (typeof value === "number" || typeof value === "boolean" || isRectValue(value))) changes[key] = value;
    }
    if (perPipeline && !this.warned.has("per-pipeline")) {
      this.warned.add("per-pipeline");
      toasts.info("Resolution, format, pyramid and region are set per pipeline (its camera binding), not on the camera.");
    }
    if (Object.keys(changes).length === 0) return;
    // A forgotten control shows the value it takes by default until the camera answers.
    if (camera)
      for (const [key, value] of Object.entries(changes)) {
        const shown = value === null ? (camera.controls?.find((c) => c.standard === key || c.name === key)?.default ?? null) : typeof value === "string" ? null : value;
        applyControlValue(camera, { standard: key, name: key }, shown);
      }
    this.controlChanges.set(id, { ...(this.controlChanges.get(id) ?? {}), ...changes });
    this.soon(`controls:${id}`, () => this.flushControls(id), CONTROL_FLUSH_MS);
  }

  private async flushControls(id: string) {
    const changes = this.controlChanges.get(id);
    this.controlChanges.delete(id);
    if (!changes || Object.keys(changes).length === 0) return;
    const result = await this.act("Camera settings", () => api.setCameraSettings(id, changes));
    const camera = this.data.cameras.find((c) => c.resourceId === id);
    if (!result) {
      if (camera) await this.refreshCameraControls(id);
      return;
    }
    if (camera) applyPersisted(camera, result.persisted);
    for (const applied of result.applied) {
      if (camera) applyControlValue(camera, { id: applied.id }, applied.value);
      // The camera took the value it could: say so, with the value it set.
      const control = camera?.controls?.find((c) => c.id === applied.id);
      if (applied.clamped && control) toasts.info(`${camera?.name ?? id}: ${clampedNote(control, scalarOf(applied.requested), scalarOf(applied.value))}`);
      if (applied.restarted) toasts.info(`${camera?.name ?? id}: the capture restarted to apply ${applied.control}`);
      else if (applied.deferred) toasts.info(`${camera?.name ?? id}: ${applied.control} applies when the camera starts`);
    }
  }

  /** Put every writable control back to its default and forget the values kept across reboots. */
  async resetCamera(id: string) {
    const camera = this.data.cameras.find((c) => c.resourceId === id);
    this.controlChanges.delete(id);
    const result = await this.act("Reset camera", () => api.resetCameraSettings(id), () => `${camera?.name ?? id} controls reset to defaults`);
    if (result) await this.refreshCameraControls(id);
  }

  async reboot(nodeId: string) {
    const ok = await this.act("Reboot", () => api.reboot(), () => "Device is rebooting");
    if (!ok) return;
    const node = this.data.nodes.find((n) => n.id === nodeId);
    if (node) node.health = "offline";
    const started = Date.now();
    const poll = window.setInterval(async () => {
      try {
        await api.health();
        if (Date.now() - started > 15_000) {
          clearInterval(poll);
          toasts.success("Device is back");
          await this.refreshAll();
        }
      } catch {
        // Still rebooting.
      }
      if (Date.now() - started > 300_000) clearInterval(poll);
    }, 3000);
  }

  async restartService(service: string) {
    await this.act("Restart service", () => api.restartService(service), () => `${service} restarted`);
    await this.refreshDevice();
  }

  async switchSlot() {
    await this.act("Switch boot slot", () => api.switchSlot());
  }

  async safeMode() {
    await this.act("Safe mode", () => api.safeMode());
  }

  /** Pick an OS image, upload it and hand it to the updater. */
  async installImage() {
    const file = await pickFile(".img,.xz,.img.xz,application/octet-stream");
    if (!file) return;
    toasts.info(`Uploading ${file.name} (${(file.size / 1024 / 1024).toFixed(0)} MiB)`);
    const upload = await this.act("Upload", () => api.uploadImage(file));
    if (!upload) return;
    await this.act("Apply update", () => api.applyUpdate({ upload_id: upload.id, sha256: upload.sha256 }), (r) => r.message);
    await this.refreshDevice();
  }


  /** Graph of a pipeline as the device has it (for screens that reload). */
  graphOf(id: string): GraphDocument {
    return fromDaedalus(this.pipelines.get(id)?.graph ?? null);
  }
}
