// A simulated robot: the cluster a HeliOS UI would see on a real robot, with
// live readings. Camera feeds replay the robot video (frames and detections
// from the CM5 run). Swap this for the Orion/agent client without changing
// any screen.

import { CATALOG_BY_ID, defaults } from "./catalog";
import type { Camera, CameraControlInfo, ClusterNode, FeatureTrack, FeatureTracks, GraphDocument, GraphEdge, GraphNode, LogLine, Resource, Service, Slot, Stream, Workload } from "./model";
import { TRAIL_LENGTH, TrailBook, type CameraFeatures, type TrailPoint } from "../vision/features";
import type { MapPoints, MapPosedFrame, MapSnapshot, MapTag, MapTransform, MapView } from "../vision/map";

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

/**
 * The OV9782's controls as its camera service lists them (Styx's native backend, at the revision
 * Backend/Cargo.lock pins): the same names, ranges, defaults and flags a device reports.
 */
function ov9782Controls(values: { exposureUs: number; gain: number; fps: number; autoExposure: boolean }): CameraControlInfo[] {
  const control = (c: Pick<CameraControlInfo, "id" | "name" | "kind"> & Partial<CameraControlInfo>): CameraControlInfo => ({
    default: null,
    value: null,
    standard: null,
    writable: true,
    readOnly: false,
    persisted: false,
    ...c,
  });
  const crop = (id: number, name: string): CameraControlInfo => control({ id, name, kind: "rectangle", rect: { x: 0, y: 0, width: 0, height: 0 } });
  return [
    control({ id: 0xf4000001, name: "exposure_time_us", kind: "uint", min: 9, max: 476066, default: 10000, step: 1, value: values.exposureUs, standard: "exposure_us" }),
    control({ id: 0xf4000002, name: "gain", kind: "float", min: 1, max: 15.94, default: 1, step: 0.0625, value: values.gain, standard: "gain" }),
    control({ id: 0xf4000003, name: "frame_duration_us", kind: "uint", min: 3209, max: 476190, default: 16667, step: 1, value: Math.round(1_000_000 / values.fps) }),
    control({ id: 0xf4000004, name: "frame_rate", kind: "float", min: 2.1, max: 311.7, default: 30, step: 0.001, value: values.fps, standard: "fps" }),
    control({ id: 0xf4000005, name: "ae_enable", kind: "bool", default: true, value: values.autoExposure, standard: "ae" }),
    control({ id: 0xf4000006, name: "exposure_value", kind: "float", min: -8, max: 8, default: 0, step: 0.1, value: 0, standard: "ev" }),
    control({ id: 0xf4000007, name: "awb_enable", kind: "bool", default: true, value: true, standard: "awb" }),
    control({ id: 0xf4000008, name: "colour_temperature", kind: "uint", min: 1000, max: 20000, default: 0, step: 1, value: 5600, standard: "colour_temperature" }),
    control({ id: 0xf4000009, name: "red_gain", kind: "float", min: 0, max: 8, default: 0, step: 0.01, value: 1.6, standard: "red_gain" }),
    control({ id: 0xf400000a, name: "blue_gain", kind: "float", min: 0, max: 8, default: 0, step: 0.01, value: 1.9, standard: "blue_gain" }),
    control({ id: 0xf4000011, name: "ae_flicker_mode", kind: "int", min: 0, max: 3, default: 3, step: 1, value: 3, menu: ["off", "50 Hz", "60 Hz", "auto"] }),
    control({ id: 0xf4000013, name: "ae_deflicker_mode", kind: "int", min: 0, max: 2, default: 2, step: 1, value: 2, menu: ["off", "on", "with flicker avoidance"] }),
    control({ id: 0xf4000012, name: "ae_flicker_detected", kind: "int", min: 0, max: 1_000_000, default: 0, step: 1, value: 10_000, readOnly: true, writable: false }),
    control({ id: 0xf4000010, name: "ae_state", kind: "int", min: 0, max: 2, default: 1, step: 1, value: 2, readOnly: true, writable: false, menu: ["inactive", "searching", "converged"] }),
    // AE limits, modes and metering region: native controls, addressed by name (0 = no limit).
    control({ id: 0xf4000020, name: "ae_exposure_max_us", kind: "uint", min: 0, max: 476066, default: 0, step: 1, value: 0 }),
    control({ id: 0xf4000021, name: "ae_gain_max", kind: "float", min: 0, max: 15.94, default: 0, step: 0.0625, value: 0 }),
    control({ id: 0xf4000022, name: "ae_exposure_mode", kind: "int", min: 0, max: 2, default: 0, step: 1, value: 0, menu: ["normal", "short", "long"] }),
    control({ id: 0xf4000023, name: "ae_metering_mode", kind: "int", min: 0, max: 2, default: 0, step: 1, value: 0, menu: ["centre-weighted", "spot", "matrix"] }),
    control({ id: 0xf4000024, name: "ae_metering_region", kind: "rectangle", rect: { x: 0, y: 0, width: 0, height: 0 } }),
    // Sensor orientation: Styx's service controls (SERVICE_HFLIP / VFLIP / ROTATION); setting one
    // restarts the capture once.
    control({ id: 0xf5f00002, name: "hflip", kind: "bool", default: false, value: false }),
    control({ id: 0xf5f00003, name: "vflip", kind: "bool", default: false, value: false }),
    control({ id: 0xf5f00004, name: "rotation", kind: "int", min: 0, max: 180, default: 0, step: 180, value: 0 }),
    crop(0xf4000100, "output_crop"),
    ...Array.from({ length: 15 }, (_, i) => crop(0xf4000101 + i, `region_crop_${i + 1}`)),
  ];
}

