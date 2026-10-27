// The cluster: everything the screens read, plus the actions they take. Live
// by default: the device's helios-api through LiveCluster (live.svelte.ts).
// With `?mock=1` (or VITE_HELIOS_MOCK=1) it runs the simulated robot from
// mock.ts instead, for UI work without a device.

import { tableFor } from "#lib/graph/graph.js";
import { dictionaryLabel } from "#lib/format.js";
import { applyControlValue } from "#lib/api/adapt.js";
import { clamp, clampedNote, rangeOr } from "#lib/camera/controls.js";
import { CAMERAS, GUEST_NODES, LOGS, MOCK_INTRINSICS, NODES, RESOURCES, ROBOT_CAMERAS, ROBOT_LOGS, ROBOT_NODES, ROBOT_RESOURCES, ROBOT_STREAMS, ROBOT_WORKLOADS, STREAMS, WORKLOADS, defaultGraph, mockCameraFeatures, mockDenseView, mockMapView, mockPoseView } from "#lib/api/mock.js";
import { api, errorText, scoped, type HeliosApi } from "#lib/api/client.js";
import { toDaedalus } from "#lib/api/adapt.js";
import { maps, type MapHost } from "./maps.svelte";
import { deviceId, entityKey } from "#lib/api/peers.js";
import { pipelineDevice, type PipelineDevice } from "#lib/pipeline/device.js";
import type { CameraControlChanges, MapEntry } from "#lib/api/types.js";
import type { DenseView } from "#lib/vision/dense.js";
import type { Intrinsics, PoseView } from "#lib/vision/pose.js";
import { BUILT_IN, type DefaultTemplate } from "#lib/core/defaults.js";
import type { CameraFeatures } from "#lib/vision/features.js";
import type { MapView } from "#lib/vision/map.js";
import { MOCK, MOCK_DEVICE } from "#lib/api/mode.js";
import { LiveCluster } from "./live.svelte";
import type { Camera, CameraSettings, ClusterNode, ControlWrite, Detection, GraphDocument, LogLine, PipelineInput, Resource, Stream, TagPose, Workload } from "#lib/api/model.js";
import { toasts } from "./toasts.svelte";
import { blockingStage } from "#lib/status/chain.js";

interface FeedData {
  width: number;
  height: number;
  frames: { source_frame: number; markers: Detection[] }[];
}
interface PoseData {
  frames: TagPose[][];
}

const FEED_FRAMES = 480;
const FEED_FPS = 16;
/** The node type of Eidos's live mapper, as a workload's graph names it. */
const LIVE_MAP_NODE = "eidos:mapping.live_map";

/**
 * Mock only: a control write as the device takes it (clamped to the range it takes now; read-only
 * ones refuse; `null` goes back to the default and is not kept). Returns the note for a clamped write.
 */
function mockControl(c: Camera, key: string, value: ControlWrite): string | null {
  const control = c.controls?.find((x) => x.standard === key || x.name === key);
  if (!control || control.readOnly || !control.writable || typeof value === "string") return null;
  if (value === null) {
    applyControlValue(c, { id: control.id }, control.default);
    control.persisted = false;
    return null;
  }
  if (typeof value !== "number") {
    applyControlValue(c, { id: control.id }, value);
    control.persisted = true;
    return null;
  }
  const range = rangeOr(control, { min: -Infinity, max: Infinity });
  const clamped = clamp(value, range.min, range.max);
  applyControlValue(c, { id: control.id }, clamped);
  control.persisted = true;
  // As the device does: setting a metering mode clears the metering region.
  if (control.name === "ae_metering_mode") {
    const region = c.controls?.find((x) => x.name === "ae_metering_region");
    if (region) region.rect = { x: 0, y: 0, width: 0, height: 0 };
  }
  return clamped !== value ? clampedNote(control, value, clamped) : null;
}

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value));
}

function jitter(value: number, spread: number, min = 0): number {
  return Math.max(min, value + (Math.random() - 0.5) * spread);
}

function push(history: number[], value: number, cap = 60) {
  history.push(value);
  if (history.length > cap) history.shift();
}

