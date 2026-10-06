// The cluster: everything the screens read, plus the actions they take. Live
// by default: the device's helios-api through LiveCluster (live.svelte.ts).
// With `?mock=1` (or VITE_HELIOS_MOCK=1) it runs the simulated robot from
// mock.ts instead, for UI work without a device.

import { CAMERAS, LOGS, NODES, RESOURCES, STREAMS, WORKLOADS } from "$lib/api/mock";
import { MOCK } from "$lib/api/mode";
import { LiveCluster } from "./live.svelte";
import type { Camera, CameraSettings, ClusterNode, Detection, GraphDocument, LogLine, Resource, Stream, TagPose, Workload } from "$lib/api/model";
import { toasts } from "./toasts.svelte";

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
  nodes = $state<ClusterNode[]>(MOCK ? clone(NODES) : []);
  cameras = $state<Camera[]>(MOCK ? clone(CAMERAS) : []);
  workloads = $state<Workload[]>(MOCK ? clone(WORKLOADS) : []);
  streams = $state<Stream[]>(MOCK ? clone(STREAMS) : []);
  resources = $state<Resource[]>(MOCK ? clone(RESOURCES) : []);
  logs = $state<LogLine[]>(MOCK ? clone(LOGS) : []);
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

  start() {
    if (this.started || typeof window === "undefined") return () => {};
    this.started = true;
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
    return this.feed?.frames[this.feedIndex(camera)]?.markers ?? [];
  }

  posesFor(camera: Camera): TagPose[] {
    return this.poses?.frames[this.feedIndex(camera)] ?? [];
  }

  node(id: string) {
    return this.nodes.find((n) => n.id === id);
  }
  camera(id: string) {
    return this.cameras.find((c) => c.resourceId === id);
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
      items.push({ tone: "error", text: `${w.name} is quarantined after ${w.restarts} restarts`, href: `/pipelines/${w.id}` });
    }
    for (const n of this.nodes.filter((x) => x.health !== "online")) {
      items.push({ tone: n.health === "offline" ? "error" : "warning", text: `${n.name} is ${n.health}`, href: `/devices/${n.id}` });
    }
    for (const n of this.nodes.filter((x) => x.clock.offsetUs > 1000 && x.health !== "offline")) {
      items.push({ tone: "warning", text: `${n.name} clock is ${(n.clock.offsetUs / 1000).toFixed(1)} ms off`, href: `/devices/${n.id}` });
    }
    return items;
  }

  private log(nodeId: string, unit: string, level: LogLine["level"], text: string) {
    this.logs.push({ ts: Date.now(), nodeId, unit, level, text });
  }

  private async later(ms = 700) {
    await new Promise((r) => setTimeout(r, ms));
  }

  // --- Actions -----------------------------------------------------------

  async restartWorkload(id: string) {
    if (this.live) return this.live.restartWorkload(id);
    const w = this.workload(id);
    if (!w) return;
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
    await this.later(300);
    w.state = "stopped";
    this.log(w.nodeId, "helios-runner", "info", `${w.name} stopped`);
    toasts.info(`${w.name} stopped`);
  }

  async saveGraph(id: string, graph: GraphDocument) {
    if (this.live) return this.live.saveGraph(id, graph);
    const w = this.workload(id);
    if (!w) return;
    await this.later(500);
    this.revisions.set(id, [...(this.revisions.get(id) ?? []), { revision: w.revision, graph: clone(w.graph) }]);
    w.graph = clone(graph);
    w.revision += 1;
    w.state = "running";
    this.log(w.nodeId, "helios-runner", "info", `${w.name} revision ${w.revision} deployed; previous revision kept for rollback`);
    toasts.success(`Deployed revision ${w.revision} to ${this.node(w.nodeId)?.name}`);
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

  async createWorkload(name: string, nodeId: string, cameraId: string, graph: GraphDocument): Promise<string> {
    if (this.live) return this.live.createWorkload(name, nodeId, cameraId, graph);
    await this.later(500);
    const id = `wl-${Math.random().toString(36).slice(2, 7)}`;
    this.workloads.push({
      id,
      name,
      nodeId,
      revision: 1,
      state: "running",
      restarts: 0,
      bindings: { camera: cameraId },
      graph: clone(graph),
      outputs: [`stream.${id}.poses`],
      perf: { tickP50Ms: 0.7, tickP99Ms: 0.8, fps: 60, memMiB: 8.9, nodeMs: Object.fromEntries(graph.nodes.map((n) => [n.id, 0.05])), tickHistory: [] },
    });
    toasts.success(`${name} deployed to ${this.node(nodeId)?.name}`);
    return id;
  }

  /** Applies camera settings live (controls call this as you drag). */
  setCamera(id: string, settings: Partial<CameraSettings>, extra?: Record<string, number | string | boolean>) {
    if (this.live) {
      void this.live.setCamera(id, { ...settings, ...(extra ?? {}) });
      return;
    }
    const c = this.camera(id);
    if (!c) return;
    Object.assign(c.settings, settings);
    if (extra) c.extra = { ...(c.extra ?? {}), ...extra };
    c.stats.fps = c.settings.fps;
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
    if (this.live) return this.live.restartService(service);
    const s = this.node(nodeId)?.services.find((x) => x.name === service);
    if (!s) return;
    s.state = "restarting";
    await this.later();
    s.state = "running";
    this.log(nodeId, service, "info", "restarted by request");
    toasts.success(`${service} restarted`);
  }

  async switchSlot(nodeId: string) {
    if (this.live) return this.live.switchSlot();
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
    if (this.live) return this.live.safeMode();
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