function cam(id: string, name: string, nodeId: string, mount: string, offset: number, extra: Partial<Camera> = {}): Camera {
  const settings = { width: 1280, height: 800, fps: 60, format: "GREY", exposureUs: 2200, autoExposure: false, gain: 4, pyramid: true, roi: null };
  return {
    resourceId: id,
    name,
    nodeId,
    sensor: "OV9782 · global shutter",
    backend: "styx native · PiSP",
    mount,
    settings,
    controls: ov9782Controls(settings),
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
    controls: [],
    controlsError: "PhotonVision owns this camera's settings",
  }),
  cam("camera.driver-laptop.usb0", "Laptop webcam", "driver-laptop", "Driver station", 300, {
    sensor: "UVC 1080p",
    backend: "styx uvc",
    stats: { fps: 30, dropped: 0, latencyMs: 41, cpuMsPerFrame: 1.2 },
    settings: { width: 1280, height: 720, fps: 30, format: "MJPEG", exposureUs: 16000, autoExposure: true, gain: 1, pyramid: false, roi: null },
    controls: [],
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
    cpu: null,
    cpuApplied: null,
    cpuError: null,
    perf: { tickP50Ms: 0.7, tickP99Ms: 0.8, fps: 60, memMiB: 8.9, nodeMs: { ...tagNodeMs }, tickHistory: history(0.7, 0.05) },
  };
}

/** Mock feature tracks: 24 slots, each a track that lives 25 ticks; then its id ends and a new one starts. */
const MOCK_SLOTS = 24;
const MOCK_LIFE = 25;
/** The mock tracks' frame: a 1280x800 sensor mode, so the overlay scales them as it does detections. */
const MOCK_FRAME = { w: 1280, h: 800 };