class ClusterStore {
  /** True while running the simulated robot (`?mock=1`). */
  readonly mock = MOCK;
  /** The simulated Raze cluster; a simulated SystemCore is one guest node with nothing else on it. */
  private readonly razeMock = MOCK && MOCK_DEVICE === "raze";
  /** One robot with two HeliOS devices: the Raze (this device) and a routed SystemCore peer. */
  private readonly robotMock = MOCK && MOCK_DEVICE === "robot";
  nodes = $state<ClusterNode[]>(MOCK ? clone(this.razeMock ? NODES : this.robotMock ? ROBOT_NODES : GUEST_NODES) : []);
  cameras = $state<Camera[]>(this.razeMock ? clone(CAMERAS) : this.robotMock ? clone(ROBOT_CAMERAS) : []);
  workloads = $state<Workload[]>(this.razeMock ? clone(WORKLOADS) : this.robotMock ? clone(ROBOT_WORKLOADS) : []);
  streams = $state<Stream[]>(this.razeMock ? clone(STREAMS) : this.robotMock ? clone(ROBOT_STREAMS) : []);
  resources = $state<Resource[]>(this.razeMock ? clone(RESOURCES) : this.robotMock ? clone(ROBOT_RESOURCES) : []);
  logs = $state<LogLine[]>(this.razeMock ? clone(LOGS) : this.robotMock ? clone(ROBOT_LOGS) : []);
  /** Advances FEED_FPS times a second; every camera view follows it. */
  frame = $state(0);
  playing = $state(true);
  feed = $state<FeedData | null>(null);
  poses = $state<PoseData | null>(null);
  private started = false;
  /** The device backend; null in mock mode. */
  readonly live: LiveCluster | null = MOCK ? null : new LiveCluster(this);
  /** Earlier deployed revisions per workload, newest last (the device keeps these for rollback). */
  private revisions = new Map<string, { revision: number; graph: GraphDocument }[]>(
    WORKLOADS.filter((w) => w.revision > 1).map((w) => [w.id, [{ revision: w.revision - 1, graph: clone(w.graph) }]]),
  );

  /**
   * Resolves once every device write the UI started has returned (and no control change waits to
   * be sent); at once on the mock. For what leaves the page (a test's end): a request cut off
   * mid-way is a change the user never saw land.
   */
  settled(): Promise<void> {
    return this.live ? this.live.settled() : Promise.resolve();
  }

  start() {
    if (this.started || typeof window === "undefined") return () => {};
    this.started = true;
    // `window.__helios.settled()`: the end-to-end suite waits for the UI's writes before it closes a page.
    (window as Window & { __helios?: { settled(): Promise<void> } }).__helios = { settled: () => this.settled() };
    if (this.live) {
      const stop = this.live.start();
      return () => {
        stop();
        this.started = false;
      };
    }
    fetch("/mock/robot/detections.json").then((r) => r.json()).then((d) => (this.feed = d)).catch(() => {});
    fetch("/mock/robot/poses.json").then((r) => r.json()).then((d) => (this.poses = d)).catch(() => {});
    const frames = setInterval(() => {
      if (this.playing) this.frame = (this.frame + 1) % FEED_FRAMES;
    }, 1000 / FEED_FPS);
    const readings = setInterval(() => this.tick(), 1000);
    return () => {
      clearInterval(frames);
      clearInterval(readings);
      this.started = false;
    };
  }

  private tick() {
    for (const n of this.nodes) {
      if (n.health === "offline") continue;
      n.uptimeS += 1;
      n.cpu = jitter(n.cpu, n.cpu * 0.08, 0.001);
      push(n.cpuHistory, n.cpu);
      n.tempC = jitter(n.tempC, 0.4, 20);
      push(n.tempHistory, n.tempC);
      n.clock.offsetUs = Math.round(jitter(n.clock.offsetUs, n.clock.offsetUs * 0.1, 1));
    }
    for (const w of this.workloads) {
      if (w.state !== "running") continue;
      w.perf.tickP50Ms = jitter(0.7, 0.03, 0.5);
      w.perf.tickP99Ms = w.perf.tickP50Ms + jitter(0.1, 0.05, 0.02);
      push(w.perf.tickHistory, w.perf.tickP50Ms);
      for (const key of Object.keys(w.perf.nodeMs)) w.perf.nodeMs[key] = jitter(w.perf.nodeMs[key], w.perf.nodeMs[key] * 0.06, 0.001);
    }
    for (const c of this.cameras) {
      c.stats.latencyMs = jitter(c.foreign ? 31 : 9.7, 0.6, 1);
    }
    if (Math.random() < 0.35) this.ambientLog();
  }

