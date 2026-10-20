// The live map (`eidos:mapping.live_map`): its pose, snapshot and timing outputs as the
// UI reads them. Byte buffers arrive as JSON arrays of byte values (`to_plain_json`); they are
// decoded here into typed arrays once, not per frame. The mapper's pose still jumps and drifts
// without tags, so nothing here feeds the robot's NetworkTables topics: this is for watching.

export type MapTrackingState = "bootstrapping" | "tracking" | "coasting" | "lost" | "relocalized";

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

export interface Quat {
  w: number;
  x: number;
  y: number;
  z: number;
}

/** A rigid transform: `reference_from_camera` and the like. */
export interface MapTransform {
  translation: Vec3;
  rotation: Quat;
}

/** The model's 1-sigma uncertainty of a pose. Eidos warns it is optimistic until calibrated. */
export interface MapSigma {
  position_m: number[];
  rotation_deg: number[];
}

export interface MapEstimate {
  reference_from_camera: MapTransform;
  covariance: number[];
  sigma: MapSigma;
  source: string;
  rms_px: number;
  points: number;
}

export interface MapPose {
  frame: number;
  t_ns: number;
  state: MapTrackingState;
  reason: string;
  valid: boolean;
  /** Present when `valid`: the frame's pose and how it was found. */
  estimate: MapEstimate | null;
  calibration: { estimated: boolean; rms_px: number; views: number; frames: number };
  alignment: { aligned: boolean; matched_tags: number; residual_m: number };
}

/** The packed point buffer, decoded: ids (u32), xyz (f32 x3), observations (u16), grey (u8). */
export interface MapPoints {
  count: number;
  min_xyz: number[];
  max_xyz: number[];
  ids: Uint32Array;
  xyz: Float32Array;
  observations: Uint16Array;
  grey: Uint8Array;
}

export interface MapPosedFrame {
  frame: number;
  pose: MapTransform;
  sigma: MapSigma;
  keyframe: boolean;
}

export interface MapTag {
  id: number;
  pose: MapTransform;
  /** How the tag is placed: non-zero when it is placed by the layout or the first tag. */
  placed: number;
}

export interface MapSnapshot {
  seq: number;
  run_id: number;
  tag_size_m: number;
  aligned: boolean;
  estimated: boolean;
  trajectory: MapPosedFrame[];
  keyframes: number[];
  points: MapPoints;
  tags: MapTag[];
}

export interface MapTiming {
  frame: number;
  t_ns: number;
  frame_ms: number;
  map_ms: number;
  stages: { name: string; ms: number }[];
}

type Json = Record<string, unknown>;
const isObject = (value: unknown): value is Json => typeof value === "object" && value !== null;
const num = (value: unknown, fallback = 0): number => (typeof value === "number" && Number.isFinite(value) ? value : fallback);
const list = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);
const numbers = (value: unknown): number[] => list(value).map((v) => num(v));

/** A byte buffer as bytes: a JSON array of byte values, or base64 text (defensive). */
export function bytesOf(value: unknown): Uint8Array {
  if (Array.isArray(value)) return Uint8Array.from(value, (v) => num(v) & 0xff);
  if (typeof value === "string") {
    try {
      const text = atob(value);
      return Uint8Array.from(text, (ch) => ch.charCodeAt(0));
    } catch {
      return new Uint8Array(0);
    }
  }
  return new Uint8Array(0);
}

function transformOf(value: unknown): MapTransform {
  const t = isObject(value) && isObject(value.translation) ? value.translation : {};
  const q = isObject(value) && isObject(value.rotation) ? value.rotation : {};
  return {
    translation: { x: num(t.x), y: num(t.y), z: num(t.z) },
    rotation: { w: num(q.w, 1), x: num(q.x), y: num(q.y), z: num(q.z) },
  };
}

function sigmaOf(value: unknown): MapSigma {
  const s = isObject(value) ? value : {};
  return { position_m: numbers(s.position_m), rotation_deg: numbers(s.rotation_deg) };
}

/** The pose output (`eidos:map_pose`), or null when the value is not one. */
export function parseMapPose(value: unknown): MapPose | null {
  if (!isObject(value) || typeof value.state !== "string") return null;
  const valid = value.valid === true;
  const p = isObject(value.pose) ? value.pose : {};
  const estimate: MapEstimate | null = valid
    ? {
        reference_from_camera: transformOf(p.reference_from_camera),
        covariance: numbers(p.covariance),
        sigma: sigmaOf(p.sigma),
        source: typeof p.source === "string" ? p.source : "",
        rms_px: num(p.rms_px),
        points: num(p.points),
      }
    : null;
  const calibration = isObject(value.calibration) ? value.calibration : {};
  const alignment = isObject(value.alignment) ? value.alignment : {};
  return {
    frame: num(value.frame),
    t_ns: num(value.t_ns),
    state: value.state as MapTrackingState,
    reason: typeof value.reason === "string" ? value.reason : "",
    valid,
    estimate,
    calibration: { estimated: calibration.estimated === true, rms_px: num(calibration.rms_px), views: num(calibration.views), frames: num(calibration.frames) },
    alignment: { aligned: alignment.aligned === true, matched_tags: num(alignment.matched_tags), residual_m: num(alignment.residual_m) },
  };
}

