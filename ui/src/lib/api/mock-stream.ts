// The simulated robot's live streams for `?mock=1`: the catalog and the samples helios-api would
// send, as the same frames (imu/frame.ts encodes them). A Raze at rest on a bench, turning slowly:
// the IMU reads gravity and the gyro with noise, the fused orientation follows the attitude, the
// magnetometer reads a field turned into the board's frame, and the status ring has a comet going
// round. The first frame is the backfill of the last few seconds, as a subscription gets.

import { encodeSamples, type Catalog, type CatalogSeries } from "../imu/frame";
import type { StreamHandlers } from "./streams";

/** The sample grid: every device is sampled on it (10 ms). */
const GRID_US = 10_000;
/** The history a new client starts with (the same as helios-api's default). */
const BACKFILL_US = 5_000_000;
/** How often the batches go out. */
const TICK_MS = 20;

type Quat = [number, number, number, number];
type Vec = [number, number, number];

const LEDS = 16;
const G = 9.80665;
const EARTH_FIELD_UT: Vec = [20, 0, -40];

const SERIES: CatalogSeries[] = [
  { name: "imu.acceleration.x", unit: "m/s²", kind: "f32", period_us: GRID_US },
  { name: "imu.acceleration.y", unit: "m/s²", kind: "f32", period_us: GRID_US },
  { name: "imu.acceleration.z", unit: "m/s²", kind: "f32", period_us: GRID_US },
  { name: "imu.angular_rate.x", unit: "rad/s", kind: "f32", period_us: GRID_US },
  { name: "imu.angular_rate.y", unit: "rad/s", kind: "f32", period_us: GRID_US },
  { name: "imu.angular_rate.z", unit: "rad/s", kind: "f32", period_us: GRID_US },
  { name: "orientation.quaternion.w", unit: "", kind: "f32", period_us: GRID_US },
  { name: "orientation.quaternion.x", unit: "", kind: "f32", period_us: GRID_US },
  { name: "orientation.quaternion.y", unit: "", kind: "f32", period_us: GRID_US },
  { name: "orientation.quaternion.z", unit: "", kind: "f32", period_us: GRID_US },
  { name: "orientation.magnetic_disturbance", unit: "", kind: "f32", period_us: GRID_US },
  { name: "orientation.confidence.imu", unit: "", kind: "f32", period_us: GRID_US },
  { name: "mag.magnetic_field.x", unit: "µT", kind: "f32", period_us: 100_000 },
  { name: "mag.magnetic_field.y", unit: "µT", kind: "f32", period_us: 100_000 },
  { name: "mag.magnetic_field.z", unit: "µT", kind: "f32", period_us: 100_000 },
  { name: "light.offset", unit: "", kind: "f32", period_us: 50_000 },
  { name: "light.clockwise", unit: "", kind: "f32", period_us: 50_000 },
  { name: "light.brightness", unit: "", kind: "f32", period_us: 50_000 },
  ...Array.from({ length: LEDS }, (_, i) => ({ name: `light.led.${i}`, unit: null, kind: "u32" as const, period_us: 50_000 })),
];

const CATALOG: Catalog = { stream: "board.hardware", owner: "node/raze-sim", epoch: 1, live: true, series: SERIES };

/** The attitude at `t` seconds: a slow tilt and a steady turn about the board's z. */
function attitude(t: number): Quat {
  return fromEuler(0.35 * Math.sin(0.4 * t), 0.25 * Math.sin(0.27 * t + 1), 0.2 * t);
}

function fromEuler(roll: number, pitch: number, yaw: number): Quat {
  const [cr, sr] = [Math.cos(roll / 2), Math.sin(roll / 2)];
  const [cp, sp] = [Math.cos(pitch / 2), Math.sin(pitch / 2)];
  const [cy, sy] = [Math.cos(yaw / 2), Math.sin(yaw / 2)];
  return normalize([
    cr * cp * cy + sr * sp * sy,
    sr * cp * cy - cr * sp * sy,
    cr * sp * cy + sr * cp * sy,
    cr * cp * sy - sr * sp * cy,
  ]);
}

function normalize(q: Quat): Quat {
  const n = Math.hypot(...q) || 1;
  return [q[0] / n, q[1] / n, q[2] / n, q[3] / n];
}

function multiply(a: Quat, b: Quat): Quat {
  return [
    a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
    a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
    a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
    a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
  ];
}

const conjugate = (q: Quat): Quat => [q[0], -q[1], -q[2], -q[3]];

/** The vector `v` taken from world to the board's frame (q is board to world): q* v q. */
function toBoard(q: Quat, v: Vec): Vec {
  const r = multiply(multiply(conjugate(q), [0, v[0], v[1], v[2]]), q);
  return [r[1], r[2], r[3]];
}

