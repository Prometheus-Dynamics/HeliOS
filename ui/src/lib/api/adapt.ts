// helios-api wire objects -> the UI model, and the editor's graph <-> the
// versioned Daedalus GraphDocument that actually travels and is stored.

import { CATALOG_BY_ID } from "./catalog";
import { previewUrl } from "./client";
import type { Camera, CameraControlInfo, ClusterNode, CropRect, GraphDocument, GraphEdge, GraphNode, LogLine, Resource, ResourceType, Service, Slot, Stream, Workload, WorkloadState } from "./model";
import type * as W from "./types";

// --- Graphs ------------------------------------------------------------------

const EDITOR_KEY = "helios.editor";

type ParamValue = number | string | boolean;

function toValue(value: ParamValue): W.DaedalusValue {
  if (typeof value === "boolean") return { type: "Bool", value };
  if (typeof value === "number") return Number.isInteger(value) ? { type: "Int", value } : { type: "Float", value };
  return { type: "String", value };
}

function fromValue(value: W.DaedalusValue | undefined): ParamValue | undefined {
  if (!value || !("value" in value)) return undefined;
  const v = value.value;
  // An enum (`{ name, value }`) edits as its variant name.
  if (value.type === "Enum" && v && typeof v === "object" && typeof (v as { name?: unknown }).name === "string") return (v as { name: string }).name;
  return typeof v === "number" || typeof v === "string" || typeof v === "boolean" ? v : undefined;
}

/** `value` encoded with the type of the const input it replaces (an enum stays an enum, a
 * float stays a float even when it is a whole number). */
function retype(value: ParamValue, original: W.DaedalusValue): W.DaedalusValue {
  if (fromValue(original) === value) return original;
  switch (original.type) {
    case "Enum":
      return { type: "Enum", value: { name: String(value), value: null } };
    case "Float":
      return { type: "Float", value: Number(value) };
    case "Int":
      return { type: "Int", value: Math.round(Number(value)) };
    case "Bool":
      return { type: "Bool", value: Boolean(value) };
    case "String":
      return { type: "String", value: String(value) };
    default:
      return toValue(value);
  }
}

/** A node's const inputs from its edited parameters, keeping the original types and order. */
function constInputs(n: GraphNode): [string, W.DaedalusValue][] {
  const raw = (n.raw as W.DaedalusNode | undefined)?.const_inputs ?? [];
  const seen = new Set<string>();
  const out: [string, W.DaedalusValue][] = [];
  for (const [name, original] of raw) {
    seen.add(name);
    if (name in n.params) out.push([name, retype(n.params[name], original)]);
    else if (fromValue(original) === undefined) out.push([name, original]); // not editable here: kept as is
  }
  for (const [name, value] of Object.entries(n.params)) if (!seen.has(name)) out.push([name, toValue(value)]);
  return out;
}

/**
 * The editor graph as a Daedalus GraphDocument: nodes reference registry ids,
 * parameters become const inputs, and editor ids/positions ride in metadata
 * so the document round-trips through the device unchanged.
 */
export function toDaedalus(graph: GraphDocument): W.DaedalusGraphDocument {
  const index = new Map(graph.nodes.map((n, i) => [n.id, i]));
  const ports = (n: GraphNode, dir: "inputs" | "outputs") => {
    const declared = CATALOG_BY_ID[n.type]?.[dir].map((p) => p.name);
    if (declared) return declared;
    const used = graph.edges.filter((e) => (dir === "inputs" ? e.to.node : e.from.node) === n.id).map((e) => (dir === "inputs" ? e.to.port : e.from.port));
    return [...new Set(used)];
  };
  return {
    format: "daedalus.graph",
    schema_version: 1,
    requires: graph.requires.map((r) => (r.version ? { id: r.id, version: r.version } : { id: r.id })),
    metadata: {
      ...(graph.metadata ?? {}),
      [EDITOR_KEY]: {
        positions: Object.fromEntries(graph.nodes.map((n) => [n.id, { x: Math.round(n.position.x), y: Math.round(n.position.y) }])),
        edges: graph.edges.map((e) => e.id),
      },
    },
    graph: {
      nodes: graph.nodes.map((n) => {
        const raw = n.raw as W.DaedalusNode | undefined;
        return {
          ...(raw ?? {}),
          id: n.type,
          label: n.id,
          inputs: raw && raw.id === n.type ? raw.inputs : ports(n, "inputs"),
          outputs: raw && raw.id === n.type ? raw.outputs : ports(n, "outputs"),
          const_inputs: constInputs(n),
          metadata: { ...(raw?.metadata ?? {}), "helios.title": { type: "String", value: n.label } },
        };
      }),
      ...(graph.graphMetadata ? { metadata: graph.graphMetadata } : {}),
      edges: graph.edges
        .filter((e) => index.has(e.from.node) && index.has(e.to.node))
        .map((e) => ({ from: { node: index.get(e.from.node)!, port: e.from.port }, to: { node: index.get(e.to.node)!, port: e.to.port } })),
    },
  };
}

