// Camera calibration: boards, captured views, and solved intrinsics per
// camera and resolution. On a robot the device captures and solves (Eidos
// ChArUco detection and calibration, `/v1/cameras/{id}/calibration/*`) and
// stores the result; in mock mode capture and solve are simulated.

import { api, errorText } from "#lib/api/client.js";
import { loadRaw, save } from "#lib/core/persist.js";
import type { Camera } from "#lib/api/model.js";
import type * as W from "#lib/api/types.js";
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
  /** The device's index of this view (live), for leaving it out of a solve. */
  index?: number;
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
  /** What the device solved, to store as is (live). */
  device?: W.CameraCalibration;
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
  /** Board corners the device sees now (live, normalised), while capturing. */
  seen = $state<[number, number][] | null>(null);
  /** Why the last live capture kept nothing. */
  note = $state<string | null>(null);
  #last = 0;
  #looking = false;
  #lastLook = 0;
  #lastKept: [number, number] | null = null;
  #dropped = new Set<number>();

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
    if (cluster.live) return this.seen;
    const marks = cluster.detectionsFor(camera);
    if (!marks.length || !cluster.feed) return null;
    const w = cluster.feed.width;
    const h = cluster.feed.height;
    return marks.flatMap((m) => m.corners.map(([x, y]) => [x / w, y / h] as [number, number]));
  }

  get #liveBoard(): W.CalibrationBoard {
    return { squares_x: this.board.cols, squares_y: this.board.rows, square_mm: this.board.square, marker_mm: this.board.marker, dictionary: this.board.dictionary };
  }

  /** Ask the device for a frame and the board in it; `keep` adds it as a view. */
  async #look(camera: Camera, keep: boolean, quiet: boolean): Promise<boolean> {
    if (this.board.kind !== "charuco") {
      if (!quiet) toasts.warning("The device calibrates with ChArUco boards");
      return false;
    }
    if (this.#looking) return false;
    this.#looking = true;
    try {
      const r = await api.captureCalibration(camera.resourceId, { board: this.#liveBoard, width: camera.settings.width, height: camera.settings.height, keep });
      const pts = r.corners.map(([x, y]) => [x / r.width, y / r.height] as [number, number]);
      this.seen = pts.length ? pts : null;
      this.note = r.message ?? null;
      if (r.kept) {
        this.captures.push({ points: pts, tilt: tiltOf(pts), index: r.views - 1 });
        this.#lastKept = centroid(pts);
      } else if (keep && !quiet) toasts.warning(r.message ?? "No board in view");
      return r.kept;
    } catch (error) {
      this.note = errorText(error);
      if (!quiet) toasts.error(`Capture: ${errorText(error)}`);
      return false;
    } finally {
      this.#looking = false;
    }
  }

  capture(camera: Camera, quiet = false): boolean {
    if (cluster.live) {
      void this.#look(camera, true, quiet);
      return true;
    }
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
    if (cluster.live) {
      // The device looks for the board about twice a second while capturing; auto-capture
      // keeps a view once the board has moved on from the last one kept.
      if (this.step !== "capture" || performance.now() - this.#lastLook < 500) return;
      this.#lastLook = performance.now();
      const moved = !this.#lastKept || !this.seen || dist(centroid(this.seen), this.#lastKept) > 0.08;
      void this.#look(camera, this.auto && moved && !!this.seen, true);
      return;
    }
    if (!this.auto || this.step !== "capture") return;
    if (cluster.frame - this.#last < 10) return;
    this.capture(camera, true);
  }

  async solve(camera: Camera) {
    if (cluster.live) {
      this.step = "solve";
      try {
        // Both lens models are fitted and the better fit kept: the lens decides, not a setting.
        const drop = [...this.#dropped];
        const fits = await Promise.allSettled((["pinhole", "fisheye"] as const).map((model) => api.solveCalibration(camera.resourceId, { model, drop })));
        const solved = fits.flatMap((f) => (f.status === "fulfilled" ? [f.value] : []));
        if (!solved.length) throw (fits[0] as PromiseRejectedResult).reason;
        const r = solved.reduce((best, f) => ((f.calibration.rms_px ?? Infinity) < (best.calibration.rms_px ?? Infinity) ? f : best));
        const used = this.captures.filter((c) => c.index === undefined || !this.#dropped.has(c.index));
        used.forEach((c, i) => (c.error = +(r.view_rms[i] ?? 0).toFixed(3)));
        this.captures = used;
        const k = r.calibration;
        const d = k.distortion ?? {};
        this.result = {
          camera: camera.resourceId,
          width: k.width,
          height: k.height,
          rms: k.rms_px ?? 0,
          fx: +k.fx.toFixed(2),
          fy: +k.fy.toFixed(2),
          cx: +k.cx.toFixed(2),
          cy: +k.cy.toFixed(2),
          dist: (k.model === "fisheye" ? [d.k1, d.k2, d.k3, d.k4] : [d.k1, d.k2, d.p1, d.p2, d.k3]).map((v) => +(v ?? 0).toFixed(5)),
          views: r.views,
          at: Date.now(),
          device: k,
        };
        this.step = "done";
      } catch (error) {
        toasts.error(`Solve: ${errorText(error)}`);
        this.step = "capture";
      }
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
    for (const c of this.captures) if ((c.error ?? 0) > cut && c.index !== undefined) this.#dropped.add(c.index);
    this.captures = this.captures.filter((c) => (c.error ?? 0) <= cut);
    toasts.info(`Dropped ${before - this.captures.length} worst views`);
    await this.solve(camera);
  }

  apply(camera: Camera) {
    if (!this.result) return;
    if (cluster.live && this.result.device) {
      const r = this.result;
      api
        .putCalibration(camera.resourceId, r.device!)
        .then(() => {
          this.saved[this.key(camera)] = r;
          save("calibrations", this.saved);
          toasts.success(`${camera.name} calibrated at ${r.width}×${r.height}: ${r.rms.toFixed(2)} px RMS, stored on the device`);
        })
        .catch((error) => toasts.error(`Save: ${errorText(error)}`));
      return;
    }
    this.saved[this.key(camera)] = this.result;
    save("calibrations", this.saved);
    toasts.success(`${camera.name} calibrated at ${camera.settings.width}×${camera.settings.height}: ${this.result.rms.toFixed(2)} px RMS`);
  }

  restart(camera?: Camera) {
    if (cluster.live && camera) void api.resetCalibration(camera.resourceId).catch(() => {});
    this.#dropped.clear();
    this.#lastKept = null;
    this.seen = null;
    this.note = null;
    this.captures = [];
    this.result = null;
    this.auto = false;
    this.step = "capture";
  }
}

function centroid(pts: [number, number][]): [number, number] {
  const n = pts.length || 1;
  return [pts.reduce((a, p) => a + p[0], 0) / n, pts.reduce((a, p) => a + p[1], 0) / n];
}

function dist(a: [number, number], b: [number, number]): number {
  return Math.hypot(a[0] - b[0], a[1] - b[1]);
}

/** A rough tilt bucket (0, 15, 30, 45 degrees) from how foreshortened the corners' spread is. */
function tiltOf(pts: [number, number][]): number {
  const [mx, my] = centroid(pts);
  let sxx = 0;
  let syy = 0;
  for (const [x, y] of pts) {
    sxx += (x - mx) ** 2;
    syy += (y - my) ** 2;
  }
  const ratio = Math.sqrt(Math.min(sxx, syy) / Math.max(sxx, syy, 1e-9));
  return Math.min(45, Math.round((Math.acos(Math.min(1, ratio / 0.75)) * 180) / Math.PI / 15) * 15);
}

export const calibration = new CalibrationStore();
