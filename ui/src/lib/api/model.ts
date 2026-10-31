// The shapes the UI works with. They follow the HeliOS model: a cluster of
// nodes; resources that exist on them; workloads (graphs) that run and bind
// resources; streams the workloads produce. Device-level facts (slots,
// recovery, logs) come from each node's Atlas agent.

import type { CameraStatus, PeerState, StatusStage } from "./types";

/** A node's kind: a HeliOS device's host (`raze`, `systemcore`, `linux`, from `GET /v1/device`), or a cluster peer's (`mcu`, `coprocessor`, `foreign`). */
export type NodeKind = string;
export type Health = "online" | "degraded" | "offline";
export type Link = "ethernet" | "usb" | "can" | "uart";
export type RecoveryLevel = "R1" | "R2" | "R3" | "R4";

export interface Slot {
  /** A or B on the A/B layout; the slot id on a dynamic-slots board. */
  name: string;
  version: string;
  active: boolean;
  confirmed: boolean;
  /** Dynamic slots: the OS family, its variant, a plain boot's slot, size and the writer's flags. */
  family?: string;
  variant?: string;
  isDefault?: boolean;
  sizeMiB?: number;
  flags?: string[];
}

export interface ClusterNode {
  id: string;
  name: string;
  kind: NodeKind;
  /** Runs HeliOS pipelines (a device's own HeliOS, not a microcontroller or a foreign system). */
  runsPipelines: boolean;
  model: string;
  role: string;
  health: Health;
  link: Link;
  address: string;
  os: { name: string; version: string };
  uptimeS: number;
  cpu: number; // 0..1 of all cores
  cpuHistory: number[];
  memMiB: number;
  memTotalMiB: number;
  tempC: number;
  tempHistory: number[];
  diskUsedMiB: number;
  diskTotalMiB: number;
  clock: { source: "ptp" | "chrony" | "local"; offsetUs: number };
  agent: { version: string; reachable: boolean };
  recovery: RecoveryLevel[];
  slots: Slot[];
  services: Service[];
  foreignSystem?: string;
  /** Another HeliOS device on this robot, reached through the gateway (`GET /v1/peers`); unset on this device. */
  peer?: { routed: boolean; state: PeerState; needsEnrollment?: boolean; keyFingerprint?: string; foundBy?: string };
}

export interface Service {
  name: string;
  state: "running" | "restarting" | "failed" | "stopped";
  memMiB: number;
  restarts: number;
}

export type ResourceType = "camera" | "stream" | "gpio" | "pwm" | "imu" | "fan" | "led" | "power" | "compute" | "bus" | "usb" | "other";
export type Ownership = "exclusive" | "shared-read" | "shared-limited";

export interface Resource {
  id: string;
  type: ResourceType;
  name: string;
  nodeId: string;
  provider: string;
  ownership: Ownership;
  health: Health;
  endpoints: string[];
  labels: Record<string, string>;
  leasedBy: string[];
}

export interface CameraSettings {
  width: number;
  height: number;
  fps: number;
  format: string;
  exposureUs: number;
  autoExposure: boolean;
  gain: number;
  pyramid: boolean;
  roi: { x: number; y: number; w: number; h: number } | null;
}

/** A crop or region rectangle, in frame pixels. Zero width or height: off. */
export interface CropRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** A value written to a camera control: a number, a boolean, a rectangle (a region), or `null` to forget it. */
export type ControlWrite = number | string | boolean | null | CropRect;

/** One entry of a menu control: the value that selects it and its display text (a name, or the
 * number of an integer menu). */
export interface MenuChoice {
  value: number;
  label: string;
  kind: "name" | "int";
}

/** A camera control the device lists (live mode), for generic editors. */
export interface CameraControlInfo {
  id: number;
  name: string;
  kind: "bool" | "int" | "uint" | "float" | "menu" | "int_menu" | "rectangle" | "none" | "unknown";
  min?: number;
  max?: number;
  /** The range the camera takes now, when narrower than min..max (exposure at the frame duration, the gain AE may use). */
  effectiveMin?: number;
  effectiveMax?: number;
  step?: number;
  /** A menu control's entries, in ascending value: only the values it takes (a V4L2 menu's
   * gaps are not listed). */
  menu?: MenuChoice[];
  default: number | boolean | null;
  /** The value now (null when it cannot be read or is not a scalar). */
  value: number | boolean | null;
  /** The standard control it answers (`exposure_us`, `gain`, `ae`, ...). */
  standard: string | null;
  /** The device lets the UI change it. */
  writable: boolean;
  /** The device reports the value and never takes one (`ae_state`, `ae_flicker_detected`). */
  readOnly: boolean;
  /** Rectangle controls (crop regions): the value now. */
  rect?: CropRect | CropRect[] | null;
  /** A value set through the UI is kept across reboots. */
  persisted: boolean;
}