  /** Background chatter so logs look like a running robot. */
  private ambientLog() {
    const razes = this.nodes.filter((n) => n.kind === "raze" && n.health !== "offline");
    if (!razes.length) return;
    const n = razes[Math.floor(Math.random() * razes.length)];
    const w = this.workloadsOn(n.id).find((x) => x.state === "running");
    const lines: [string, LogLine["level"], string][] = [
      ["atlas-agent", "info", `health ok · ${n.tempC.toFixed(0)} °C · watchdog fed`],
      ["orion-node", "info", `status lane: ${12 + Math.floor(Math.random() * 6)} subjects updated, 0 persisted writes`],
      ["helios-io", "info", `camera delivered 600 frames, 0 dropped, latency ${(9 + Math.random()).toFixed(1)} ms`],
    ];
    if (w) lines.push(["helios-runner", "info", `${w.name}: p50 ${w.perf.tickP50Ms.toFixed(2)} ms, p99 ${w.perf.tickP99Ms.toFixed(2)} ms`]);
    const [unit, level, text] = lines[Math.floor(Math.random() * lines.length)];
    this.log(n.id, unit, level, text);
    if (this.logs.length > 600) this.logs.splice(0, this.logs.length - 600);
  }

  /** Index into the replayed robot video for a camera, given its feed offset. */
  feedIndex(camera: Camera): number {
    return (this.frame + camera.feed.offset) % FEED_FRAMES;
  }

  detectionsFor(camera: Camera): Detection[] {
    if (this.live) return this.live.detectionsFor(camera.resourceId);
    return this.feed?.frames?.[this.feedIndex(camera)]?.markers ?? [];
  }

  /** Feature tracks of a camera with a running `tracks` pipeline (null without one). */
  featureTracksFor(camera: Camera): CameraFeatures | null {
    if (this.live) return this.live.featureTracksFor(camera.resourceId);
    const tracking = this.workloads.some((w) => w.state === "running" && w.outputs.some((o) => o.endsWith(".tracks")) && Object.values(w.bindings).includes(camera.resourceId));
    return tracking ? mockCameraFeatures(this.feedIndex(camera)) : null;
  }

  /** Whether a workload runs Eidos's live mapper (`eidos:mapping.live_map`). */
  isLiveMap(w: Workload): boolean {
    return w.graph.nodes.some((n) => n.type === LIVE_MAP_NODE);
  }

  /** A live-map workload's latest pose, snapshot and timing (null before its first output). */
  mapFor(w: Workload): MapView | null {
    if (this.live) return this.live.mapFor(w.id);
    return this.isLiveMap(w) && w.state === "running" ? mockMapView(this.frame) : null;
  }

  /** The p50 frame and map milliseconds of a live-map workload over the last second. */
  mapTimingP50(w: Workload): { frameMs: number; mapMs: number } | null {
    if (this.live) return this.live.mapTimingP50(w.id);
    return this.isLiveMap(w) && w.state === "running" ? { frameMs: 3.8 + (this.frame % 7) * 0.05, mapMs: 1.6 + (this.frame % 5) * 0.04 } : null;
  }

  /** The running live-map workload that a camera feeds (the mock robot has one). */
  private mapOfCamera(camera: Camera): Workload | undefined {
    return this.workloads.find((w) => w.state === "running" && this.isLiveMap(w) && Object.values(w.bindings).includes(camera.resourceId));
  }

  /** A live-map workload's dense outputs: the mesh, the depth and the timing (null before the first). */
  denseFor(w: Workload): DenseView | null {
    if (this.live) return this.live.denseFor(w.id);
    return this.isLiveMap(w) && w.state === "running" ? mockDenseView(this.frame) : null;
  }

