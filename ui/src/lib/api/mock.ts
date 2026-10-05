// A simulated robot: the cluster a HeliOS UI would see on a real robot, with
// live readings. Camera feeds replay the robot video (frames and detections
// from the CM5 run). Swap this for the Orion/agent client without changing
// any screen.

import { CATALOG_BY_ID, defaults } from "./catalog";
import type { Camera, ClusterNode, GraphDocument, GraphEdge, GraphNode, LogLine, Resource, Service, Slot, Stream, Workload } from "./model";

const now = Date.now();

function history(base: number, spread: number, n = 60): number[] {
  return Array.from({ length: n }, (_, i) => Math.max(0, base + Math.sin(i / 5) * spread * 0.5 + (Math.random() - 0.5) * spread));
}

function slots(active: "A" | "B", current: string, previous: string): Slot[] {
  return [
    { name: "A", version: active === "A" ? current : previous, active: active === "A", confirmed: true },
    { name: "B", version: active === "B" ? current : previous, active: active === "B", confirmed: true },
  ];
}

const razeServices = (): Service[] => [
  { name: "atlas-agent", state: "running", memMiB: 2.1, restarts: 0 },
  { name: "orion-node", state: "running", memMiB: 9.8, restarts: 0 },
  { name: "helios-io", state: "running", memMiB: 7.4, restarts: 0 },
  { name: "helios-runner", state: "running", memMiB: 4.2, restarts: 0 },
  { name: "helios-api", state: "running", memMiB: 5.6, restarts: 0 },
];

function raze(id: string, name: string, role: string, address: string, cpu: number, temp: number): ClusterNode {
  return {
    id,
    name,
    kind: "raze",
    model: "Raze CM5 · OV9782",
    role,
    health: "online",
    link: "ethernet",
    address,
    os: { name: "HeliOS", version: "2026.4.0" },
    uptimeS: 3600 * 2 + Math.random() * 900,
    cpu,
    cpuHistory: history(cpu, 0.03),
    memMiB: 38 + Math.random() * 6,
    memTotalMiB: 1950,
    tempC: temp,
    tempHistory: history(temp, 2),
    diskUsedMiB: 61,
    diskTotalMiB: 14100,
    clock: { source: "ptp", offsetUs: Math.round(Math.random() * 40 + 5) },
    agent: { version: "0.3.0", reachable: true },
    recovery: ["R1", "R2", "R3", "R4"],
    slots: slots("A", "2026.4.0", "2026.3.2"),
    services: razeServices(),
  };
}