export interface Camera {
  resourceId: string;
  name: string;
  nodeId: string;
  sensor: string;
  backend: string;
  mount: string;
  settings: CameraSettings;
  /** The sensor mode the camera service reports, when it lists one (read-only for now). */
  mode?: string | null;
  stats: { fps: number; dropped: number; latencyMs: number; cpuMsPerFrame: number };
  /** `base`: recorded frames replayed (mocks); `live`: the camera's MJPEG preview URL. */
  feed: { base: string; offset: number; live?: string };
  foreign?: string;
  /** Deeper sensor, ISP and transport controls, by name. */
  extra?: Record<string, number | string | boolean>;
  /** Live mode: the camera's controls as its camera service lists them. */
  controls?: CameraControlInfo[];
  /** Live mode: why the controls could not be read. */
  controlsError?: string;
  /** Live mode: the camera's status chain (why there are no tags, frames, calibration...). */
  status?: CameraStatus | null;
}

export interface GraphNode {
  id: string;
  type: string;
  label: string;
  params: Record<string, number | string | boolean>;
  position: { x: number; y: number };
  /** The Daedalus node this came from (bundle, compute, metadata, typed const inputs), so
   * saving writes it back unchanged apart from the parameters edited here. */
  raw?: unknown;
}

export interface GraphEdge {
  id: string;
  from: { node: string; port: string };
  to: { node: string; port: string };
}

/**
 * The graph editor's form of a pipeline graph. It never travels or is stored
 * as-is: `toDaedalus` (api/adapt.ts) turns it into the versioned Daedalus
 * GraphDocument the device validates and runs, and `fromDaedalus` reads it back.
 */
export interface GraphDocument {
  format: "daedalus.graph";
  schema_version: 1;
  requires: { id: string; version?: string | null }[];
  nodes: GraphNode[];
  edges: GraphEdge[];
  /** The document's own metadata (title, ...), kept on save. */
  metadata?: Record<string, unknown>;
  /** The graph's metadata, kept on save. */
  graphMetadata?: Record<string, unknown>;
}

export type WorkloadState = "running" | "starting" | "quarantined" | "stopped";

/**
 * The CPU cores a pipeline can be pinned to: the engine's cores (helios-engine.service's
 * `CPUAffinity=1 2 3` on the CM5; core 0 is left to the system and the camera).
 */
export const ENGINE_CPU_CORES = [1, 2, 3] as const;

/** One graph input of a pipeline: a camera's frames (with the pipeline's own output size and
 * pyramid) or another pipeline's host output. */
export interface PipelineInput {
  source: "camera" | "pipeline";
  /** Camera inputs: the camera, and what the pipeline asks of it (unset: the camera's own). */
  resourceId?: string;
  outputWidth?: number;
  outputHeight?: number;
  pyramid?: number;
  /** Pipeline inputs: the producer, its output port, and how long a frame waits for it (ms). */
  pipeline?: string;
  port?: string;
  maxWaitMs?: number;
}

export interface Workload {
  id: string;
  name: string;
  nodeId: string;
  revision: number;
  state: WorkloadState;
  restarts: number;
  /** Why the pipeline is not running (its graph, its errors), or that it is. Live mode only. */
  status?: StatusStage;
  /** Each input's camera (`resource_id`); pipeline inputs have none. See `inputs` for everything. */
  bindings: Record<string, string>;
  inputs: Record<string, PipelineInput>;
  /** The Eidos detector's search: `tracked` (a full search every `fullSearchEvery` frames) or `full`; null without a detector. */
  searchMode: "tracked" | "full" | null;
  fullSearchEvery: number | null;
  graph: GraphDocument;
  outputs: string[];
  /** The core the graph thread is pinned to (`null`: the scheduler's choice). */
  cpu: number | null;
  /** The core the graph thread runs on, as the engine applied it (telemetry); `null` if not pinned. */
  cpuApplied: number | null;
  /** Why the engine could not pin the graph thread to `cpu`. */
  cpuError: string | null;
  /** The saved map the live mapper starts from (`null`: none); the mock and the device keep it per pipeline. */
  map?: string | null;
  perf: {
    tickP50Ms: number;
    tickP99Ms: number;
    fps: number;
    memMiB: number;
    nodeMs: Record<string, number>;
    tickHistory: number[];
  };
}

export interface Stream {
  id: string;
  name: string;
  schema: "detections" | "poses" | "telemetry";
  producer: string;
  rateHz: number;
  latencyMs: number;
  bridges: string[];
}

export interface LogLine {
  ts: number;
  nodeId: string;
  unit: string;
  level: "info" | "warn" | "error";
  text: string;
}

export interface Detection {
  id: number;
  /** Corner pixels in the frame the detector saw (`frame`), or in the feed's pixels when `frame` is absent. */
  corners: [number, number][];
  /** The size of the frame the corners are in (the pipeline's frame, e.g. 1280x800), when the output says so. */
  frame?: { w: number; h: number };
}

/** One feature track of a `tracks` output (`eidos:feature_tracks`): stable id, position, age in ticks. */
export interface FeatureTrack {
  id: number;
  x: number;
  y: number;
  age: number;
  /** `new` when detected this tick (age 0), `tracked` when followed from the previous frame. */
  status: "new" | "tracked";
}

/** A `tracks` output's value: the live tracks of one tick and their counts. */
export interface FeatureTracks {
  tracks: FeatureTrack[];
  summary: { count: number; tracked: number; new: number; lost: number };
  /** The size of the frame the positions are in (the pipeline's frame), when known. */
  frame?: { w: number; h: number };
}

export interface TagPose {
  id: number;
  t: [number, number, number];
  r: [number, number, number];
}