  /** The dense outputs of the workload a camera feeds. */
  denseForCamera(camera: Camera): DenseView | null {
    if (this.live) return this.live.denseOfCamera(camera.resourceId);
    const w = this.mapOfCamera(camera);
    return w ? this.denseFor(w) : null;
  }

  /** A workload's tag poses (`pose_solutions`) and camera in the field (`multi_tag_pose`). */
  poseViewFor(w: Workload): PoseView | null {
    if (this.live) return this.live.posesFor(w.id);
    return this.isLiveMap(w) && w.state === "running" ? mockPoseView(this.frame) : null;
  }

  /** The tag and field poses of the workload a camera feeds. */
  poseViewForCamera(camera: Camera): PoseView | null {
    if (this.live) return this.live.posesOfCamera(camera.resourceId);
    const w = this.mapOfCamera(camera);
    return w ? this.poseViewFor(w) : null;
  }

  /** A camera's intrinsics from its calibration (null without one, or until it is read). */
  intrinsicsFor(camera: Camera): Intrinsics | null {
    if (this.live) return this.live.intrinsicsFor(camera.resourceId);
    return MOCK_INTRINSICS;
  }

  posesFor(camera: Camera): TagPose[] {
    return this.poses?.frames?.[this.feedIndex(camera)] ?? [];
  }

  node(id: string) {
    return this.nodes.find((n) => n.id === id);
  }
  /** This device: the gateway the robot's peers are reached through (the node that is not a peer). */
  get gateway() {
    return this.nodes.find((n) => !n.peer);
  }
  camera(id: string) {
    return this.cameras.find((c) => c.resourceId === id);
  }
  /** The input of `w` bound to a camera (the device's graphs call it `frame`), if any. */
  cameraInput(w: Workload): string | undefined {
    return Object.entries(w.bindings).find(([, id]) => this.cameras.some((c) => c.resourceId === id))?.[0];
  }
  /** The camera `w` reads, if any. */
  cameraOf(w: Workload): Camera | undefined {
    const input = this.cameraInput(w);
    return input ? this.camera(w.bindings[input]) : undefined;
  }
  /**
   * Why a quarantined pipeline stopped, from what it reads: its camera's first blocking stage when
   * the camera is there and blocked, the camera's absence when it is not connected, else the
   * pipeline's own status. `restart` says whether restarting it can help.
   */
  stopOf(w: Workload): { reason: string; hint: string | null; stage: string | null; restart: boolean } {
    const stop = blockingStage(this.cameraOf(w)?.status);
    if (stop) return { reason: `${stop.label}: ${stop.reason}`, hint: stop.hint, stage: stop.stage, restart: false };
    const absent = Object.values(w.bindings).find((id) => !this.camera(id));
    if (absent) return { reason: `camera not connected (${absent} is not on the device)`, hint: "Plug it in, or stop the pipeline.", stage: null, restart: false };
    return { reason: w.status?.reason ?? "stopped", hint: w.status?.hint ?? null, stage: null, restart: true };
  }
  workload(id: string) {
    return this.workloads.find((w) => w.id === id);
  }
  workloadsOn(nodeId: string) {
    return this.workloads.filter((w) => w.nodeId === nodeId);
  }

  get attention(): { tone: "error" | "warning"; text: string; href: string }[] {
    const items: { tone: "error" | "warning"; text: string; href: string }[] = [];
    for (const w of this.workloads.filter((x) => x.state === "quarantined")) {
      items.push({ tone: "error", text: `${w.name} is stopped: ${this.stopOf(w).reason}`, href: `/pipelines/${encodeURIComponent(w.id)}` });
    }
    for (const c of this.cameras) {
      const stop = blockingStage(c.status);
      if (stop?.state === "blocked") items.push({ tone: "error", text: `${c.name}: ${stop.reason}`, href: `/cameras/${encodeURIComponent(c.resourceId)}` });
    }
    for (const n of this.nodes.filter((x) => x.health !== "online" && x.peer?.routed !== false)) {
      items.push({ tone: n.health === "offline" ? "error" : "warning", text: `${n.name} is ${n.health}`, href: `/devices/${encodeURIComponent(n.id)}` });
    }
    for (const n of this.nodes.filter((x) => x.clock.offsetUs > 1000 && x.health !== "offline")) {
      items.push({ tone: "warning", text: `${n.name} clock is ${(n.clock.offsetUs / 1000).toFixed(1)} ms off`, href: `/devices/${encodeURIComponent(n.id)}` });
    }
    return items;
  }

