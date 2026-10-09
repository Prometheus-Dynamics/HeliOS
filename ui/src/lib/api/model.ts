// The shapes the UI works with. They follow the HeliOS model: a cluster of
// nodes; resources that exist on them; workloads (graphs) that run and bind
// resources; streams the workloads produce. Device-level facts (slots,
// recovery, logs) come from each node's Atlas agent.

export type NodeKind = "raze" | "mcu" | "coprocessor" | "foreign";
export type Health = "online" | "degraded" | "offline";
export type Link = "ethernet" | "usb" | "can" | "uart";
export type RecoveryLevel = "R1" | "R2" | "R3" | "R4";

export interface Slot {
  name: "A" | "B";
  version: string;
  active: boolean;
  confirmed: boolean;
}

export interface ClusterNode {
  id: string;
  name: string;
  kind: NodeKind;
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

/** A camera control the device lists (live mode), for generic editors. */
export interface CameraControlInfo {
  id: number;
  name: string;
  kind: "bool" | "int" | "uint" | "float" | "menu" | "int_menu" | "rectangle" | "none" | "unknown";
  min?: number;
  max?: number;
  step?: number;
  menu?: string[];
  default: number | boolean | null;
  /** The value now (null when it cannot be read or is not a scalar). */
  value: number | boolean | null;
  /** The standard control it answers (`exposure_us`, `gain`, `ae`, ...). */
  standard: string | null;
  /** The device lets the UI change it. */
  writable: boolean;
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
}

export interface GraphNode {
  id: string;
  type: string;
  label: string;
  params: Record<string, number | string | boolean>;
  position: { x: number; y: number };
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
  requires: { id: string }[];
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export type WorkloadState = "running" | "starting" | "quarantined" | "stopped";

export interface Workload {
  id: string;
  name: string;
  nodeId: string;
  revision: number;
  state: WorkloadState;
  restarts: number;
  bindings: Record<string, string>;
  graph: GraphDocument;
  outputs: string[];
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
  corners: [number, number][];
}

export interface TagPose {
  id: number;
  t: [number, number, number];
  r: [number, number, number];
}