/** The mock `tracks` value at replay index `index`: ids are never reused, and positions drift on loops. */
export function mockFeatureTracks(index: number): FeatureTracks {
  const tracks: FeatureTrack[] = [];
  for (let slot = 0; slot < MOCK_SLOTS; slot++) {
    const phase = index + slot * 3;
    const cycle = Math.floor(phase / MOCK_LIFE);
    const age = phase % MOCK_LIFE;
    const radius = 120 + (slot % 6) * 70;
    const angle = 0.04 * index + slot;
    tracks.push({
      id: cycle * MOCK_SLOTS + slot + 1,
      x: MOCK_FRAME.w / 2 + radius * Math.cos(angle),
      y: MOCK_FRAME.h / 2 + 0.6 * radius * Math.sin(angle),
      age,
      status: age === 0 ? "new" : "tracked",
    });
  }
  // A slot restarts (its old id ends) on the ticks where its age is 0, after the first tick.
  const fresh = tracks.filter((t) => t.status === "new").length;
  const lost = index > 0 ? fresh : 0;
  return { tracks, summary: { count: tracks.length, tracked: tracks.length - fresh, new: fresh, lost }, frame: MOCK_FRAME };
}

/** A camera's mock feature tracks at replay index `index`, with the trails of the last ticks (as the live store keeps them). */
export function mockCameraFeatures(index: number): CameraFeatures {
  const book = new TrailBook();
  let trails = new Map<number, TrailPoint[]>();
  for (let back = TRAIL_LENGTH - 1; back >= 0; back--) trails = book.advance(mockFeatureTracks(Math.max(0, index - back)).tracks);
  const tracks = mockFeatureTracks(index);
  return { tracks, trails, signature: JSON.stringify(tracks) };
}

function featuresGraph(): GraphDocument {
  return {
    format: "daedalus.graph",
    schema_version: 1,
    requires: [{ id: "styx.frames" }, { id: "eidos" }, { id: "helios" }],
    nodes: [node("camera", "styx:camera", 0, 60), node("tracks", "eidos:features.track", 270, 40), node("stream", "helios:stream", 540, 40, { name: "tracks" })],
    edges: [edge("camera", "frame", "tracks", "frame"), edge("tracks", "tracks", "stream", "value")],
  };
}

/** The live map's graph: the tracked AprilTag detector feeding Eidos's live mapper (`eidos:mapping.live_map`). */
function liveMapGraph(): GraphDocument {
  return {
    format: "daedalus.graph",
    schema_version: 1,
    requires: [{ id: "styx.frames" }, { id: "eidos" }, { id: "helios" }],
    nodes: [
      node("camera", "styx:camera", 0, 80),
      node("detector", "eidos:detectors.tracked_detect", 270, 40, { dictionary: "apriltag_36h11" }),
      node("mapper", "eidos:mapping.live_map", 540, 40),
      node("stream", "helios:stream", 810, 40, { name: "map" }),
    ],
    edges: [
      edge("camera", "frame", "detector", "frame"),
      edge("camera", "frame", "mapper", "frame"),
      edge("detector", "detections", "mapper", "detections"),
      edge("detector", "rejected", "mapper", "rejected"),
      edge("detector", "refined_corners", "mapper", "refined_corners"),
      edge("mapper", "pose", "stream", "value"),
    ],
  };
}

