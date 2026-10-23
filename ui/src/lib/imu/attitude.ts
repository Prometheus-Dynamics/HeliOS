// The IMU's attitude: a Mahony filter over the accelerometer and gyro (the gyro integrated, pulled
// toward the accelerometer's gravity so tilt doesn't drift), and the board's own fused quaternion
// (lemnosd's `orientation` device) when it streams. Heading (yaw) has nothing to pull it back
// in the filter, so it drifts slowly; `zero()` makes the current heading straight ahead.

import type { LiveDevice } from "./feed.ts";

/** A unit quaternion, w first: the board's frame to the world's (z up). */
export type Quat = [number, number, number, number];

/** How hard the accelerometer pulls (1/s): higher follows it faster but shakes more. */
const KP = 2;
/** A gap longer than this (s) between samples isn't integrated across. */
const MAX_DT = 0.1;
/** Accelerometer magnitudes outside this band (m/s²) are a shake, not gravity. */
const G_BAND: [number, number] = [6, 13.6];

export function normalize(q: Quat): Quat {
  const n = Math.hypot(...q) || 1;
  return [q[0] / n, q[1] / n, q[2] / n, q[3] / n];
}

/** The rotation that takes the board's `up` (measured gravity) to world z: tilt only. */
export function fromGravity(ax: number, ay: number, az: number): Quat {
  const n = Math.hypot(ax, ay, az) || 1;
  const [x, y, z] = [ax / n, ay / n, az / n];
  // Rotate (x, y, z) onto (0, 0, 1): axis (y, -x, 0), angle acos(z).
  const half = Math.acos(Math.max(-1, Math.min(1, z))) / 2;
  const s = Math.hypot(x, y);
  if (s < 1e-9) return z >= 0 ? [1, 0, 0, 0] : [0, 1, 0, 0];
  const k = Math.sin(half) / s;
  return normalize([Math.cos(half), y * k, -x * k, 0]);
}

/** One Mahony step: `gyro` rad/s, `accel` m/s² (the support force, up), over `dt` s. */
export function step(q: Quat, gyro: [number, number, number], accel: [number, number, number], dt: number): Quat {
  let [gx, gy, gz] = gyro;
  const [w, x, y, z] = q;
  const an = Math.hypot(...accel);
  if (an > G_BAND[0] && an < G_BAND[1]) {
    const [ax, ay, az] = [accel[0] / an, accel[1] / an, accel[2] / an];
    // World up in the board's frame, from q.
    const vx = 2 * (x * z - w * y);
    const vy = 2 * (w * x + y * z);
    const vz = w * w - x * x - y * y + z * z;
    // Turn the estimate toward the measurement: a × v.
    gx += KP * (ay * vz - az * vy);
    gy += KP * (az * vx - ax * vz);
    gz += KP * (ax * vy - ay * vx);
  }
  const h = 0.5 * dt;
  return normalize([
    w + h * (-x * gx - y * gy - z * gz),
    x + h * (w * gx + y * gz - z * gy),
    y + h * (w * gy - x * gz + z * gx),
    z + h * (w * gz + x * gy - y * gx),
  ]);
}

/** Roll, pitch, yaw in degrees (ZYX), for the readout. */
export function euler(q: Quat): { roll: number; pitch: number; yaw: number } {
  const [w, x, y, z] = q;
  const deg = 180 / Math.PI;
  return {
    roll: Math.atan2(2 * (w * x + y * z), 1 - 2 * (x * x + y * y)) * deg,
    pitch: Math.asin(Math.max(-1, Math.min(1, 2 * (w * y - z * x)))) * deg,
    yaw: Math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z)) * deg,
  };
}

/** a ⊗ b (Hamilton, scalar first). */
export function multiply(a: Quat, b: Quat): Quat {
  return [
    a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
    a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
    a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
    a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
  ];
}