/** A Daedalus GraphDocument as the editor graph (nodes laid out on a grid when it has no editor metadata). */
export function fromDaedalus(doc: W.DaedalusGraphDocument | null): GraphDocument {
  if (!doc) return { format: "daedalus.graph", schema_version: 1, requires: [], nodes: [], edges: [] };
  const editor = (doc.metadata?.[EDITOR_KEY] ?? {}) as { positions?: Record<string, { x: number; y: number }>; edges?: string[] };
  const ids = doc.graph.nodes.map((n, i) => n.label || `${n.id.split(/[:.]/).at(-1)}_${i}`);
  const nodes: GraphNode[] = doc.graph.nodes.map((n, i) => {
    const params: Record<string, ParamValue> = {};
    for (const [name, value] of n.const_inputs ?? []) {
      const v = fromValue(value);
      if (v !== undefined) params[name] = v;
    }
    const title = fromValue(n.metadata?.["helios.title"]);
    return {
      id: ids[i],
      type: n.id,
      label: typeof title === "string" ? title : (CATALOG_BY_ID[n.id]?.title ?? n.id),
      params,
      position: editor.positions?.[ids[i]] ?? { x: (i % 4) * 270, y: Math.floor(i / 4) * 200 },
      raw: n,
    };
  });
  const edges: GraphEdge[] = doc.graph.edges.map((e, i) => ({ id: editor.edges?.[i] ?? `e${i + 1}`, from: { node: ids[e.from.node], port: e.from.port }, to: { node: ids[e.to.node], port: e.to.port } }));
  const { [EDITOR_KEY]: _editor, ...metadata } = (doc.metadata ?? {}) as Record<string, unknown>;
  return { format: "daedalus.graph", schema_version: 1, requires: doc.requires.map((r) => ({ id: r.id, version: r.version ?? null })), nodes, edges, metadata, graphMetadata: doc.graph.metadata };
}

// --- Device ------------------------------------------------------------------

const MiB = 1024 * 1024;

function serviceState(unit: W.UnitStatus): Service["state"] {
  if (unit.active_state === "active") return "running";
  if (unit.active_state === "activating" || unit.active_state === "reloading") return "restarting";
  if (unit.active_state === "failed") return "failed";
  return "stopped";
}

export function toService(unit: W.UnitStatus): Service {
  return { name: unit.unit.replace(/\.service$/, ""), state: serviceState(unit), memMiB: (unit.memory_bytes ?? 0) / MiB, restarts: unit.restarts ?? 0 };
}

function slotsFrom(status: W.UpdateStatus | null, version: string): Slot[] {
  if (status?.slots.layout === "slots") {
    return (status.slots.installed ?? []).map((s) => ({
      name: s.id,
      version: s.version ?? "",
      active: s.id === status.slots.active,
      confirmed: s.good !== "no" && !(s.flags ?? []).includes("trial"),
      family: s.family,
      variant: s.variant,
      isDefault: s.id === status.slots.default,
      sizeMiB: s.size ? s.size / 1024 / 1024 : undefined,
      flags: s.flags,
    }));
  }
  const active = status?.slots.active?.toUpperCase();
  if (active !== "A" && active !== "B") return [];
  // On a trial boot the running slot is not confirmed until update-health passes.
  const confirmed = status?.phase !== "trying" || status.slots.staged?.toUpperCase() !== active;
  const other = status?.slots.staged?.toUpperCase() !== active ? (status?.version_staged ?? "") : "";
  return (["A", "B"] as const).map((name) => ({ name, version: name === active ? (status?.version_active ?? version) : other, active: name === active, confirmed: name === active ? confirmed : false }));
}

