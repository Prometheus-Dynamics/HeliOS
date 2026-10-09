// Camera calibration: boards, captured views, and solved intrinsics per
// camera and resolution. Capture and solve are simulated here; on a robot the
// device runs them (Eidos) and the UI shows the same steps.

import { loadRaw, save } from "#lib/core/persist.js";
import type { Camera } from "#lib/api/model.js";
import { cluster } from "./cluster.svelte";
import { toasts } from "./toasts.svelte";

export type BoardKind = "charuco" | "chessboard" | "aprilgrid";

export interface Board {
  kind: BoardKind;
  cols: number;
  rows: number;
  /** Square edge, millimetres. */
  square: number;
  /** Marker edge (ChArUco, AprilGrid), millimetres. */
  marker: number;
  dictionary: string;
  paper: "letter" | "a4" | "a3" | "tabloid";
}

export interface Capture {
  /** Board corners seen, normalised 0..1 image coordinates. */
  points: [number, number][];
  /** Board tilt bucket, degrees, for pose variety. */
  tilt: number;
  error?: number;
}

export interface Calibration {
  camera: string;
  width: number;
  height: number;
  rms: number;
  fx: number;
  fy: number;
  cx: number;
  cy: number;
  dist: number[];
  views: number;
  at: number;
}

export const COVER_COLS = 8;
export const COVER_ROWS = 5;

class CalibrationStore {
  board = $state<Board>(loadRaw("cal-board", { kind: "charuco", cols: 8, rows: 6, square: 30, marker: 22, dictionary: "aruco_4x4_50", paper: "letter" }));
  captures = $state<Capture[]>([]);
  step = $state<"setup" | "capture" | "solve" | "done">("setup");
  auto = $state(false);
  result = $state<Calibration | null>(null);
  saved = $state<Record<string, Calibration>>(loadRaw("calibrations", {}));
  #last = 0;

  saveBoard() {
    save("cal-board", this.board);
  }

  key(c: Camera) {
    return `${c.resourceId}@${c.settings.width}x${c.settings.height}`;
  }

  coverage(): number[] {
    const cells = new Array(COVER_COLS * COVER_ROWS).fill(0);
    for (const cap of this.captures)
      for (const [x, y] of cap.points) {
        const i = Math.min(COVER_ROWS - 1, Math.floor(y * COVER_ROWS)) * COVER_COLS + Math.min(COVER_COLS - 1, Math.floor(x * COVER_COLS));
        cells[i]++;
      }
    return cells;
  }

  get coveredFraction(): number {
    return this.coverage().filter((n) => n > 0).length / (COVER_COLS * COVER_ROWS);
  }

  get tiltVariety(): number {
    return new Set(this.captures.map((c) => c.tilt)).size;
  }

  /** Board visible in this frame? Uses the frame's tag detections as the board's footprint. */
  boardIn(camera: Camera): [number, number][] | null {
    const marks = cluster.detectionsFor(camera);
    if (!marks.length || !cluster.feed) return null;
    const w = cluster.feed.width;
    const h = cluster.feed.height;
    return marks.flatMap((m) => m.corners.map(([x, y]) => [x / w, y / h] as [number, number]));
  }

  capture(camera: Camera, quiet = false): boolean {
    const pts = this.boardIn(camera);
    if (!pts) {
      if (!quiet) toasts.warning("No board in view");
      return false;
    }
    this.captures.push({ points: pts, tilt: Math.round(((cluster.frame * 7) % 60) / 15) * 15 });
    this.#last = cluster.frame;
    return true;
  }

  /** Called every frame while auto-capture is on: takes a view when the board has moved on. */
  autoTick(camera: Camera) {
    if (!this.auto || this.step !== "capture") return;
    if (cluster.frame - this.#last < 10) return;
    this.capture(camera, true);
  }

  async solve(camera: Camera) {
    if (cluster.live) {
      // The device has no calibration service yet; the API says so (501).
      await cluster.live.calibrate(camera.resourceId);
      return;
    }
    this.step = "solve";
    await new Promise((r) => setTimeout(r, 900));
    const cover = this.coveredFraction;
    const n = this.captures.length;
    const base = 0.16 + (1 - cover) * 0.5 + Math.max(0, 15 - n) * 0.02;
    for (const c of this.captures) c.error = +(base * (0.6 + Math.random() * 0.9)).toFixed(3);
    const w = camera.settings.width;
    const h = camera.settings.height;
    const f = (w / 2) / Math.tan((80 / 2) * (Math.PI / 180));
    this.result = {
      camera: camera.resourceId,
      width: w,
      height: h,
      rms: +base.toFixed(3),
      fx: +(f * (1 + (Math.random() - 0.5) * 0.01)).toFixed(2),
      fy: +(f * (1 + (Math.random() - 0.5) * 0.01)).toFixed(2),
      cx: +(w / 2 + (Math.random() - 0.5) * 12).toFixed(2),
      cy: +(h / 2 + (Math.random() - 0.5) * 10).toFixed(2),
      dist: [-0.312, 0.118, 0.0004, -0.0002, -0.021].map((d) => +(d * (1 + (Math.random() - 0.5) * 0.1)).toFixed(5)),
      views: n,
      at: Date.now(),
    };
    this.step = "done";
  }

  /** Drops the worst views and solves again. */
  async rejectOutliers(camera: Camera) {
    const errs = this.captures.map((c) => c.error ?? 0).sort((a, b) => a - b);
    const cut = errs[Math.floor(errs.length * 0.8)] ?? Infinity;
    const before = this.captures.length;
    this.captures = this.captures.filter((c) => (c.error ?? 0) <= cut);
    toasts.info(`Dropped ${before - this.captures.length} worst views`);
    await this.solve(camera);
  }

  apply(camera: Camera) {
    if (!this.result) return;
    this.saved[this.key(camera)] = this.result;
    save("calibrations", this.saved);
    toasts.success(`${camera.name} calibrated at ${camera.settings.width}×${camera.settings.height}: ${this.result.rms.toFixed(2)} px RMS`);
  }

  restart() {
    this.captures = [];
    this.result = null;
    this.auto = false;
    this.step = "capture";
  }
}

export const calibration = new CalibrationStore();
