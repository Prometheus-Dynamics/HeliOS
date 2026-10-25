// The tag and field poses of a camera's pipeline (`pose_solutions`, `multi_tag_pose`) and the
// camera's intrinsics, for drawing a tag's axes on the camera view and the camera's view of the
// scene. Poses are rigid transforms as Eidos writes them (`{translation: {x,y,z}, rotation:
// {w,x,y,z}}`): a tag's pose is camera-from-tag, the field pose is field-from-camera (WPILib).

import type { MapTransform, Vec3 } from "./map";

export interface TagPose {
  id: number;
  /** The tag in the camera's optical frame (x right, y down, z forward). */
  pose: MapTransform;
  errorPx: number | null;
  ambiguity: number | null;
}

export interface TagPoses {
  status: string;
  tags: TagPose[];
}

export interface FieldPose {
  valid: boolean;
  status: string;
  /** The camera in the field (WPILib blue-origin frame); null without a valid pose. */
  cameraInField: MapTransform | null;
  /** The robot in the field, through the camera's mount; null without a mount or a valid pose. */
  robotInField: MapTransform | null;
  rmsPx: number | null;
  inliers: number;
  ambiguity: number | null;
}

/** A camera's intrinsics at a frame of `width` x `height` pixels, with its lens model and distortion. */
export interface Intrinsics {
  fx: number;
  fy: number;
  cx: number;
  cy: number;
  width: number;
  height: number;
  /** `pinhole` (Brown-Conrady distortion) or `fisheye` (Kannala-Brandt); pinhole without distortion when absent. */
  model?: "pinhole" | "fisheye";
  distortion?: Partial<Record<"k1" | "k2" | "k3" | "k4" | "p1" | "p2", number>>;
}

type Json = Record<string, unknown>;
const isObject = (value: unknown): value is Json => typeof value === "object" && value !== null;
const num = (value: unknown, fallback = 0): number => (typeof value === "number" && Number.isFinite(value) ? value : fallback);
const orNull = (value: unknown): number | null => (typeof value === "number" && Number.isFinite(value) ? value : null);

/** A pose as Eidos writes it, or null when it is not one (a missing or empty pose). */
export function parsePose(value: unknown): MapTransform | null {
  if (!isObject(value) || !isObject(value.translation) || !isObject(value.rotation)) return null;
  const t = value.translation;
  const q = value.rotation;
  const parts = [t.x, t.y, t.z, q.w, q.x, q.y, q.z];
  if (!parts.every((v) => typeof v === "number" && Number.isFinite(v))) return null;
  return {
    translation: { x: num(t.x), y: num(t.y), z: num(t.z) },
    rotation: { w: num(q.w, 1), x: num(q.x), y: num(q.y), z: num(q.z) },
  };
}

/** The `pose_solutions` value: each tag's best pose, in the camera's frame. Null when it is not one. */
export function parseTagPoses(value: unknown): TagPoses | null {
  if (!isObject(value) || !Array.isArray(value.tags)) return null;
  const tags: TagPose[] = [];
  for (const entry of value.tags) {
    if (!isObject(entry)) continue;
    const pose = parsePose(entry.best);
    if (!pose) continue;
    tags.push({ id: num(entry.id), pose, errorPx: orNull(entry.best_error_px), ambiguity: orNull(entry.ambiguity) });
  }
  return { status: typeof value.status === "string" ? value.status : "", tags };
}

/** The `multi_tag_pose` value: the camera (and robot) in the field. Null when it is not one. */
export function parseFieldPose(value: unknown): FieldPose | null {
  if (!isObject(value) || typeof value.valid !== "boolean") return null;
  const valid = value.valid;
  const hasRig = value.has_rig === true;
  return {
    valid,
    status: typeof value.status === "string" ? value.status : "",
    cameraInField: valid ? parsePose(value.reference_from_camera) : null,
    robotInField: valid && hasRig ? parsePose(value.reference_from_rig) : null,
    rmsPx: orNull(value.rms_px),
    inliers: num(value.inlier_tags),
    ambiguity: orNull(value.ambiguity),
  };
}

