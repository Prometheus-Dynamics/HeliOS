// A simulated robot: the cluster a HeliOS UI would see on a real robot, with
// live readings. Camera feeds replay the robot video (frames and detections
// from the CM5 run). Swap this for the Orion/agent client without changing
// any screen.

import { CATALOG_BY_ID, defaults } from "./catalog";
import type { Camera, CameraControlInfo, ClusterNode, FeatureTrack, FeatureTracks, GraphDocument, GraphEdge, GraphNode, LogLine, Resource, Service, Slot, Stream, Workload } from "./model";
import { TRAIL_LENGTH, TrailBook, type CameraFeatures, type TrailPoint } from "../vision/features";
import type { MapPoints, MapPosedFrame, MapSnapshot, MapTag, MapTransform, MapView } from "../vision/map";
import { encodeBase64, type DenseMeshView, type DenseView } from "../vision/dense";
import { parseDenseDepth, parseDenseMesh, parseDenseTiming } from "../vision/dense";
import { scaleIntrinsics, type Intrinsics, type PoseView, type TagPose, type TagPoses } from "../vision/pose";

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
    inputs: { camera: { source: "camera", resourceId: cameraId, outputWidth: 640, outputHeight: 400, pyramid: 1 } },
    searchMode: "tracked",
    fullSearchEvery: 8,
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

/** A default template's graph (`core/defaults.ts`) as the mock draws it: one tracked detector
 * feeding the tag pose, the field pose, the live map and the dense map as the template has them,
 * or the feature tracks. */