/** Random-looking but repeatable noise in [-1, 1) for sample `k` of channel `c`. */
function noise(k: number, c: number): number {
  const x = Math.sin(k * 12.9898 + c * 78.233) * 43758.5453;
  return (x - Math.floor(x)) * 2 - 1;
}

function hsv(hue: number): number {
  const h = ((hue % 1) + 1) % 1;
  const i = Math.floor(h * 6);
  const f = h * 6 - i;
  const q = 1 - f;
  const [r, g, b] = [[1, f, 0], [q, 1, 0], [0, 1, f], [0, q, 1], [f, 0, 1], [1, 0, q]][i % 6];
  const c = (x: number) => Math.round(x * 255);
  return (c(r) << 16) | (c(g) << 8) | c(b);
}

/** One sample of every series at grid point `k`, keyed by series index (absent where not sampled). */
function sampleAt(k: number): Map<number, number> {
  const t = k * GRID_US / 1e6;
  const q = attitude(t);
  const q1 = attitude(t + 1e-3);
  // Body rate: the rotation from q to q1 over the step, as an axis times angle.
  const d = multiply(conjugate(q), q1);
  const w: Vec = [(2 * d[1]) / 1e-3, (2 * d[2]) / 1e-3, (2 * d[3]) / 1e-3];
  const up = toBoard(q, [0, 0, G]);
  const field = toBoard(q, EARTH_FIELD_UT);
  const out = new Map<number, number>();
  // Series 0-2 accel, 3-5 gyro, 6-9 quaternion (w, x, y, z), 10 disturbance, 11 IMU confidence.
  for (let c = 0; c < 3; c++) {
    out.set(c, up[c] + 0.02 * noise(k, c));
    out.set(3 + c, w[c] + 0.004 * noise(k, 3 + c));
  }
  for (let c = 0; c < 4; c++) out.set(6 + c, q[c]);
  out.set(10, 0);
  out.set(11, 0.9);
  // Magnetometer, every 100 ms.
  if (k % 10 === 0) for (let c = 0; c < 3; c++) out.set(12 + c, field[c] + 0.3 * noise(k, 12 + c));
  // The ring: offset, clockwise, brightness, then its LEDs, every 50 ms.
  if (k % 5 === 0) {
    out.set(15, 0);
    out.set(16, 1);
    out.set(17, 0.4);
    for (let led = 0; led < LEDS; led++) {
      const angle = (((led / LEDS - t * 0.15) % 1) + 1) % 1;
      const lit = Math.max(0, Math.cos((angle + 1) * Math.PI * 2 * 2) * 0.5 + 0.5) ** 6;
      out.set(18 + led, ledColor(angle, lit));
    }
  }
  return out;
}

/** A ring LED's colour (0xWWRRGGBB, white 0): a hue along the ring, dimmed by `lit`. */
function ledColor(angle: number, lit: number): number {
  const rgb = hsv(0.55 + angle * 0.25);
  const scale = (channel: number) => Math.round(((rgb >> channel) & 0xff) * lit);
  return (scale(16) << 16) | (scale(8) << 8) | scale(0);
}

/** The samples of grid points `from` to `to` (both included), as the frame's blocks. */
function blocksFor(from: number, to: number): { series: number; samples: [number, number][] }[] {
  const per = new Map<number, [number, number][]>();
  for (let k = from; k <= to; k++) {
    const t = k * GRID_US;
    for (const [series, value] of sampleAt(k)) {
      let list = per.get(series);
      if (!list) per.set(series, (list = []));
      list.push([t, value]);
    }
  }
  return [...per.entries()].sort(([a], [b]) => a - b).map(([series, samples]) => ({ series, samples }));
}

/** The simulated stream, to the same handlers as the device's. Returns the stop. */
export function openSimulatedStream(handlers: StreamHandlers): () => void {
  handlers.text(JSON.stringify({ type: "catalog", ...CATALOG }));
  handlers.text(JSON.stringify({ type: "status", state: "live", reason: "" }));
  let last = Math.floor(Date.now() * 1000 / GRID_US);
  const backfillFrom = last - BACKFILL_US / GRID_US;
  handlers.binary(encodeSamples({ backfill: true, blocks: blocksFor(backfillFrom, last) }));
  const timer = window.setInterval(() => {
    const now = Math.floor(Date.now() * 1000 / GRID_US);
    if (now <= last) return;
    handlers.binary(encodeSamples({ backfill: false, blocks: blocksFor(last + 1, now) }));
    last = now;
  }, TICK_MS);
  return () => window.clearInterval(timer);
}