export const NODES: ClusterNode[] = [
  raze("raze-front", "Front", "AprilTag localisation", "10.0.0.11", 0.055, 51),
  raze("raze-back", "Back", "AprilTag localisation", "10.0.0.12", 0.052, 49),
  raze("raze-left", "Left", "Game piece detection", "10.0.0.13", 0.081, 55),
  raze("raze-right", "Right", "AprilTag localisation", "10.0.0.14", 0.049, 48),
  {
    id: "mcu-imu",
    name: "IMU board",
    kind: "mcu",
    model: "RP2350 · BMI088 + BMM150",
    role: "Orientation",
    health: "online",
    link: "can",
    address: "can0 · 0x21",
    os: { name: "agent firmware", version: "0.3.0" },
    uptimeS: 3600 * 2 + 400,
    cpu: 0.12,
    cpuHistory: history(0.12, 0.02),
    memMiB: 0.09,
    memTotalMiB: 0.5,
    tempC: 39,
    tempHistory: history(39, 1),
    diskUsedMiB: 0.6,
    diskTotalMiB: 4,
    clock: { source: "ptp", offsetUs: 120 },
    agent: { version: "0.3.0", reachable: true },
    recovery: ["R1", "R2", "R3", "R4"],
    slots: slots("B", "0.3.0", "0.2.4"),
    services: [{ name: "imu-driver", state: "running", memMiB: 0.03, restarts: 0 }],
  },
  {
    id: "mcu-leds",
    name: "LED controller",
    kind: "mcu",
    model: "RP2040 · WS2812 ×4",
    role: "Status lighting",
    health: "online",
    link: "uart",
    address: "ttyAMA2",
    os: { name: "agent firmware", version: "0.3.0" },
    uptimeS: 3600 * 2 + 410,
    cpu: 0.04,
    cpuHistory: history(0.04, 0.01),
    memMiB: 0.05,
    memTotalMiB: 0.26,
    tempC: 36,
    tempHistory: history(36, 1),
    diskUsedMiB: 0.3,
    diskTotalMiB: 2,
    clock: { source: "local", offsetUs: 900 },
    agent: { version: "0.3.0", reachable: true },
    recovery: ["R1", "R2", "R4"],
    slots: slots("A", "0.3.0", "0.3.0"),
    services: [{ name: "led-driver", state: "running", memMiB: 0.02, restarts: 0 }],
  },
  {
    id: "pv-coproc",
    name: "PhotonVision coprocessor",
    kind: "foreign",
    model: "Orange Pi 5 · PhotonVision 2026.1",
    role: "Foreign cameras (adapter)",
    health: "degraded",
    link: "ethernet",
    address: "10.0.0.31",
    os: { name: "PhotonVision", version: "2026.1.2" },
    uptimeS: 3600 + 20,
    cpu: 0.34,
    cpuHistory: history(0.34, 0.08),
    memMiB: 612,
    memTotalMiB: 7900,
    tempC: 62,
    tempHistory: history(62, 3),
    diskUsedMiB: 3100,
    diskTotalMiB: 29000,
    clock: { source: "chrony", offsetUs: 2400 },
    agent: { version: "0.3.0", reachable: true },
    recovery: ["R1", "R4"],
    slots: [],
    services: [{ name: "photonvision-adapter", state: "running", memMiB: 3.1, restarts: 2 }],
    foreignSystem: "PhotonVision",
  },
  {
    id: "driver-laptop",
    name: "Driver laptop",
    kind: "coprocessor",
    model: "Linux laptop",
    role: "Dashboards · USB camera",
    health: "online",
    link: "ethernet",
    address: "10.0.0.5",
    os: { name: "Ubuntu", version: "24.04" },
    uptimeS: 3600 * 5,
    cpu: 0.08,
    cpuHistory: history(0.08, 0.03),
    memMiB: 4300,
    memTotalMiB: 16000,
    tempC: 47,
    tempHistory: history(47, 2),
    diskUsedMiB: 210000,
    diskTotalMiB: 480000,
    clock: { source: "ptp", offsetUs: 30 },
    agent: { version: "0.3.0", reachable: true },
    recovery: ["R1"],
    slots: [],
    services: [{ name: "orion-node", state: "running", memMiB: 11, restarts: 0 }],
  },
];

function cam(id: string, name: string, nodeId: string, mount: string, offset: number, extra: Partial<Camera> = {}): Camera {
  return {
    resourceId: id,
    name,
    nodeId,
    sensor: "OV9782 · global shutter",
    backend: "styx native · PiSP",
    mount,
    settings: { width: 1280, height: 800, fps: 60, format: "GREY", exposureUs: 2200, autoExposure: false, gain: 4, pyramid: true, roi: null },
    stats: { fps: 60, dropped: 0, latencyMs: 9.7, cpuMsPerFrame: 0.3 },
    feed: { base: "/mock/robot", offset },
    ...extra,
  };
}

export const CAMERAS: Camera[] = [
  cam("camera.raze-front.cam0", "Front", "raze-front", "Front bumper, 18° up", 0),
  cam("camera.raze-back.cam0", "Back", "raze-back", "Rear frame, 15° up", 120),
  cam("camera.raze-left.cam0", "Left", "raze-left", "Left side, intake", 240),
  cam("camera.raze-right.cam0", "Right", "raze-right", "Right side, 20° up", 360),
  cam("camera.pv-coproc.arducam", "PV Arducam", "pv-coproc", "Turret", 60, {
    sensor: "Arducam OV9281",
    backend: "PhotonVision adapter",
    foreign: "PhotonVision",
    stats: { fps: 30, dropped: 3, latencyMs: 31, cpuMsPerFrame: 0 },
    settings: { width: 1280, height: 800, fps: 30, format: "MJPEG", exposureUs: 6000, autoExposure: true, gain: 1, pyramid: false, roi: null },
  }),
  cam("camera.driver-laptop.usb0", "Laptop webcam", "driver-laptop", "Driver station", 300, {
    sensor: "UVC 1080p",
    backend: "styx uvc",
    stats: { fps: 30, dropped: 0, latencyMs: 41, cpuMsPerFrame: 1.2 },
    settings: { width: 1280, height: 720, fps: 30, format: "MJPEG", exposureUs: 16000, autoExposure: true, gain: 1, pyramid: false, roi: null },
  }),
];