/** Rotates `v` by the unit quaternion `q` (w, x, y, z). */
export function rotate(q: MapTransform["rotation"], v: Vec3): Vec3 {
  // v' = v + 2w(u x v) + 2 u x (u x v), with u the quaternion's vector part.
  const { w, x: ux, y: uy, z: uz } = q;
  const tx = 2 * (uy * v.z - uz * v.y);
  const ty = 2 * (uz * v.x - ux * v.z);
  const tz = 2 * (ux * v.y - uy * v.x);
  return {
    x: v.x + w * tx + (uy * tz - uz * ty),
    y: v.y + w * ty + (uz * tx - ux * tz),
    z: v.z + w * tz + (ux * ty - uy * tx),
  };
}

export interface AxisSegment {
  axis: "x" | "y" | "z";
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/**
 * A point in the camera's optical frame, as a pixel: the pinhole projection with the lens's
 * distortion (Kannala-Brandt for a fisheye, Brown-Conrady otherwise). Null when it is behind the camera.
 */
export function projectPoint(k: Intrinsics, x3: number, y3: number, z3: number): { u: number; v: number } | null {
  if (z3 < 0.02) return null;
  const x = x3 / z3;
  const y = y3 / z3;
  const d = k.distortion ?? {};
  let xd = x;
  let yd = y;
  if (k.model === "fisheye") {
    const r = Math.hypot(x, y);
    if (r > 1e-9) {
      const theta = Math.atan(r);
      const t2 = theta * theta;
      const td = theta * (1 + (d.k1 ?? 0) * t2 + (d.k2 ?? 0) * t2 * t2 + (d.k3 ?? 0) * t2 ** 3 + (d.k4 ?? 0) * t2 ** 4);
      xd = x * (td / r);
      yd = y * (td / r);
    }
  } else if (d.k1 || d.k2 || d.k3 || d.p1 || d.p2) {
    const r2 = x * x + y * y;
    const radial = 1 + (d.k1 ?? 0) * r2 + (d.k2 ?? 0) * r2 * r2 + (d.k3 ?? 0) * r2 ** 3;
    xd = x * radial + 2 * (d.p1 ?? 0) * x * y + (d.p2 ?? 0) * (r2 + 2 * x * x);
    yd = y * radial + (d.p1 ?? 0) * (r2 + 2 * y * y) + 2 * (d.p2 ?? 0) * x * y;
  }
  return { u: k.fx * xd + k.cx, v: k.fy * yd + k.cy };
}

/**
 * A tag's axes as pixel segments: from the tag's centre along its x, y and z, each one tag edge
 * long, projected with `k` (intrinsics in the pixels they are drawn in). Null when the tag is
 * behind the camera; an axis that crosses behind it is left out.
 */
export function tagAxes(pose: MapTransform, sizeM: number, k: Intrinsics): AxisSegment[] | null {
  const project = (p: Vec3): { u: number; v: number } | null => {
    const r = rotate(pose.rotation, p);
    return projectPoint(k, r.x + pose.translation.x, r.y + pose.translation.y, r.z + pose.translation.z);
  };
  const origin = project({ x: 0, y: 0, z: 0 });
  if (!origin) return null;
  const length = sizeM;
  const ends: [AxisSegment["axis"], Vec3][] = [
    ["x", { x: length, y: 0, z: 0 }],
    ["y", { x: 0, y: length, z: 0 }],
    ["z", { x: 0, y: 0, z: length }],
  ];
  const out: AxisSegment[] = [];
  for (const [axis, end] of ends) {
    const p = project(end);
    if (p) out.push({ axis, x0: origin.u, y0: origin.v, x1: p.u, y1: p.v });
  }
  return out;
}

/** Intrinsics scaled from their own frame to a frame of `width` x `height` (the same field of view). */
export function scaleIntrinsics(k: Intrinsics, width: number, height: number): Intrinsics {
  const sx = width / k.width;
  const sy = height / k.height;
  return { ...k, fx: k.fx * sx, fy: k.fy * sy, cx: k.cx * sx, cy: k.cy * sy, width, height };
}

/** The vertical field of view (radians) and the aspect ratio of a camera's intrinsics. */
export function fieldOfView(k: Pick<Intrinsics, "fy" | "width" | "height">): { fovY: number; aspect: number } {
  return { fovY: 2 * Math.atan(k.height / (2 * k.fy)), aspect: k.width / k.height };
}

/** A pipeline's latest tag poses (`pose_solutions`) and field pose (`multi_tag_pose`). */
export interface PoseView {
  tags: TagPoses | null;
  field: FieldPose | null;
}