  private log(nodeId: string, unit: string, level: LogLine["level"], text: string) {
    this.logs.push({ ts: Date.now(), nodeId, unit, level, text });
  }

  private async later(ms = 700) {
    await new Promise((r) => setTimeout(r, ms));
  }

  /**
   * Mock only: sends a write for a routed peer through the peer's scope, as the live store does, so
   * the simulated peer keeps the request (`mock-device.ts`). Nothing is sent for this device.
   */
  private toPeer(nodeId: string | undefined, run: (peer: HeliosApi) => Promise<unknown>) {
    if (!this.mock || !nodeId || this.node(nodeId)?.peer?.routed !== true) return;
    run(scoped(nodeId)).catch(() => undefined);
  }

  /**
   * The pipeline a camera feeds: on the camera's own node, the running one that binds it (else the
   * first that does). Keys are per node, so a peer's camera is only read by its node's pipelines.
   */
  pipelineOf(cameraKey: string): string | undefined {
    const camera = this.camera(cameraKey);
    if (!camera) return undefined;
    const reading = this.workloads.filter((w) => w.nodeId === camera.nodeId && Object.values(w.bindings).includes(cameraKey));
    return (reading.find((w) => w.state === "running") ?? reading[0])?.id;
  }

  // --- Actions -----------------------------------------------------------

  async restartWorkload(id: string) {
    if (this.live) return this.live.restartWorkload(id);
    const w = this.workload(id);
    if (!w) return;
    this.toPeer(w.nodeId, (peer) => peer.restartPipeline(deviceId(w.nodeId, id)));
    w.state = "starting";
    await this.later();
    w.state = "running";
    w.restarts = 0;
    w.perf = { ...w.perf, fps: 60, memMiB: 8.9, tickP50Ms: 0.7, tickP99Ms: 0.8, nodeMs: Object.keys(w.perf.nodeMs).length ? w.perf.nodeMs : { mask: 0.3, quads: 0.29, decode: 0.04 } };
    this.log(w.nodeId, "helios-runner", "info", `${w.name} restarted (revision ${w.revision})`);
    toasts.success(`${w.name} restarted`);
  }

  async stopWorkload(id: string) {
    if (this.live) return this.live.stopWorkload(id);
    const w = this.workload(id);
    if (!w) return;
    this.toPeer(w.nodeId, (peer) => peer.stopPipeline(deviceId(w.nodeId, id)));
    await this.later(300);
    w.state = "stopped";
    this.log(w.nodeId, "helios-runner", "info", `${w.name} stopped`);
    toasts.info(`${w.name} stopped`);
  }

  /** Deploy `graph` as the pipeline's next revision; true when the device took it. */
  async saveGraph(id: string, graph: GraphDocument): Promise<boolean> {
    if (this.live) return this.live.saveGraph(id, graph);
    const w = this.workload(id);
    if (!w) return false;
    await this.later(500);
    this.revisions.set(id, [...(this.revisions.get(id) ?? []), { revision: w.revision, graph: clone(w.graph) }]);
    w.graph = clone(graph);
    w.revision += 1;
    w.state = "running";
    this.log(w.nodeId, "helios-runner", "info", `${w.name} revision ${w.revision} deployed; previous revision kept for rollback`);
    toasts.success(`Deployed revision ${w.revision} to ${this.node(w.nodeId)?.name}`);
    return true;
  }

  /** Whether an earlier revision is available to roll back to. */
  canRollBack(id: string): boolean {
    if (this.live) return this.live.canRollBack(id);
    return (this.revisions.get(id)?.length ?? 0) > 0;
  }