function clockSource(source: string | undefined): ClusterNode["clock"]["source"] {
  return source === "ptp" ? "ptp" : source === "chrony" || source === "ntp" ? "chrony" : "local";
}

export interface DeviceSnapshot {
  device: W.Device;
  identity: W.Identity | null;
  metrics: W.Metrics | null;
  services: W.UnitStatus[];
  update: W.UpdateStatus | null;
}

/** This device as the UI's cluster node. */
export function toNode(snapshot: DeviceSnapshot, previous?: ClusterNode): ClusterNode {
  const { device, identity, metrics, services, update } = snapshot;
  const cpu = metrics?.cpu ?? previous?.cpu ?? 0;
  const tempC = metrics?.temperature_c ?? previous?.tempC ?? 0;
  const push = (history: number[] | undefined, value: number) => [...(history ?? []), value].slice(-60);
  const version = device.os.version ?? identity?.os.version ?? "unknown";
  const recovery: ClusterNode["recovery"] = ["R1"];
  if (identity?.update_methods.includes("ab-tryboot") || update?.slots.active) recovery.push("R2");
  recovery.push("R4");
  const total = metrics?.memory_total_bytes ?? 0;
  return {
    id: device.node_id,
    name: device.hostname ?? identity?.hostname ?? device.node_id,
    kind: "raze",
    model: device.model ?? identity?.model ?? "HeliOS device",
    role: "HeliOS",
    health: device.orion.reachable ? "online" : "degraded",
    link: "ethernet",
    address: typeof window === "undefined" ? "" : window.location.hostname,
    os: { name: device.os.name ?? "HeliOS", version },
    uptimeS: metrics?.uptime_s ?? device.uptime_s ?? 0,
    cpu,
    cpuHistory: metrics ? push(previous?.cpuHistory, cpu) : (previous?.cpuHistory ?? []),
    memMiB: (total - (metrics?.memory_available_bytes ?? total)) / MiB,
    memTotalMiB: total / MiB,
    tempC,
    tempHistory: metrics?.temperature_c != null ? push(previous?.tempHistory, tempC) : (previous?.tempHistory ?? []),
    diskUsedMiB: (metrics?.disk?.used_bytes ?? 0) / MiB,
    diskTotalMiB: (metrics?.disk?.total_bytes ?? 0) / MiB,
    clock: { source: clockSource(device.clock?.source), offsetUs: Math.abs(Math.round(device.clock?.offset_us ?? 0)) },
    agent: { version: device.api, reachable: true },
    recovery,
    slots: slotsFrom(update, version),
    services: services.map(toService),
  };
}

// --- Cameras -----------------------------------------------------------------

function parseMode(mode: string | undefined): { width: number; height: number; format: string } {
  const size = mode?.match(/(\d+)\s*x\s*(\d+)/);
  const format = mode?.match(/\b([A-Z][A-Z0-9]{2,})\b/)?.[1];
  return { width: size ? Number(size[1]) : 1280, height: size ? Number(size[2]) : 800, format: format ?? "?" };
}

function mountText(mount: W.CameraMount | null): string {
  if (!mount) return "Not placed";
  return `x ${mount.x.toFixed(2)} y ${mount.y.toFixed(2)} z ${mount.z.toFixed(2)} m · yaw ${Math.round(mount.yaw)}° pitch ${Math.round(mount.pitch)}°`;
}

/** A camera's display name and detail from the device string Styx reports: "ov9782 (kernel
 * driver i2c 10-0060)" shows as "OV9782", with "kernel driver i2c 10-0060" as its detail. */
export function cameraLabel(raw: string): { name: string; detail: string } {
  const m = /^\s*([A-Za-z0-9_-]+)\s*\((.+)\)\s*$/.exec(raw);
  return m ? { name: m[1].toUpperCase(), detail: m[2] } : { name: raw, detail: "" };
}