let edgeSeq = 0;
function edge(from: string, fromPort: string, to: string, toPort: string): GraphEdge {
  edgeSeq += 1;
  return { id: `e${edgeSeq}`, from: { node: from, port: fromPort }, to: { node: to, port: toPort } };
}
function node(id: string, type: string, x: number, y: number, params: Record<string, number | string | boolean> = {}): GraphNode {
  return { id, type, label: CATALOG_BY_ID[type]?.title ?? type, params: { ...defaults(CATALOG_BY_ID[type]), ...params }, position: { x, y } };
}

export function tagGraph(dictionary: string, table: string): GraphDocument {
  return {
    format: "daedalus.graph",
    schema_version: 1,
    requires: [{ id: "styx.frames" }, { id: "eidos" }, { id: "helios" }],
    nodes: [
      node("camera", "styx:camera", 0, 130),
      node("mask", "eidos:aruco.mask_prep_runs", 270, 0),
      node("quads", "eidos:aruco.quads_from_runs", 270, 250),
      node("decode", "eidos:aruco.decode", 540, 0, { dictionary }),
      node("validate", "eidos:aruco.validate", 540, 250),
      node("refine", "eidos:aruco.refine", 810, 0),
      node("pose", "eidos:aruco.pose", 810, 250),
      node("stream", "helios:stream", 1080, 60, { name: "poses" }),
      node("nt4", "helios:nt4", 1080, 270, { table }),
    ],
    edges: [
      edge("camera", "frame", "mask", "frame"),
      edge("mask", "runs", "quads", "runs"),
      edge("camera", "frame", "decode", "frame"),
      edge("quads", "quads", "decode", "quads"),
      edge("camera", "frame", "validate", "frame"),
      edge("decode", "detections", "validate", "detections"),
      edge("camera", "frame", "refine", "frame"),
      edge("validate", "detections", "refine", "detections"),
      edge("validate", "detections", "pose", "detections"),
      edge("refine", "refined_corners", "pose", "refined_corners"),
      edge("pose", "poses", "stream", "value"),
      edge("pose", "poses", "nt4", "value"),
    ],
  };
}

const tagNodeMs = { camera: 0.02, mask: 0.3, quads: 0.29, decode: 0.04, validate: 0.005, refine: 0.004, pose: 0.01, stream: 0.006, nt4: 0.008 };

function tagWorkload(id: string, name: string, nodeId: string, cameraId: string, table: string): Workload {
  return {
    id,
    name,
    nodeId,
    revision: 7,
    state: "running",
    restarts: 0,
    bindings: { camera: cameraId },
    graph: tagGraph("apriltag_36h11", table),
    outputs: [`stream.${id}.poses`],
    perf: { tickP50Ms: 0.7, tickP99Ms: 0.8, fps: 60, memMiB: 8.9, nodeMs: { ...tagNodeMs }, tickHistory: history(0.7, 0.05) },
  };
}

export const WORKLOADS: Workload[] = [
  tagWorkload("tags-front", "AprilTags · front", "raze-front", "camera.raze-front.cam0", "/helios/front"),
  tagWorkload("tags-back", "AprilTags · back", "raze-back", "camera.raze-back.cam0", "/helios/back"),
  tagWorkload("tags-right", "AprilTags · right", "raze-right", "camera.raze-right.cam0", "/helios/right"),
  {
    ...tagWorkload("tags-left", "ArUco game pieces · left", "raze-left", "camera.raze-left.cam0", "/helios/left"),
    revision: 3,
    graph: tagGraph("aruco_4x4_50", "/helios/left"),
  },
  {
    ...tagWorkload("tags-left-exp", "Experimental CLAHE tags · left", "raze-left", "camera.raze-left.cam0", "/helios/left"),
    revision: 2,
    state: "quarantined",
    restarts: 5,
    perf: { tickP50Ms: 0, tickP99Ms: 0, fps: 0, memMiB: 0, nodeMs: {}, tickHistory: [] },
  },
];

export const STREAMS: Stream[] = [
  ...WORKLOADS.filter((w) => w.state === "running").map((w) => ({
    id: `stream.${w.id}.poses`,
    name: `${w.name} · poses`,
    schema: "poses" as const,
    producer: w.id,
    rateHz: 60,
    latencyMs: 1.9,
    bridges: ["nt4", "api"],
  })),
  { id: "stream.pv-coproc.targets", name: "PhotonVision · turret targets", schema: "detections", producer: "photonvision-adapter", rateHz: 30, latencyMs: 34, bridges: ["nt4", "api"] },
  { id: "stream.mcu-imu.orientation", name: "IMU · orientation", schema: "telemetry", producer: "imu-driver", rateHz: 200, latencyMs: 0.6, bridges: ["api", "cluster"] },
];