  async rollBackWorkload(id: string) {
    if (this.live) return this.live.rollBackWorkload(id);
    const w = this.workload(id);
    const previous = this.revisions.get(id)?.pop();
    if (!w || !previous) return;
    await this.later(500);
    w.graph = clone(previous.graph);
    w.revision = previous.revision;
    w.state = "running";
    w.restarts = 0;
    w.perf = { ...w.perf, fps: 60, memMiB: 8.9, tickP50Ms: 0.7, tickP99Ms: 0.8, nodeMs: Object.fromEntries(previous.graph.nodes.map((n) => [n.id, 0.05])) };
    this.log(w.nodeId, "helios-runner", "info", `${w.name} rolled back to revision ${w.revision}`);
    toasts.success(`${w.name} is back on revision ${w.revision}`);
  }

  async bindInput(id: string, input: string, resourceId: string) {
    if (this.live) return this.live.bindInput(id, input, resourceId);
    const w = this.workload(id);
    if (!w) return;
    w.bindings = { ...w.bindings, [input]: resourceId };
    this.log(w.nodeId, "helios-runner", "info", `${w.name}: ${input} bound to ${resourceId}`);
  }

  /** The default pipelines this device offers a camera (`core/defaults.ts`). */
  async defaultTemplates(): Promise<DefaultTemplate[]> {
    if (this.live) return this.live.defaultTemplates();
    return BUILT_IN;
  }

  /**
   * Make `template` (a default template's id) what `camera` runs, or nothing but video (`null`).
   * A camera runs one pipeline: its running one takes the new graph in place (same id, so its
   * NetworkTables path stays), the others on the camera stop.
   */
  async applyDefault(cameraId: string, template: DefaultTemplate | null, name: string) {
    if (this.live) return this.live.applyDefault(cameraId, template, name);
    const camera = this.camera(cameraId);
    const mine = this.workloads.filter((w) => Object.values(w.bindings).includes(cameraId));
    const keep = mine.find((w) => w.state === "running" || w.state === "starting") ?? mine[0];
    for (const w of mine) if (w !== keep || !template) w.state = "stopped";
    if (!template || !camera) {
      toasts.info(`${name}: video only`);
      return;
    }
    const graph = defaultGraph(template.id, tableFor(camera.nodeId));
    if (!keep) {
      await this.createWorkload(`${template.title} · ${name}`, camera.nodeId, cameraId, graph);
      return;
    }
    keep.graph = graph;
    keep.name = `${template.title} · ${name}`;
    keep.revision += 1;
    keep.state = "running";
    keep.inputs = { camera: { ...keep.inputs.camera, source: "camera", resourceId: cameraId, pyramid: template.capabilities.includes("map") || template.capabilities.includes("features") ? 3 : 1 } };
    keep.searchMode = template.capabilities.includes("tags") ? (keep.searchMode ?? "tracked") : null;
    keep.perf = { ...keep.perf, nodeMs: Object.fromEntries(graph.nodes.map((n) => [n.id, 0.05])) };
    toasts.success(`${name} now runs ${template.title}`);
  }

  /** Change what a pipeline asks of one input (its camera, output size, pyramid; or the producer
   * and wait of a pipeline input). Only the fields given change. */
  async setInput(id: string, input: string, change: Partial<PipelineInput>) {
    if (this.live) return this.live.setInput(id, input, change);
    const w = this.workload(id);
    if (!w) return;
    const next = { ...(w.inputs[input] ?? { source: "camera" as const }), ...change };
    w.inputs = { ...w.inputs, [input]: next };
    if (next.resourceId) w.bindings = { ...w.bindings, [input]: next.resourceId };
    this.log(w.nodeId, "helios-runner", "info", `${w.name}: ${input} updated`);
  }

  /** The detector's search: `tracked` with a full search every `every` frames, or `full`. */
  async setSearch(id: string, mode: "tracked" | "full", every: number | null) {
    if (this.live) return this.live.setSearch(id, mode, every);
    const w = this.workload(id);
    if (!w) return;
    w.searchMode = mode;
    w.fullSearchEvery = mode === "tracked" ? (every ?? 8) : null;
  }

  /** Change the tag family a pipeline decodes. */
  async setDictionary(id: string, dictionary: string) {
    if (this.live) return this.live.setDictionary(id, dictionary);
    const w = this.workload(id);
    const node = w?.graph.nodes.find((n) => n.params.dictionary !== undefined);
    if (!w || !node) return;
    node.params.dictionary = dictionary;
    toasts.success(`${w.name} now finds ${dictionaryLabel(dictionary)}`);
  }