export function defaultGraph(templateId: string, table: string): GraphDocument {
  if (templateId === "features-track") return featuresGraph();
  const aruco = templateId.startsWith("aruco");
  const map = templateId.includes("-map");
  const dense = templateId.endsWith("-dense");
  const nodes = [
    node("camera", "styx:camera", 0, 120),
    node("detector", "eidos:detectors.tracked_detect", 270, 100, { dictionary: aruco ? "aruco_4x4_50" : "apriltag_36h11" }),
    node("pose", "eidos:aruco.pose", 540, 0),
    ...(aruco ? [] : [node("field", "eidos:aruco.multi_tag_pose", 540, 160)]),
    ...(map ? [node("mapper", "eidos:mapping.live_map", 540, 320)] : []),
    ...(dense ? [node("dense", "eidos:dense.map", 810, 360)] : []),
    node("nt4", "helios:nt4", 1080, 80, { table }),
  ];
  const edges = [
    edge("camera", "frame", "detector", "frame"),
    edge("detector", "detections", "pose", "detections"),
    edge("detector", "refined_corners", "pose", "refined_corners"),
    edge("pose", "poses", "nt4", "value"),
    ...(aruco ? [] : [edge("detector", "detections", "field", "detections"), edge("detector", "refined_corners", "field", "refined_corners")]),
    ...(map ? [edge("camera", "frame", "mapper", "frame"), edge("detector", "detections", "mapper", "detections"), edge("detector", "refined_corners", "mapper", "refined_corners")] : []),
    ...(dense ? [edge("camera", "frame", "dense", "frame"), edge("mapper", "pose", "dense", "pose")] : []),
  ];
  return { format: "daedalus.graph", schema_version: 1, requires: [{ id: "styx.frames" }, { id: "eidos" }, { id: "helios" }], nodes, edges };
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
    inputs: { camera: { source: "camera", resourceId: "camera.raze-front.cam0", pyramid: 3 } },
    searchMode: "tracked",
    fullSearchEvery: 8,
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
    inputs: { camera: { source: "camera", resourceId: "camera.raze-front.cam0", pyramid: 3 } },
    searchMode: null,
    fullSearchEvery: null,
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

const mockLogs: LogLine[] = Array.from({ length: 40 }, (_, i) => {
  const [unit, level, text] = logTemplates[i % logTemplates.length];
  const nodeId = unit === "photonvision-adapter" ? "pv-coproc" : unit.includes("left") || text.includes("left") ? "raze-left" : NODES[i % 4].id;
  return { ts: now - (40 - i) * 47_000, nodeId, unit, level, text };
});
// As on a real board: systemd logs the same line twice in one millisecond (two units starting).
export const LOGS: LogLine[] = [...mockLogs, { ...mockLogs[mockLogs.length - 1] }];

// --- The live map (`eidos:mapping.live_map`), simulated -----------------------------------------
// A 5 x 3.6 m room: points on its walls and floor, a camera looping through it, six tags on the
// walls. The points are built once; `mockMapView` moves the camera and the keyframes.

const MOCK_ROOM = { halfX: 2.5, halfZ: 1.8, height: 2.4 };
/** Where the mock room sits in the field (its centre, metres): the mock map is aligned to the field layout. */
const MOCK_ORIGIN: [number, number, number] = [8.27, 0, 4.03];
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
    const p = [x + MOCK_ORIGIN[0], y + MOCK_ORIGIN[1], z + MOCK_ORIGIN[2]];
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

/** A rotation quaternion (w, x, y, z) and a vector. */
type Quat = { w: number; x: number; y: number; z: number };
type Vec = [number, number, number];

const sub = (a: Vec, b: Vec): Vec => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const cross = (a: Vec, b: Vec): Vec => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const unit = (a: Vec): Vec => {
  const n = Math.hypot(a[0], a[1], a[2]) || 1;
  return [a[0] / n, a[1] / n, a[2] / n];
};

function qmul(a: Quat, b: Quat): Quat {
  return {
    w: a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
    x: a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
    y: a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
    z: a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
  };
}

function qconj(q: Quat): Quat {
  return { w: q.w, x: -q.x, y: -q.y, z: -q.z };
}

/** `v` rotated by the unit quaternion `q`. */
function qrot(q: Quat, v: Vec): Vec {
  const tx = 2 * (q.y * v[2] - q.z * v[1]);
  const ty = 2 * (q.z * v[0] - q.x * v[2]);
  const tz = 2 * (q.x * v[1] - q.y * v[0]);
  return [v[0] + q.w * tx + (q.y * tz - q.z * ty), v[1] + q.w * ty + (q.z * tx - q.x * tz), v[2] + q.w * tz + (q.x * ty - q.y * tx)];
}

/**
 * The optical frame (x right, y down, z forward) of a camera at `position` looking at `target`,
 * with the world's y up: the camera stays level.
 */
function opticalLookAt(position: Vec, target: Vec): Quat {
  const z = unit(sub(target, position));
  const x = unit(cross(z, [0, 1, 0]));
  const y = cross(z, x);
  const [m00, m01, m02] = [x[0], y[0], z[0]];
  const [m10, m11, m12] = [x[1], y[1], z[1]];
  const [m20, m21, m22] = [x[2], y[2], z[2]];
  const trace = m00 + m11 + m22;
  if (trace > 0) {
    const s = Math.sqrt(trace + 1) * 2;
    return { w: 0.25 * s, x: (m21 - m12) / s, y: (m02 - m20) / s, z: (m10 - m01) / s };
  }
  if (m00 > m11 && m00 > m22) {
    const s = Math.sqrt(1 + m00 - m11 - m22) * 2;
    return { w: (m21 - m12) / s, x: 0.25 * s, y: (m01 + m10) / s, z: (m02 + m20) / s };
  }
  if (m11 > m22) {
    const s = Math.sqrt(1 + m11 - m00 - m22) * 2;
    return { w: (m02 - m20) / s, x: (m01 + m10) / s, y: 0.25 * s, z: (m12 + m21) / s };
  }
  const s = Math.sqrt(1 + m22 - m00 - m11) * 2;
  return { w: (m10 - m01) / s, x: (m02 + m20) / s, y: (m12 + m21) / s, z: 0.25 * s };
}

/** The room's centre, where the camera looks (a point at the height of its view). */
const MOCK_TARGET: Vec = [MOCK_ORIGIN[0], 1.0, MOCK_ORIGIN[2]];

/** A camera at `position`, turned to look at the room's centre. */
function lookAtCentre(position: [number, number, number]): MapTransform {
  const [px, py, pz] = position;
  const q = opticalLookAt([px, py, pz], MOCK_TARGET);
  return { translation: { x: px, y: py, z: pz }, rotation: q };
}

/** The camera's path: an ellipse through the room, one lap every ~210 ticks. */
function mockCameraAt(tick: number): [number, number, number] {
  const phase = tick * 0.03;
  return [MOCK_ORIGIN[0] + 1.6 * Math.cos(phase), 1.1 + 0.04 * Math.sin(2 * phase), MOCK_ORIGIN[2] + 1.0 * Math.sin(phase)];
}

/** The six mock tags on the walls. `placed` is the frame the map placed it at (0: the first tag). */
const MOCK_TAGS: { id: number; position: [number, number, number]; yaw: number; placed: number }[] = [
  { id: 101, position: [-MOCK_ROOM.halfX, 1.2, -0.9], yaw: Math.PI / 2, placed: 0 },
  { id: 102, position: [-MOCK_ROOM.halfX, 1.2, 0.9], yaw: Math.PI / 2, placed: 0 },
  { id: 103, position: [MOCK_ROOM.halfX, 1.2, -0.6], yaw: -Math.PI / 2, placed: 0 },
  { id: 104, position: [0.6, 1.2, -MOCK_ROOM.halfZ], yaw: 0, placed: 0 },
  { id: 141, position: [-0.6, 1.2, MOCK_ROOM.halfZ], yaw: Math.PI, placed: 14 },
  { id: 142, position: [1.2, 0.9, MOCK_ROOM.halfZ], yaw: Math.PI, placed: 37 },
];

function mockTagPose(tag: (typeof MOCK_TAGS)[number]): MapTransform {
  const [x, y, z] = [tag.position[0] + MOCK_ORIGIN[0], tag.position[1] + MOCK_ORIGIN[1], tag.position[2] + MOCK_ORIGIN[2]];
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
    aligned: true,
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
      alignment: { aligned: true, matched_tags: 6, residual_m: 0.012 },
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

// --- The dense mapper and the tag poses (`?mock=1`) -----------------------------------------
// The simulated robot's dense mapper: a mesh of the room (each second) and the depth the camera
// sees of it (every two seconds), each in the device's wire format, so the same parsers read the
// mock as the device. The tag poses are the room's tags as the camera sees them.

/** The simulated camera's intrinsics: 1280 x 800, about 64 degrees across. */
export const MOCK_INTRINSICS: Intrinsics = { fx: 1020, fy: 1020, cx: 640, cy: 400, width: 1280, height: 800 };
/** The depth preview: half the sweep, 0.6 to 4 m (the room's range, so its gradient reads). */
const MOCK_DEPTH = { width: 128, height: 80, near: 0.6, far: 4 };
const MOCK_MESH_EVERY = MOCK_MAP_HZ;
const MOCK_DEPTH_EVERY = MOCK_MAP_HZ * 2;

/** Accumulates a mesh: whole grids of quads, two triangles each, with one normal and grey per grid. */
class MockMesh {
  readonly positions: number[] = [];
  readonly normals: number[] = [];
  readonly grey: number[] = [];
  readonly indices: number[] = [];

  /** A grid of `nu` x `nv` quads over `at(s, t)` for s, t in [0, 1]. */
  grid(at: (s: number, t: number) => Vec, normal: Vec, nu: number, nv: number, grey: number) {
    const base = this.grey.length;
    for (let j = 0; j <= nv; j++)
      for (let i = 0; i <= nu; i++) {
        const p = at(i / nu, j / nv);
        this.positions.push(p[0], p[1], p[2]);
        this.normals.push(normal[0], normal[1], normal[2]);
        this.grey.push(grey);
      }
    for (let j = 0; j < nv; j++)
      for (let i = 0; i < nu; i++) {
        const a = base + j * (nu + 1) + i;
        const d = a + nu + 1;
        this.indices.push(a, d, a + 1, a + 1, d, d + 1);
      }
  }
}

/** The room's surfaces the camera has mapped: the floor, the four walls and a table. */
function mockRoomMesh(): MockMesh {
  const { halfX: hx, halfZ: hz, height: h } = MOCK_ROOM;
  const mesh = new MockMesh();
  mesh.grid((s, t) => [-hx + 2 * hx * s, 0, -hz + 2 * hz * t], [0, 1, 0], 24, 18, 96);
  mesh.grid((s, t) => [-hx + 2 * hx * s, h * t, -hz], [0, 0, 1], 24, 8, 170);
  mesh.grid((s, t) => [-hx + 2 * hx * s, h * t, hz], [0, 0, -1], 24, 8, 150);
  mesh.grid((s, t) => [-hx, h * t, -hz + 2 * hz * s], [1, 0, 0], 18, 8, 176);
  mesh.grid((s, t) => [hx, h * t, -hz + 2 * hz * s], [-1, 0, 0], 18, 8, 160);
  // A table: 1.0 x 0.75 m, 0.75 m high, at the room's middle.
  const [tx0, tx1, tz0, tz1, th] = [-1.3, -0.3, 0.225, 0.975, 0.75];
  mesh.grid((s, t) => [tx0 + (tx1 - tx0) * s, th, tz0 + (tz1 - tz0) * t], [0, 1, 0], 6, 4, 224);
  mesh.grid((s, t) => [tx1, th * t, tz0 + (tz1 - tz0) * s], [1, 0, 0], 4, 3, 184);
  mesh.grid((s, t) => [tx0, th * t, tz0 + (tz1 - tz0) * s], [-1, 0, 0], 4, 3, 184);
  mesh.grid((s, t) => [tx0 + (tx1 - tx0) * s, th * t, tz0], [0, 0, -1], 6, 3, 190);
  mesh.grid((s, t) => [tx0 + (tx1 - tx0) * s, th * t, tz1], [0, 0, 1], 6, 3, 190);
  return mesh;
}

/** The mesh at tick `tick`, in the device's wire format (base64 of little-endian values). */
function mockMeshWire(tick: number): Record<string, unknown> {
  const mesh = mockRoomMesh();
  const [ox, oy, oz] = MOCK_ORIGIN;
  for (let i = 0; i < mesh.positions.length; i += 3) {
    mesh.positions[i] += ox;
    mesh.positions[i + 1] += oy;
    mesh.positions[i + 2] += oz;
  }
  const vertices = mesh.grey.length;
  const f32 = (values: number[]) => new Uint8Array(new Float32Array(values).buffer);
  const u32 = (values: number[]) => new Uint8Array(new Uint32Array(values).buffer);
  return {
    seq: Math.floor(tick / MOCK_MESH_EVERY) + 1,
    frame: tick,
    t_ns: Math.round((tick * 1e9) / MOCK_MAP_HZ),
    keyframes: Math.floor(tick / 16) + 1,
    submaps: 1,
    voxel_m: 0.05,
    vertices,
    triangles: mesh.indices.length / 3,
    positions: encodeBase64(f32(mesh.positions)),
    normals: encodeBase64(f32(mesh.normals)),
    grey: encodeBase64(new Uint8Array(mesh.grey)),
    indices: encodeBase64(u32(mesh.indices)),
  };
}

/** The depth the camera sees of the room at `pose`: each pixel's ray walked to the room's surface. */
function mockDepthWire(tick: number, pose: MapTransform): Record<string, unknown> {
  const { width, height, near, far } = MOCK_DEPTH;
  const k = scaleIntrinsics(MOCK_INTRINSICS, width, height);
  const { halfX: hx, halfZ: hz, height: h } = MOCK_ROOM;
  const [ox, oy, oz] = MOCK_ORIGIN;
  const q = pose.rotation;
  // The camera in the room's own frame, where the box is.
  const eye: Vec = [pose.translation.x - ox, pose.translation.y - oy, pose.translation.z - oz];
  const lo: Vec = [-hx, 0, -hz];
  const hi: Vec = [hx, h, hz];
  const bytes = new Uint8Array(width * height);
  for (let v = 0; v < height; v++)
    for (let u = 0; u < width; u++) {
      const dir = unit([(u + 0.5 - k.cx) / k.fx, (v + 0.5 - k.cy) / k.fy, 1]);
      const world = qrot(q, dir);
      // The ray leaves the box through its nearest face: the smallest positive exit distance.
      let t = Infinity;
      let ceiling = false;
      for (let axis = 0; axis < 3; axis++) {
        const d = world[axis];
        if (Math.abs(d) < 1e-9) continue;
        const exit = d > 0 ? (hi[axis] - eye[axis]) / d : (lo[axis] - eye[axis]) / d;
        if (exit > 0 && exit < t) {
          t = exit;
          ceiling = axis === 1 && d > 0;
        }
      }
      // The sweep never maps the ceiling, so pixels that see it stay unknown (0).
      const z = t * dir[2];
      if (!Number.isFinite(t) || ceiling || z <= 0 || z > far) continue;
      const scaled = (1 / z - 1 / far) / (1 / near - 1 / far);
      bytes[v * width + u] = Math.min(255, Math.max(1, Math.round(255 * scaled)));
    }
  return { frame: tick, width, height, inverse_depth: encodeBase64(bytes) };
}

/** Each output is made once per its period; `at` is when it was made (its age starts there). */
let meshMemo: { key: number; at: number; view: DenseMeshView } | null = null;
let depthMemo: { key: number; at: number; view: NonNullable<DenseView["depth"]> } | null = null;

/** The mock robot's dense outputs at tick `tick`: a mesh each second, a depth each two seconds. */
export function mockDenseView(tick: number): DenseView {
  const meshKey = Math.floor(tick / MOCK_MESH_EVERY);
  if (!meshMemo || meshMemo.key !== meshKey) {
    const view = parseDenseMesh(mockMeshWire(meshKey * MOCK_MESH_EVERY));
    if (view) meshMemo = { key: meshKey, at: performance.now(), view };
  }
  const depthKey = Math.floor(tick / MOCK_DEPTH_EVERY);
  if (!depthMemo || depthMemo.key !== depthKey) {
    const depthTick = depthKey * MOCK_DEPTH_EVERY;
    const view = parseDenseDepth(mockDepthWire(depthTick, lookAtCentre(mockCameraAt(depthTick))));
    if (view) depthMemo = { key: depthKey, at: performance.now(), view };
  }
  const timing = parseDenseTiming({
    frame: tick,
    t_ns: Math.round((tick * 1e9) / MOCK_MAP_HZ),
    state: "running",
    processed: Math.floor(tick / 16),
    keyframes: Math.floor(tick / 16),
    dropped_keyframes: 0,
    pending: 0,
    keyframe_rate: 0.5,
    sweep_ms: 3.1 + (tick % 5) * 0.05,
    fill_ms: 1.2,
    integrate_ms: 2.4,
    mesh_ms: 0.8,
    budget_ms: 2,
    spacing: 16,
    cost_ms: 5.2,
    arrived: Math.floor(tick / 16),
    dropped_blocks: 0,
    submaps: 1,
    blocks: 1200,
    volume_bytes: 4_200_000,
  });
  return { mesh: meshMemo?.view ?? null, depth: depthMemo?.view ?? null, timing, depthAt: depthMemo?.at ?? 0, meshAt: meshMemo?.at ?? 0 };
}

/** The tag poses at tick `tick`: the tags the camera faces and sees, camera-from-tag, and the camera in the field. */
export function mockPoseView(tick: number): PoseView {
  const camera = lookAtCentre(mockCameraAt(tick));
  const q = camera.rotation;
  const inv = qconj(q);
  const eye: Vec = [camera.translation.x, camera.translation.y, camera.translation.z];
  const tanX = MOCK_INTRINSICS.width / (2 * MOCK_INTRINSICS.fx);
  const tags: TagPose[] = [];
  for (const tag of MOCK_TAGS) {
    const world = mockTagPose(tag);
    const at: Vec = [world.translation.x, world.translation.y, world.translation.z];
    const normal = qrot({ w: world.rotation.w, x: world.rotation.x, y: world.rotation.y, z: world.rotation.z }, [0, 0, 1]);
    const toCamera = sub(eye, at);
    if (normal[0] * toCamera[0] + normal[1] * toCamera[1] + normal[2] * toCamera[2] <= 0) continue;
    const inCamera = qrot(inv, sub(at, eye));
    if (inCamera[2] < 0.2 || Math.abs(inCamera[0] / inCamera[2]) > tanX) continue;
    const rot = qmul(inv, { w: world.rotation.w, x: world.rotation.x, y: world.rotation.y, z: world.rotation.z });
    tags.push({
      id: tag.id,
      pose: { translation: { x: inCamera[0], y: inCamera[1], z: inCamera[2] }, rotation: rot },
      errorPx: 0.3,
      ambiguity: 0.1,
    });
  }
  const solutions: TagPoses = { status: "calibrated", tags };
  return {
    tags: solutions,
    field: { valid: true, status: "calibrated", cameraInField: camera, robotInField: null, rmsPx: 0.4, inliers: tags.length, ambiguity: 0.05 },
  };
}