/** The points of a snapshot, decoded. The counts come from `count`; short buffers are cut to whole rows. */
export function parsePoints(value: unknown): MapPoints {
  const p = isObject(value) ? value : {};
  const ids = bytesOf(p.ids);
  const xyz = bytesOf(p.xyz);
  const observations = bytesOf(p.observations);
  const grey = bytesOf(p.grey);
  const count = Math.min(Math.floor(ids.length / 4), Math.floor(xyz.length / 12), Math.floor(observations.length / 2), grey.length);
  const view = (bytes: Uint8Array) => new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const idView = view(ids);
  const xyzView = view(xyz);
  const obsView = view(observations);
  const idArray = new Uint32Array(count);
  const xyzArray = new Float32Array(count * 3);
  const obsArray = new Uint16Array(count);
  for (let i = 0; i < count; i++) {
    idArray[i] = idView.getUint32(i * 4, true);
    obsArray[i] = obsView.getUint16(i * 2, true);
    for (let axis = 0; axis < 3; axis++) xyzArray[i * 3 + axis] = xyzView.getFloat32((i * 3 + axis) * 4, true);
  }
  return {
    count,
    min_xyz: numbers(p.min_xyz),
    max_xyz: numbers(p.max_xyz),
    ids: idArray,
    xyz: xyzArray,
    observations: obsArray,
    grey: grey.slice(0, count),
  };
}

/** The snapshot output (`eidos:map_snapshot`), or null when the value is not one. */
export function parseMapSnapshot(value: unknown): MapSnapshot | null {
  if (!isObject(value) || !("trajectory" in value)) return null;
  return {
    seq: num(value.seq),
    run_id: num(value.run_id),
    tag_size_m: num(value.tag_size_m, 0.1651),
    aligned: value.aligned === true,
    estimated: value.estimated === true,
    trajectory: list(value.trajectory).map((f) => {
      const frame = isObject(f) ? f : {};
      return { frame: num(frame.frame), pose: transformOf(frame.pose), sigma: sigmaOf(frame.sigma), keyframe: frame.keyframe === true };
    }),
    keyframes: numbers(value.keyframes),
    points: parsePoints(value.points),
    tags: list(value.tags).map((t) => {
      const tag = isObject(t) ? t : {};
      return { id: num(tag.id), pose: transformOf(tag.pose), placed: num(tag.placed) };
    }),
  };
}

/** The timing output (`eidos:map_timing`), or null when the value is not one. */
export function parseMapTiming(value: unknown): MapTiming | null {
  if (!isObject(value) || !("frame_ms" in value)) return null;
  return {
    frame: num(value.frame),
    t_ns: num(value.t_ns),
    frame_ms: num(value.frame_ms),
    map_ms: num(value.map_ms),
    stages: list(value.stages).map((s) => {
      const stage = isObject(s) ? s : {};
      return { name: typeof stage.name === "string" ? stage.name : "", ms: num(stage.ms) };
    }),
  };
}

/** The median of the timings seen in the last `windowMs` (by the time they were pushed). */
export class TimingWindow {
  private samples: { at: number; frameMs: number; mapMs: number }[] = [];

  constructor(private windowMs = 1000) {}

  push(at: number, frameMs: number, mapMs: number) {
    this.samples.push({ at, frameMs, mapMs });
    this.trim(at);
  }

  /** The p50 frame and map milliseconds of the window ending at `now`, or null without samples. */
  p50(now: number): { frameMs: number; mapMs: number; count: number } | null {
    this.trim(now);
    if (!this.samples.length) return null;
    const median = (values: number[]) => {
      const sorted = [...values].sort((a, b) => a - b);
      return sorted[Math.floor(sorted.length / 2)];
    };
    return {
      frameMs: median(this.samples.map((s) => s.frameMs)),
      mapMs: median(this.samples.map((s) => s.mapMs)),
      count: this.samples.length,
    };
  }

  private trim(now: number) {
    const cutoff = now - this.windowMs;
    let drop = 0;
    while (drop < this.samples.length && this.samples[drop].at < cutoff) drop++;
    if (drop) this.samples.splice(0, drop);
  }
}

/** The view a Map pane draws: the latest of each output of one pipeline. */
export interface MapView {
  pose: MapPose | null;
  snapshot: MapSnapshot | null;
  timing: MapTiming | null;
}