  /** The device a node is, as the maps store reads its saved maps: this device, or a peer through the gateway. */
  mapHost(nodeId: string): MapHost {
    const remote = this.node(nodeId)?.peer !== undefined;
    return { id: nodeId, remote, scope: remote ? scoped(nodeId) : api };
  }

  /** Where a pipeline runs, for the calls that read it: its id on its device, and the API that reaches the device. */
  deviceOf(w: Workload): PipelineDevice {
    return pipelineDevice(w.nodeId, w.id, this.node(w.nodeId)?.peer !== undefined);
  }

  /** Saves pipeline `id`'s live map now, as `name` (the mapper gives its map within seconds). */
  async saveMap(id: string, name?: string): Promise<MapEntry | undefined> {
    const w = this.workload(id);
    if (!w) return undefined;
    return maps.save(this.mapHost(w.nodeId), id, deviceId(w.nodeId, id), name);
  }

  /** The saved map pipeline `id` starts from (`null`: none); it is loaded when the pipeline starts or restarts. */
  async setMap(id: string, map: string | null) {
    if (this.live) return this.live.setMap(id, map);
    const w = this.workload(id);
    if (!w) return;
    try {
      // The simulated device keeps the choice as the device does (a PUT of the pipeline's spec).
      // A PUT replaces the spec: the inputs stay as they are, the choice of map changes.
      const bindings = Object.fromEntries(Object.entries(w.bindings).map(([input, resourceId]) => [input, { resource_id: resourceId }]));
      await this.mapHost(w.nodeId).scope.putPipeline(deviceId(w.nodeId, id), { name: w.name, graph: toDaedalus(w.graph), bindings, map: map ?? undefined });
    } catch (error) {
      toasts.error(`Map: ${errorText(error)}`);
      return;
    }
    w.map = map;
    toasts.success(map === null ? `${w.name} starts with no map` : `${w.name} starts from its saved map when it next starts`);
  }

  /** Pin a pipeline's graph thread to a core (`null`: the scheduler's choice). */
  async setCpu(id: string, cpu: number | null) {
    if (this.live) return this.live.setCpu(id, cpu);
    const w = this.workload(id);
    if (!w) return;
    w.cpu = cpu;
    w.cpuApplied = cpu;
    w.cpuError = null;
    toasts.success(cpu === null ? `${w.name} is placed by the scheduler` : `${w.name} pinned to core ${cpu}`);
  }

  async createWorkload(name: string, nodeId: string, cameraId: string, graph: GraphDocument): Promise<string> {
    if (this.live) return this.live.createWorkload(name, nodeId, cameraId, graph);
    await this.later(500);
    // A peer's pipeline is keyed by its node (`entityKey`), as the live store keys it.
    const id = entityKey(this.node(nodeId)?.peer ? nodeId : undefined, `wl-${Math.random().toString(36).slice(2, 7)}`);
    this.workloads.push({
      id,
      name,
      nodeId,
      revision: 1,
      state: "running",
      restarts: 0,
      bindings: { camera: cameraId },
      inputs: { camera: { source: "camera", resourceId: cameraId, pyramid: 1 } },
      searchMode: graph.nodes.some((n) => n.type.startsWith("eidos:detectors.")) ? "tracked" : null,
      fullSearchEvery: 8,
      graph: clone(graph),
      outputs: [`stream.${id}.poses`],
      cpu: null,
      cpuApplied: null,
      cpuError: null,
      perf: { tickP50Ms: 0.7, tickP99Ms: 0.8, fps: 60, memMiB: 8.9, nodeMs: Object.fromEntries(graph.nodes.map((n) => [n.id, 0.05])), tickHistory: [] },
    });
    toasts.success(`${name} deployed to ${this.node(nodeId)?.name}`);
    return id;
  }