export const RESOURCES: Resource[] = [
  ...CAMERAS.map((c) => ({
    id: c.resourceId,
    type: "camera" as const,
    name: `${c.name} camera`,
    nodeId: c.nodeId,
    provider: c.foreign ? "photonvision-adapter" : "helios-io",
    ownership: "shared-read" as const,
    health: c.foreign ? ("degraded" as const) : ("online" as const),
    endpoints: c.foreign ? [`mjpeg+http://10.0.0.31:1182/stream`] : [`styx-frames+unix:///run/helios/cameras/${c.resourceId}.sock`],
    labels: { sensor: c.sensor, mount: c.mount },
    leasedBy: WORKLOADS.filter((w) => Object.values(w.bindings).includes(c.resourceId) && w.state === "running").map((w) => w.id),
  })),
  ...STREAMS.map((s) => ({
    id: s.id,
    type: "stream" as const,
    name: s.name,
    nodeId: WORKLOADS.find((w) => w.id === s.producer)?.nodeId ?? (s.producer.startsWith("imu") ? "mcu-imu" : "pv-coproc"),
    provider: s.producer,
    ownership: "shared-read" as const,
    health: "online" as const,
    endpoints: [`stream+quic://cluster/${s.id}`, ...s.bridges.map((b) => (b === "nt4" ? "nt4://10.0.0.2/helios" : b === "api" ? "ws://helios.local/v1/streams" : "orion-link"))],
    labels: { schema: s.schema, rate: `${s.rateHz} Hz` },
    leasedBy: [],
  })),
  { id: "imu.mcu-imu.bmi088", type: "imu", name: "BMI088 accel + gyro", nodeId: "mcu-imu", provider: "imu-driver", ownership: "shared-read", health: "online", endpoints: ["orion-link://can0/0x21"], labels: { rate: "1 kHz" }, leasedBy: [] },
  { id: "led.mcu-leds.strip0", type: "led", name: "Status strip", nodeId: "mcu-leds", provider: "led-driver", ownership: "exclusive", health: "online", endpoints: ["orion-link://ttyAMA2"], labels: { pixels: "60" }, leasedBy: ["lighting"] },
  ...NODES.filter((n) => n.kind === "raze").flatMap((n): Resource[] => [
    { id: `fan.${n.id}`, type: "fan" as const, name: "Cooling fan", nodeId: n.id, provider: "helios-io", ownership: "exclusive" as const, health: "online" as const, endpoints: [`lemnos://hwmon/pwm-fan`], labels: { mode: "auto" }, leasedBy: [] },
    { id: `compute.${n.id}`, type: "compute" as const, name: "Graph runner", nodeId: n.id, provider: "helios-runner", ownership: "shared-limited" as const, health: "online" as const, endpoints: [`orion://${n.id}/executor/helios.graph.v1`], labels: { cores: "4", free_mib: "1880" }, leasedBy: WORKLOADS.filter((w) => w.nodeId === n.id).map((w) => w.id) },
  ]),
];

const logTemplates: [string, LogLine["level"], string][] = [
  ["helios-runner", "info", "tags-front tick p50 0.70 ms, p99 0.80 ms over 3600 frames"],
  ["helios-io", "info", "camera.raze-front.cam0 delivered GREY 1280x800 @ 60 fps, hardware pyramid level 1"],
  ["orion-node", "info", "status lane: 14 subjects, 0 persisted writes in the last minute"],
  ["atlas-agent", "info", "health ok · slot A confirmed · watchdog fed"],
  ["helios-runner", "warn", "tags-left-exp worker exceeded 32 MiB cap; restarting (attempt 5)"],
  ["helios-runner", "error", "tags-left-exp quarantined after 5 restarts; revision 1 restored"],
  ["photonvision-adapter", "warn", "PhotonVision clock offset 2.4 ms; detections timestamped with declared offset"],
];

export const LOGS: LogLine[] = Array.from({ length: 40 }, (_, i) => {
  const [unit, level, text] = logTemplates[i % logTemplates.length];
  const nodeId = unit === "photonvision-adapter" ? "pv-coproc" : unit.includes("left") || text.includes("left") ? "raze-left" : NODES[i % 4].id;
  return { ts: now - (40 - i) * 47_000, nodeId, unit, level, text };
});