/** The rotation about world z that makes `q`'s heading zero. */
function headingOffset(q: Quat): Quat {
  const half = (-euler(q).yaw * Math.PI) / 360;
  return [Math.cos(half), 0, 0, Math.sin(half)];
}

/**
 * The case's frame from the IMU's, as the Raze Gen 1 mounts it. The case (the 3D model) has z out
 * of the lens and the power terminal on its -y edge; measured on a board: lens up reads accel
 * z = -g, power terminal up reads x = +g. So the IMU's -z is the lens, its +x the terminal, its
 * +y the case's -x: a half turn about the case's (1, -1, 0). The quaternion takes case vectors to
 * IMU vectors.
 */
export const CASE_IN_IMU: Quat = [0, Math.SQRT1_2, -Math.SQRT1_2, 0];

/** The case's orientation (case frame to world) from the IMU's. */
export function caseOrientation(q: Quat): Quat {
  return multiply(q, CASE_IN_IMU);
}

/** Feeds an IMU device's new rows to the filter. `channels`: the indices of ax, ay, az, gx, gy, gz. */
export class Orientation {
  q: Quat = [1, 0, 0, 0];
  private lastUs = -Infinity;
  private started = false;

  constructor(private readonly channels: number[]) {}

  /** Folds in every row newer than the last one seen. */
  update(device: LiveDevice) {
    const [ax, ay, az, gx, gy, gz] = this.channels;
    for (let i = device.firstFrom(this.lastUs + 1); i < device.count; i++) {
      const at = device.index(i);
      const t = device.t[at];
      const a: [number, number, number] = [device.v[ax][at], device.v[ay][at], device.v[az][at]];
      const g: [number, number, number] = [device.v[gx][at], device.v[gy][at], device.v[gz][at]];
      if (a.some(Number.isNaN)) continue;
      if (!this.started) {
        this.q = fromGravity(...a);
        this.started = true;
      } else {
        const dt = (t - this.lastUs) / 1_000_000;
        if (dt > 0 && dt <= MAX_DT) this.q = step(this.q, g.some(Number.isNaN) ? [0, 0, 0] : g, a, dt);
      }
      this.lastUs = t;
    }
  }

  zero() {
    this.q = multiply(headingOffset(this.q), this.q);
  }
}

/** The board's fused orientation (lemnosd's `orientation` device): its quaternion as it streams, with
 * "Zero heading" as a turn about world z kept here. */
export class FusedOrientation {
  q: Quat = [1, 0, 0, 0];
  /** The board's own quaternion, before the heading offset. */
  private raw: Quat = [1, 0, 0, 0];
  /** Applied as offset ⊗ raw. */
  private offset: Quat = [1, 0, 0, 0];
  /** True while the board ignores the magnetometer (heading from the gyro only). */
  disturbed = false;
  confidence: number | null = null;

  constructor(private readonly channels: { q: number[]; disturbed: number; confidence: number }) {}

  update(device: LiveDevice) {
    if (!device.count) return;
    const q = this.channels.q.map((c) => device.latest(c)) as Quat;
    if (q.some((v) => !Number.isFinite(v))) return;
    this.raw = normalize(q);
    this.q = multiply(this.offset, this.raw);
    const d = device.latest(this.channels.disturbed);
    this.disturbed = d >= 0.5;
    const c = device.latest(this.channels.confidence);
    this.confidence = Number.isFinite(c) ? c : null;
  }

  zero() {
    this.offset = headingOffset(this.raw);
    this.q = multiply(this.offset, this.raw);
  }

  /** The channel indices in a fusion device, or null when it isn't one. */
  static channelsOf(names: string[]): { q: number[]; disturbed: number; confidence: number } | null {
    const q = ["quaternion.w", "quaternion.x", "quaternion.y", "quaternion.z"].map((n) => names.indexOf(n));
    if (q.some((i) => i < 0)) return null;
    return { q, disturbed: names.indexOf("magnetic_disturbance"), confidence: names.indexOf("confidence.imu") };
  }
}