export function toCamera(camera: W.Camera): Camera {
  const capture = camera.live?.captures[0];
  const label = cameraLabel(camera.name);
  const mode = parseMode(capture?.mode);
  const ae = capture?.ae_state?.toLowerCase();
  return {
    resourceId: camera.id,
    name: label.name,
    nodeId: camera.node_id,
    sensor: label.detail || (camera.live?.sources[0]?.name ?? camera.labels.sensor ?? camera.name),
    backend: camera.backend ? `styx ${camera.backend}` : "styx",
    mount: mountText(camera.mount),
    settings: {
      width: mode.width,
      height: mode.height,
      fps: capture?.fps_configured ?? 0,
      format: mode.format,
      exposureUs: capture?.exposure_us ?? 0,
      autoExposure: !!ae && !["off", "manual", "locked"].includes(ae),
      gain: capture?.analogue_gain ?? 1,
      pyramid: false,
      roi: null,
    },
    stats: { fps: capture?.fps_measured ?? 0, dropped: capture?.drops ?? 0, latencyMs: capture?.latency_p50_ms ?? 0, cpuMsPerFrame: (capture?.cpu_per_frame_us ?? 0) / 1000 },
    // The camera service's low-priority JPEG preview; without a camera service the feed
    // shows a placeholder.
    feed: { base: "", offset: 0, live: camera.preview_available ? previewUrl(camera.id) : undefined },
  };
}

const scalar = (value: W.ControlValue | null | undefined): number | boolean | null => (typeof value === "number" || typeof value === "boolean" ? value : null);
const num = (value: W.ControlValue | null | undefined): number | undefined => (typeof value === "number" ? value : undefined);

/** The camera service's control descriptors, for the camera pane's editors. */
export function toControls(settings: W.CameraSettings): CameraControlInfo[] {
  return settings.controls.map((c) => ({
    id: c.id,
    name: c.name,
    kind: c.kind,
    min: num(c.min),
    max: num(c.max),
    effectiveMin: num(c.effective_min),
    effectiveMax: num(c.effective_max),
    step: num(c.step),
    menu: c.menu ?? undefined,
    default: scalar(c.default),
    value: scalar(c.current),
    standard: c.standard,
    writable: c.writable && !c.read_only,
    readOnly: c.read_only,
    rect: rectOf(c.current),
    persisted: c.persisted ?? false,
  }));
}

const isRect = (value: unknown): value is CropRect => typeof value === "object" && value !== null && "width" in value && "height" in value;
/** A rectangle control's value (one, or a list); undefined for a control that is not a rectangle. */
const rectOf = (value: W.ControlValue | null | undefined): CropRect | CropRect[] | undefined => {
  if (Array.isArray(value)) return value.every(isRect) ? value : undefined;
  return isRect(value) ? value : undefined;
};

/** Set a control's value on the camera, and the settings fields its standard control feeds. */
export function applyControlValue(camera: Camera, match: { id?: number; standard?: string | null; name?: string }, value: W.ControlValue | null) {
  const control = camera.controls?.find((c) => (match.standard && c.standard === match.standard) || (match.id !== undefined && c.id === match.id) || (match.name !== undefined && c.name === match.name));
  const v = scalar(value);
  if (control) control.value = v;
  if (control && control.rect !== undefined && isRect(value)) control.rect = value;
  const standard = match.standard ?? control?.standard;
  if (v === null || !standard) return;
  if (standard === "exposure_us" && typeof v === "number") camera.settings.exposureUs = v;
  else if (standard === "gain" && typeof v === "number") camera.settings.gain = v;
  else if (standard === "ae" && typeof v === "boolean") camera.settings.autoExposure = v;
  else if (standard === "fps" && typeof v === "number") camera.settings.fps = v;
}

/** Mark the controls whose values the camera keeps across reboots (a `persisted` map of keys: standard keys or names). */
export function applyPersisted(camera: Camera, persisted: Record<string, W.ControlValue>) {
  for (const control of camera.controls ?? []) control.persisted = control.name in persisted || (control.standard !== null && control.standard in persisted);
}

/** Take a `GET /v1/cameras/{id}/settings` answer into the camera. */
export function applyCameraSettings(camera: Camera, settings: W.CameraSettings) {
  camera.mode = settings.mode;
  camera.controls = toControls(settings);
  camera.controlsError = undefined;
  for (const control of camera.controls) if (control.standard) applyControlValue(camera, { standard: control.standard }, control.value);
}

// --- Pipelines ---------------------------------------------------------------

function workloadState(p: W.Pipeline): WorkloadState {
  if (!p.enabled) return "stopped";
  switch (p.state) {
    case "running":
      return "running";
    case "failed":
      return "quarantined";
    case "stopped":
    case "completed":
      return "stopped";
    default:
      return "starting";
  }
}

