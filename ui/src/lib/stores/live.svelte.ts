// The live side of the cluster store: loads the device from helios-api, keeps
// it current from the event stream (with a slow poll as a safety net) and
// turns the screens' actions into API calls. Features the device has no
// backend for yet (501) are reported once, as "not available".

import { applyCameraSettings, applyControlValue, fromDaedalus, toCamera, toDaedalus, toLogLine, toNode, toResource, toStreams, toWorkload, type DeviceSnapshot } from "$lib/api/adapt";
import { api, ApiError, errorText, isNotAvailable } from "$lib/api/client";
import type { Camera, CameraSettings, ClusterNode, GraphDocument, LogLine, Resource, Stream, Workload } from "$lib/api/model";
import type * as W from "$lib/api/types";
import { pickFile } from "$lib/files";
import { toasts } from "./toasts.svelte";

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
const LOG_CAP = 600;

class LiveState {
  connected = $state(false);
  error = $state<string | null>(null);
  update = $state<W.UpdateStatus | null>(null);
  identity = $state<W.Identity | null>(null);
  plugins = $state<string[]>([]);
  /** Camera mounts stored on the device (robot frame, metres, degrees). */
  mounts = $state<Record<string, W.CameraMount>>({});
}

export const liveState = new LiveState();

export class LiveCluster {
  private pipelines = new Map<string, W.Pipeline>();
  private snapshot: Partial<DeviceSnapshot> = {};
  private timers: number[] = [];
  private unsubscribers: (() => void)[] = [];
  private pending = new Map<string, number>();
  private warned = new Set<string>();
  private controlChanges = new Map<string, W.CameraControlChanges>();

  constructor(private data: ClusterData) {}

  get nodeId(): string {
    return this.snapshot.device?.node_id ?? "device";
  }

  start(): () => void {
    void this.refreshAll();
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
    await Promise.all([this.refreshCameras(), this.refreshPipelines(), this.refreshResources(), this.refreshLogs()]);
  }

  async refreshDevice() {
    const device = await this.guard("device", () => api.device());
    if (!device) return;
    this.snapshot.device = device;
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
    if (plugins) liveState.plugins = plugins.plugins;
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

  async saveGraph(id: string, graph: GraphDocument) {
    await this.act("Deploy", () => api.putPipeline(id, this.specFor(id, graph)), (p) => `Deployed revision ${p.revision ?? "?"} of ${p.name}`);
    await this.refreshPipelines();
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
      if (value === undefined || value === null) continue;
      if (PER_PIPELINE.has(key)) {
        perPipeline = true;
        continue;
      }
      if (key === "awb" && typeof value === "string") changes.awb = value === "auto";
      else if (CONTROL_KEYS[key] && (typeof value === "number" || typeof value === "boolean")) changes[CONTROL_KEYS[key]] = value;
      else if (STANDARD_KEYS.has(key) && (typeof value === "number" || typeof value === "boolean" || typeof value === "string")) changes[key] = value;
      else if (camera?.controls?.some((c) => c.name === key) && (typeof value === "number" || typeof value === "boolean")) changes[key] = value;
    }
    if (perPipeline && !this.warned.has("per-pipeline")) {
      this.warned.add("per-pipeline");
      toasts.info("Resolution, format, pyramid and region are set per pipeline (its camera binding), not on the camera.");
    }
    if (Object.keys(changes).length === 0) return;
    if (camera) for (const [key, value] of Object.entries(changes)) applyControlValue(camera, { standard: key, name: key }, typeof value === "string" ? null : value);
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
    for (const applied of result.applied) {
      if (camera) applyControlValue(camera, { id: applied.id }, applied.value);
      if (applied.restarted) toasts.info(`${camera?.name ?? id}: the capture restarted to apply ${applied.control}`);
      else if (applied.deferred) toasts.info(`${camera?.name ?? id}: ${applied.control} applies when the camera starts`);
    }
  }

  /** Put every writable control back to its default. */
  async resetCamera(id: string) {
    const camera = this.data.cameras.find((c) => c.resourceId === id);
    if (!camera?.controls) return;
    const changes: W.CameraControlChanges = {};
    for (const c of camera.controls) {
      if (!c.writable || c.default === null || c.standard === "af_trigger" || !["bool", "int", "uint", "float", "menu", "int_menu"].includes(c.kind)) continue;
      changes[c.standard ?? c.name] = c.default;
    }
    if (Object.keys(changes).length === 0) return;
    const result = await this.act("Reset camera", () => api.setCameraSettings(id, changes), () => `${camera.name} controls reset to defaults`);
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

  async calibrate(cameraId: string) {
    await this.act("Calibration", () => api.calibrate(cameraId));
  }

  /** Graph of a pipeline as the device has it (for screens that reload). */
  graphOf(id: string): GraphDocument {
    return fromDaedalus(this.pipelines.get(id)?.graph ?? null);
  }
}