  /** Applies camera settings live (controls call this as you drag). */
  setCamera(id: string, settings: Partial<CameraSettings>, extra?: Record<string, ControlWrite>) {
    if (this.live) {
      void this.live.setCamera(id, { ...settings, ...(extra ?? {}) });
      return;
    }
    const c = this.camera(id);
    if (!c) return;
    Object.assign(c.settings, settings);
    if (extra) {
      // Values kept by name; a null forgets one.
      const kept = { ...(c.extra ?? {}) };
      for (const [key, value] of Object.entries(extra)) {
        if (value === null) delete kept[key];
        else if (typeof value === "number" || typeof value === "string" || typeof value === "boolean") kept[key] = value;
      }
      c.extra = kept;
    }
    // The mock's controls take the same keys the device does: standard keys, or names.
    const keys: Record<string, ControlWrite | undefined> = { exposure_us: settings.exposureUs, ae: settings.autoExposure, gain: settings.gain, fps: settings.fps, ...extra };
    const sent: Record<string, ControlWrite> = {};
    for (const [key, value] of Object.entries(keys)) {
      if (value === undefined) continue;
      const note = mockControl(c, key, value);
      if (note) toasts.info(`${c.name}: ${note}`);
      if (value !== null && typeof value !== "object") sent[key] = value;
    }
    c.stats.fps = c.settings.fps;
    // A peer's camera takes the same change on the peer (the mock peer keeps the request).
    this.toPeer(c.nodeId, (peer) => peer.setCameraSettings(deviceId(c.nodeId, id), sent as CameraControlChanges));
  }

  async updateCamera(id: string, settings: Partial<CameraSettings>) {
    if (this.live) return this.live.setCamera(id, settings);
    const c = this.camera(id);
    if (!c) return;
    await this.later(250);
    this.setCamera(id, settings);
    toasts.success(`${c.name} camera updated`);
  }

  async reboot(nodeId: string) {
    if (this.live) return this.live.reboot(nodeId);
    const n = this.node(nodeId);
    if (!n) return;
    n.health = "offline";
    this.log(nodeId, "atlas-agent", "warn", "reboot requested");
    toasts.info(`${n.name} is rebooting`);
    await this.later(3500);
    n.health = "online";
    n.uptimeS = 0;
    this.log(nodeId, "atlas-agent", "info", "boot ok · slot confirmed by health check");
    toasts.success(`${n.name} is back`);
  }

  async restartService(nodeId: string, service: string) {
    if (this.live) return this.live.restartService(nodeId, service);
    const s = this.node(nodeId)?.services.find((x) => x.name === service);
    if (!s) return;
    s.state = "restarting";
    await this.later();
    s.state = "running";
    this.log(nodeId, service, "info", "restarted by request");
    toasts.success(`${service} restarted`);
  }

  async switchSlot(nodeId: string) {
    if (this.live) return this.live.switchSlot(nodeId);
    const n = this.node(nodeId);
    if (!n || n.slots.length < 2) return;
    const target = n.slots.find((s) => !s.active)!;
    toasts.info(`${n.name}: trial boot into slot ${target.name}`);
    n.health = "offline";
    await this.later(4000);
    for (const s of n.slots) s.active = s === target;
    target.confirmed = true;
    n.os.version = target.version;
    n.health = "online";
    n.uptimeS = 0;
    this.log(nodeId, "atlas-agent", "info", `slot ${target.name} (${target.version}) booted and confirmed`);
    toasts.success(`${n.name} is running ${target.version} from slot ${target.name}`);
  }

  async installUpdate(nodeId: string, version: string) {
    if (this.live) return this.live.installImage();
    const n = this.node(nodeId);
    if (!n || n.slots.length < 2) return;
    const target = n.slots.find((s) => !s.active)!;
    toasts.info(`Writing ${version} to slot ${target.name} on ${n.name}`);
    await this.later(2500);
    target.version = version;
    target.confirmed = false;
    await this.switchSlot(nodeId);
  }

  async safeMode(nodeId: string) {
    if (this.live) return this.live.safeMode(nodeId);
    const n = this.node(nodeId);
    if (!n) return;
    await this.later(500);
    for (const s of n.services) if (s.name !== "atlas-agent" && s.name !== "orion-node") s.state = "stopped";
    n.health = "degraded";
    this.log(nodeId, "atlas-agent", "warn", "safe mode: application services stopped; agent and control plane only");
    toasts.warning(`${n.name} is in safe mode`);
  }
}

export const cluster = new ClusterStore();