export function outputStreamId(pipeline: string, port: string): string {
  return `output.${pipeline}.${port}`;
}

/** Per graph node, the milliseconds per tick its Daedalus stages took over the metrics window.
 * A node's stages are the planned nodes with its label, or whose label starts with it (a node
 * group's internal stages: `detector/plan`, `detector.mask`, ...). */
export function nodeTimes(p: W.Pipeline, labels: string[]): Record<string, number> {
  const nodes = p.metrics?.window?.nodes ?? [];
  const out: Record<string, number> = {};
  for (const label of labels) {
    const own = nodes.filter((n) => n.label === label || n.label.startsWith(`${label}/`) || n.label.startsWith(`${label}.`) || n.label.startsWith(`${label}:`));
    if (own.length) out[label] = own.reduce((sum, n) => sum + (n.handler_us_per_tick || n.total_us_per_tick || 0), 0) / 1000;
  }
  return out;
}

/** The q-quantile of a sample (0 for none). */
function quantile(values: number[], q: number): number {
  const v = values.filter((x) => x > 0).sort((a, b) => a - b);
  return v.length ? v[Math.min(v.length - 1, Math.floor(q * v.length))] : 0;
}

export function toWorkload(p: W.Pipeline, nodeId: string, previous?: Workload): Workload {
  const fps = Number(p.telemetry?.fps ?? 0);
  const tick = Number(p.telemetry?.last_tick_ms ?? 0);
  const graph = fromDaedalus(p.graph);
  const history = [...(previous?.perf.tickHistory ?? []), tick].slice(-60);
  return {
    id: p.id,
    name: p.name,
    nodeId: p.node_id ?? nodeId,
    revision: p.revision ?? 0,
    state: workloadState(p),
    restarts: previous?.restarts ?? 0,
    bindings: Object.fromEntries(Object.entries(p.bindings).map(([input, b]) => [input, b.resource_id])),
    graph,
    outputs: p.outputs.map((o) => outputStreamId(p.id, o.port)),
    perf: { tickP50Ms: quantile(history, 0.5), tickP99Ms: quantile(history, 0.99), fps, memMiB: 0, nodeMs: nodeTimes(p, graph.nodes.map((n) => n.id)), tickHistory: history },
  };
}

export function toStreams(pipelines: W.Pipeline[]): Stream[] {
  return pipelines.flatMap((p) =>
    p.outputs.map((o) => ({
      id: outputStreamId(p.id, o.port),
      name: `${p.name} · ${o.port}`,
      schema: /pose/i.test(o.port) ? ("poses" as const) : /detect|marker|tag/i.test(o.port) ? ("detections" as const) : ("telemetry" as const),
      producer: p.id,
      rateHz: Number(p.telemetry?.fps ?? 0),
      latencyMs: 0,
      bridges: ["api"],
    })),
  );
}

// --- Resources ---------------------------------------------------------------

function resourceType(type: string): ResourceType {
  if (type === "camera.device") return "camera";
  if (type.startsWith("gpio.")) return "gpio";
  if (type.startsWith("pwm.")) return "pwm";
  if (type.startsWith("execution.")) return "compute";
  if (type.startsWith("usb.")) return "usb";
  if (type.startsWith("i2c.") || type.startsWith("spi.")) return "bus";
  return "other";
}

function health(h: string): Resource["health"] {
  return h === "healthy" ? "online" : h === "failed" ? "offline" : "degraded";
}

export function toResource(r: W.Resource, nodeId: string): Resource {
  return {
    id: r.id,
    type: resourceType(r.type),
    name: r.name,
    nodeId,
    provider: r.provider.replace(/^provider\./, "").replace(/\.[^.]+$/, ""),
    ownership: r.ownership === "exclusive" ? "exclusive" : r.ownership === "shared_read" ? "shared-read" : "shared-limited",
    health: health(r.health),
    endpoints: r.endpoints,
    labels: { ...r.labels, orion_type: r.type },
    leasedBy: r.leased_by,
  };
}

// --- Logs --------------------------------------------------------------------

export function toLogLine(line: W.LogLine, nodeId: string): LogLine {
  return { ts: line.at_ms, nodeId, unit: line.unit, level: line.level === "debug" ? "info" : line.level, text: line.message };
}