export const WORKLOADS: Workload[] = [
  tagWorkload("tags-front", "AprilTags · front", "raze-front", "camera.raze-front.cam0", "/helios/front"),
  {
    id: "live-map-front",
    name: "Live map · front",
    nodeId: "raze-front",
    revision: 1,
    state: "running",
    restarts: 0,
    bindings: { camera: "camera.raze-front.cam0" },
    graph: liveMapGraph(),
    outputs: ["stream.live-map-front.pose", "stream.live-map-front.snapshot", "stream.live-map-front.timing"],
    cpu: null,
    cpuApplied: null,
    cpuError: null,
    perf: { tickP50Ms: 4.2, tickP99Ms: 6.1, fps: 16, memMiB: 14.6, nodeMs: { camera: 0.02, detector: 1.1, mapper: 2.9 }, tickHistory: history(4.2, 0.4) },
  },
  {
    id: "features-front",
    name: "Feature tracks · front",
    nodeId: "raze-front",
    revision: 1,
    state: "running",
    restarts: 0,
    bindings: { camera: "camera.raze-front.cam0" },
    graph: featuresGraph(),
    outputs: ["stream.features-front.tracks"],
    cpu: null,
    cpuApplied: null,
    cpuError: null,
    perf: { tickP50Ms: 0.3, tickP99Ms: 0.4, fps: 60, memMiB: 2.1, nodeMs: { camera: 0.02, tracks: 0.2, stream: 0.006 }, tickHistory: history(0.3, 0.03) },
  },
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
  ...WORKLOADS.filter((w) => w.state === "running" && w.outputs.includes(`stream.${w.id}.poses`)).map((w) => ({
    id: `stream.${w.id}.poses`,
    name: `${w.name} · poses`,
    schema: "poses" as const,
    producer: w.id,
    rateHz: 60,
    latencyMs: 1.9,
    bridges: ["nt4", "api"],
  })),
  { id: "stream.features-front.tracks", name: "Feature tracks · front · tracks", schema: "telemetry", producer: "features-front", rateHz: 60, latencyMs: 1.1, bridges: ["api"] },
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

// --- The live map (`eidos:mapping.live_map`), simulated -----------------------------------------
// A 5 x 3.6 m room: points on its walls and floor, a camera looping through it, six tags on the
// walls. The points are built once; `mockMapView` moves the camera and the keyframes.

const MOCK_ROOM = { halfX: 2.5, halfZ: 1.8, height: 2.4 };
/** The mock map's ticks per second (the pane's frame rate). */
const MOCK_MAP_HZ = 16;

/** A small seeded generator, so the mock room is the same on every load. */
function seeded(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function buildMockPoints(): MapPoints {
  const rand = seeded(20260);
  const count = 2600;
  const xyz = new Float32Array(count * 3);
  const ids = new Uint32Array(count);
  const observations = new Uint16Array(count);
  const grey = new Uint8Array(count);
  const { halfX, halfZ, height } = MOCK_ROOM;
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < count; i++) {
    let x: number;
    let y: number;
    let z: number;
    if (rand() < 0.2) {
      // Floor.
      x = (rand() * 2 - 1) * halfX;
      z = (rand() * 2 - 1) * halfZ;
      y = 0;
    } else {
      // Walls: the long ones (z = ±halfZ) and the short ones (x = ±halfX).
      y = rand() * height;
      const wall = Math.floor(rand() * 4);
      if (wall < 2) {
        x = (rand() * 2 - 1) * halfX;
        z = wall === 0 ? -halfZ : halfZ;
      } else {
        z = (rand() * 2 - 1) * halfZ;
        x = wall === 2 ? -halfX : halfX;
      }
    }
    const p = [x, y, z];
    xyz.set(p, i * 3);
    p.forEach((v, axis) => {
      min[axis] = Math.min(min[axis], v);
      max[axis] = Math.max(max[axis], v);
    });
    ids[i] = i + 1;
    observations[i] = 2 + Math.floor(rand() * 18);
    grey[i] = 70 + Math.floor(rand() * 150);
  }
  return { count, min_xyz: min, max_xyz: max, ids, xyz, observations, grey };
}

let mockPoints: MapPoints | null = null;

/** A camera at `position`, turned to face the room's centre (yaw only; it stays level). */
function lookAtCentre(position: [number, number, number]): MapTransform {
  const [px, py, pz] = position;
  const yaw = Math.atan2(-px, -pz);
  return { translation: { x: px, y: py, z: pz }, rotation: { w: Math.cos(yaw / 2), x: 0, y: Math.sin(yaw / 2), z: 0 } };
}

/** The camera's path: an ellipse through the room, one lap every ~210 ticks. */
function mockCameraAt(tick: number): [number, number, number] {
  const phase = tick * 0.03;
  return [1.6 * Math.cos(phase), 1.1 + 0.04 * Math.sin(2 * phase), 1.0 * Math.sin(phase)];
}

/** The six mock tags on the walls. `placed` is the frame the map placed it at (0: the first tag). */
const MOCK_TAGS: { id: number; position: [number, number, number]; yaw: number; placed: number }[] = [
  { id: 1, position: [-MOCK_ROOM.halfX, 1.2, -0.9], yaw: Math.PI / 2, placed: 0 },
  { id: 2, position: [-MOCK_ROOM.halfX, 1.2, 0.9], yaw: Math.PI / 2, placed: 0 },
  { id: 3, position: [MOCK_ROOM.halfX, 1.2, -0.6], yaw: -Math.PI / 2, placed: 0 },
  { id: 4, position: [0.6, 1.2, -MOCK_ROOM.halfZ], yaw: 0, placed: 0 },
  { id: 41, position: [-0.6, 1.2, MOCK_ROOM.halfZ], yaw: Math.PI, placed: 14 },
  { id: 42, position: [1.2, 0.9, MOCK_ROOM.halfZ], yaw: Math.PI, placed: 37 },
];

function mockTagPose(tag: (typeof MOCK_TAGS)[number]): MapTransform {
  const [x, y, z] = tag.position;
  return { translation: { x, y, z }, rotation: { w: Math.cos(tag.yaw / 2), x: 0, y: Math.sin(tag.yaw / 2), z: 0 } };
}

/** The mock live-map view at tick `tick`: the camera has moved `tick` ticks along its path. */
export function mockMapView(tick: number): MapView {
  mockPoints ??= buildMockPoints();
  const position = mockCameraAt(tick);
  const sigmaPos = 0.08 + 0.02 * Math.sin(tick * 0.09);
  const trajectory: MapPosedFrame[] = [];
  for (let i = Math.max(0, tick - 240); i <= tick; i += 2) {
    trajectory.push({
      frame: i,
      pose: lookAtCentre(mockCameraAt(i)),
      sigma: { position_m: [0.08, 0.08, 0.06], rotation_deg: [2, 2, 1] },
      keyframe: i % 16 === 0,
    });
  }
  const snapshot: MapSnapshot = {
    seq: Math.floor(tick / (MOCK_MAP_HZ / 2)),
    run_id: 7,
    tag_size_m: 0.1651,
    aligned: false,
    estimated: true,
    trajectory,
    keyframes: trajectory.filter((f) => f.keyframe).map((f) => f.frame),
    points: mockPoints,
    tags: MOCK_TAGS.map((tag) => ({ id: tag.id, pose: mockTagPose(tag), placed: tag.placed }) satisfies MapTag),
  };
  const map_ms = 1.6 + (tick % 5) * 0.04;
  const t_ns = Math.round((tick * 1e9) / MOCK_MAP_HZ);
  return {
    pose: {
      frame: tick,
      t_ns,
      state: "tracking",
      reason: "tags",
      valid: true,
      estimate: {
        reference_from_camera: lookAtCentre(position),
        covariance: [],
        sigma: { position_m: [sigmaPos, sigmaPos * 1.1, sigmaPos * 0.8], rotation_deg: [4, 3.5, 1.2] },
        source: "tags",
        rms_px: 0.42,
        points: 0,
      },
      calibration: { estimated: true, rms_px: 0.4, views: 107, frames: 150 },
      alignment: { aligned: false, matched_tags: 0, residual_m: 0 },
    },
    snapshot,
    timing: {
      frame: tick,
      t_ns,
      frame_ms: 3.8 + (tick % 7) * 0.05,
      map_ms,
      stages: [
        { name: "track", ms: 0.6 },
        { name: "tag detect", ms: 0.9 },
        { name: "solve", ms: 0.8 + (tick % 5) * 0.02 },
        { name: "map", ms: map_ms },
      ],
    },
  };
}
