// A pose scenario against a running HeliOS sim (tools/sim/scenario.sh starts the sim for one).
//
// tools/sim/scenarios/<name>/ holds the scenario: `layout.json` (a WPILib field layout, the tags
// Styx renders and the pipeline knows) and `camera.json` (Styx's camera pose or trajectory). This
// script runs the live-map template with that layout on the simulated camera, records its `pose`
// output for SIM_SCENARIO_S seconds, and joins each pose with Styx's truth for the same frame
// (nearest timestamp within a quarter frame period). It writes `<out>/poses.jsonl` (the raw lines)
// and `<out>/report.csv` (one row per frame), and prints a summary.
//
// Per frame: the true and estimated camera position (layout frame), the error, the jump (the
// estimate's step minus the truth's step), the mapper's state and source, the visible tags' size
// in pixels (mean side of the black square) and the viewing angle (between the tag's normal and
// the ray to the camera).
//
//   SIM_URL (default http://127.0.0.1:5800), SIM_TRUTH (required), SIM_SCENARIO (required: name),
//   SIM_SCENARIO_S (default 64, scene seconds), SIM_SCENARIO_FPS (default 120), SIM_SCENARIO_OUT (default: the truth file's directory),
//   SIM_CALIBRATION (default tools/sim/ov9782-calibration.json),
//   SIM_SCENARIO_JUMP_CM (a jump above this counts, default 10).
//
// Calibration: scenario.json in the scenario directory, `{"calibration": "file"}` (the default: the
// lens of SIM_CALIBRATION is PUT to the camera) or `{"calibration": "none"}` (no calibration is given:
// any stored one is cleared, and the mapper estimates the camera from its opening frames). Styx renders
// with the true lens either way.
//
// Phases: a truth frame is "tags" while a tag corner is in the image before any frame without one, then
// "tag-free" while none is, then "tags again" for the tagged frames after that. Each phase's pose error
// is printed, and the report's `phase` column says which phase a frame is in.
//
// The calibration a pose reports (`calibration`: estimated, rms_px, views, frames) is in the report's
// cal_* columns. A pose that carries intrinsics would be compared with the true lens; the summary says
// when it carries none.

import { type TruthFrame, readTruth } from "./truth.ts";

const BASE = (process.env.SIM_URL ?? "http://127.0.0.1:5800").replace(/\/$/, "");
const WS_BASE = BASE.replace(/^http/, "ws");
const TRUTH = process.env.SIM_TRUTH ?? "";
const NAME = process.env.SIM_SCENARIO ?? "";
const SECONDS = Number(process.env.SIM_SCENARIO_S ?? 64);
/** The camera's frame rate (the OV9782's 120 fps mode, as on the robot); with Styx's lockstep
 * clock the frames are then exactly one 120 fps period apart in scene time. */
const FPS = Number(process.env.SIM_SCENARIO_FPS ?? 120);
const JUMP_CM = Number(process.env.SIM_SCENARIO_JUMP_CM ?? 10);
const CALIBRATION = process.env.SIM_CALIBRATION ?? new URL("./ov9782-calibration.json", import.meta.url).pathname;
if (!TRUTH || !NAME) {
  console.error("SIM_TRUTH and SIM_SCENARIO are required (tools/sim/scenario.sh sets them)");
  process.exit(2);
}
const DIR = new URL(`./scenarios/${NAME}/`, import.meta.url).pathname;
const OUT = process.env.SIM_SCENARIO_OUT ?? TRUTH.replace(/\/[^/]*$/, "");
const PIPELINE_ID = `sim-scenario-${NAME}`;
const LAYOUT_ID = `sim-${NAME}`;
type Phase = "tags" | "tag-free" | "tags again";

// The lens the scene was rendered with (the same file the sim's camera is built from).
const LENS = await Bun.file(CALIBRATION).json();
const SCENARIO_CONFIG_PATH = `${DIR}scenario.json`;
const scenarioConfig = (await Bun.file(SCENARIO_CONFIG_PATH).exists()) ? await Bun.file(SCENARIO_CONFIG_PATH).json() : {};
const CALIBRATION_MODE: string = process.env.SIM_SCENARIO_CALIBRATION ?? scenarioConfig.calibration ?? "file";
if (CALIBRATION_MODE !== "file" && CALIBRATION_MODE !== "none") {
  console.error(`${SCENARIO_CONFIG_PATH}: calibration must be "file" or "none", not ${JSON.stringify(CALIBRATION_MODE)}`);
  process.exit(2);
}
const CALIBRATED = CALIBRATION_MODE === "file";
// The live mapper's lens model for self-calibration (`lens_model`: auto, pinhole or fisheye), from
// SIM_SCENARIO_LENS_MODEL or scenario.json's "lens_model"; unset keeps the node's default (auto).
const LENS_MODEL: string | undefined = process.env.SIM_SCENARIO_LENS_MODEL ?? scenarioConfig.lens_model;
if (LENS_MODEL !== undefined && !["auto", "pinhole", "fisheye"].includes(LENS_MODEL)) {
  console.error(`lens_model must be auto, pinhole or fisheye, not ${JSON.stringify(LENS_MODEL)}`);
  process.exit(2);
}

type Vec3 = [number, number, number];

async function api(method: string, path: string, body?: unknown, contentType = "application/json"): Promise<any> {
  const init: RequestInit = { method, headers: body === undefined ? {} : { "content-type": contentType } };
  if (body !== undefined) init.body = typeof body === "string" ? body : JSON.stringify(body);
  const response = await fetch(BASE + path, init);
  const text = await response.text();
  if (!response.ok) throw new Error(`${method} ${path}: HTTP ${response.status} ${text}`);
  return text ? JSON.parse(text) : undefined;
}

async function poll<T>(read: () => Promise<T | undefined>, what: string, timeoutMs = 60_000): Promise<T> {
  const deadline = performance.now() + timeoutMs;
  for (;;) {
    const value = await read();
    if (value !== undefined) return value;
    if (performance.now() > deadline) throw new Error(`timed out waiting for ${what}`);
    await Bun.sleep(200);
  }
}

// --- Set up: calibration, layout, pipeline ----------------------------------------------------

const camera = await poll(async () => {
  const cameras: any[] = await api("GET", "/v1/cameras");
  return cameras.find((c) => String(c.name).includes("simulated") && c.frames_endpoint);
}, "the simulated camera");
if (CALIBRATED) {
  await api("PUT", `/v1/cameras/${encodeURIComponent(camera.id)}/calibration`, LENS);
} else {
  // No calibration: clear every stored one (DELETE with no size forgets all) and check that none is left.
  await api("DELETE", `/v1/cameras/${encodeURIComponent(camera.id)}/calibration`);
  const left = await api("GET", `/v1/cameras/${encodeURIComponent(camera.id)}/calibration`);
  if ((left?.calibrations ?? []).length) throw new Error(`the camera still has ${left.calibrations.length} calibration(s)`);
}
const settings = await api("PATCH", `/v1/cameras/${encodeURIComponent(camera.id)}/settings`, { fps: FPS });
console.log(`[scenario] camera frame rate ${FPS} fps: ${JSON.stringify(settings?.applied ?? settings)}`);
const layoutText = await Bun.file(`${DIR}layout.json`).text();
await fetch(`${BASE}/v1/field-layouts/${LAYOUT_ID}`, { method: "DELETE" });
await api("POST", `/v1/field-layouts?id=${LAYOUT_ID}&name=${encodeURIComponent(`Sim ${NAME}`)}`, layoutText);
const templates: any[] = await api("GET", "/v1/pipelines/templates");
const template = templates.find((t) => t.id === "live-map");
if (!template) throw new Error("no live-map template");
// The live mapper's settings: scenario.json's "live_map" object (e.g. {"calibration_frames": 2400})
// plus SIM_SCENARIO_LIVE_MAP ("calibration_frames=2400,..."), and lens_model above. Integers are
// Int, other numbers Float, lens_model an enum, other text String.
const liveMapConfig: Record<string, unknown> = { ...(scenarioConfig.live_map ?? {}) };
for (const pair of (process.env.SIM_SCENARIO_LIVE_MAP ?? "").split(",").filter(Boolean)) {
  const [key, value] = pair.split("=");
  liveMapConfig[key.trim()] = Number.isFinite(Number(value)) ? Number(value) : value.trim();
}
if (LENS_MODEL) liveMapConfig.lens_model = LENS_MODEL;
if (Object.keys(liveMapConfig).length) {
  const mapper = template.graph.graph.nodes.find((n: any) => n.bundle === "eidos:mapping.live_map");
  if (!mapper) throw new Error("the live-map template has no eidos:mapping.live_map node");
  for (const [key, value] of Object.entries(liveMapConfig)) {
    const typed =
      key === "lens_model"
        ? { type: "Enum", value: { name: value, value: null } }
        : typeof value === "number"
          ? Number.isInteger(value)
            ? { type: "Int", value }
            : { type: "Float", value }
          : { type: "String", value: String(value) };
    mapper.const_inputs = (mapper.const_inputs ?? []).filter((c: any) => c[0] !== key);
    mapper.const_inputs.push([key, typed]);
  }
}
await fetch(`${BASE}/v1/pipelines/${PIPELINE_ID}`, { method: "DELETE" });
await api("POST", "/v1/pipelines", {
  id: PIPELINE_ID,
  name: `Sim scenario ${NAME}`,
  graph: template.graph,
  bindings: { [template.camera_input]: { resource_id: camera.id, pyramid: template.pyramid } },
  field_layout: LAYOUT_ID,
  enabled: true,
});
console.log(`[scenario] ${NAME}: live-map on ${camera.id} with layout ${LAYOUT_ID}, ${CALIBRATED ? "calibrated from the file" : "uncalibrated (the mapper estimates it)"}${Object.keys(liveMapConfig).length ? `, live_map ${JSON.stringify(liveMapConfig)}` : ""}, recording ${SECONDS} s`);

// --- Record -----------------------------------------------------------------------------------

const lines: string[] = [];
const timingLines: string[] = [];
// SIM_SCENARIO_S of scene time: the frames' own timestamps (under Styx's lockstep clock the sim
// runs slower than wall time), with a wall-clock cap of SIM_SCENARIO_WALL_S (default 20x).
const WALL_S = Number(process.env.SIM_SCENARIO_WALL_S ?? SECONDS * 20);
let firstTs: number | undefined;
let wallStopped = false;
await new Promise<void>((resolve, reject) => {
  const socket = new WebSocket(`${WS_BASE}/v1/live/ws?pipelines=${PIPELINE_ID}&ports=pose,timing`);
  const finish = () => {
    clearTimeout(timer);
    socket.close();
    resolve();
  };
  const timer = setTimeout(() => {
    wallStopped = true;
    finish();
  }, WALL_S * 1000);
  socket.onmessage = (event) => {
    if (typeof event.data !== "string") return;
    for (const line of event.data.split("\n").filter(Boolean)) {
      (line.includes('"port":"timing"') ? timingLines : lines).push(line);
      const ts = Number(/"frame_ts_ns":(\d+)/.exec(line)?.[1]);
      if (!Number.isFinite(ts)) continue;
      firstTs ??= ts;
      if (ts - firstTs >= SECONDS * 1e9) return finish();
    }
  };
  socket.onerror = () => {
    clearTimeout(timer);
    reject(new Error("live WebSocket failed"));
  };
});
if (wallStopped) console.log(`[scenario] stopped at the wall-clock cap (${WALL_S} s) before ${SECONDS} s of scene time`);
const telemetry = (await api("GET", `/v1/pipelines/${PIPELINE_ID}`)).telemetry ?? {};
await Bun.write(`${OUT}/poses.jsonl`, lines.join("\n") + "\n");
await Bun.write(`${OUT}/timing.jsonl`, timingLines.join("\n") + "\n");

// --- Truth ------------------------------------------------------------------------------------

// Styx's truth (truth.ts): the camera in the field (WPILib frame) and each tag's corners in pixels.
const truth: TruthFrame[] = readTruth(await Bun.file(TRUTH).text()).sort((a, b) => a.ts - b.ts);
const period = (truth[truth.length - 1].ts - truth[0].ts) / Math.max(1, truth.length - 1);
function nearestTruth(ts: number): TruthFrame | undefined {
  let lo = 0;
  let hi = truth.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (truth[mid].ts < ts) lo = mid + 1;
    else hi = mid;
  }
  let best: TruthFrame | undefined;
  for (const candidate of [truth[lo - 1], truth[lo]]) if (candidate && (!best || Math.abs(candidate.ts - ts) < Math.abs(best.ts - ts))) best = candidate;
  return best && Math.abs(best.ts - ts) <= period / 4 ? best : undefined;
}

// The phase of each truth frame (see the header): is a tag's corner in the image?
const inImage = ([x, y]: [number, number]) => x >= 0 && x < LENS.width && y >= 0 && y < LENS.height;
const phaseOf = new Map<TruthFrame, Phase>();
let seenTagFree = false;
for (const frame of truth) {
  const visible = frame.tags.some((tag) => tag.corners.some(inImage));
  phaseOf.set(frame, visible ? (seenTagFree ? "tags again" : "tags") : "tag-free");
  if (!visible) seenTagFree = true;
}

// The layout's tags: centre and normal (a WPILib tag faces its +X).
const layoutTags = new Map<number, { centre: Vec3; normal: Vec3 }>();
for (const tag of JSON.parse(layoutText).tags) {
  const { W: w, X: x, Y: y, Z: z } = tag.pose.rotation.quaternion;
  const normal: Vec3 = [1 - 2 * (y * y + z * z), 2 * (x * y + w * z), 2 * (x * z - w * y)];
  layoutTags.set(tag.ID, { centre: [tag.pose.translation.x, tag.pose.translation.y, tag.pose.translation.z], normal });
}
const sub = (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const norm = (a: Vec3) => Math.hypot(a[0], a[1], a[2]);
function viewingAngleDeg(id: number, cameraAt: Vec3): number | undefined {
  const tag = layoutTags.get(id);
  if (!tag) return undefined;
  const ray = sub(cameraAt, tag.centre);
  const cos = (ray[0] * tag.normal[0] + ray[1] * tag.normal[1] + ray[2] * tag.normal[2]) / norm(ray);
  return (Math.acos(Math.min(1, Math.max(-1, cos))) * 180) / Math.PI;
}
function sidePx(corners: [number, number][]): number {
  let sum = 0;
  for (let i = 0; i < 4; i++) sum += Math.hypot(corners[(i + 1) % 4][0] - corners[i][0], corners[(i + 1) % 4][1] - corners[i][1]);
  return sum / 4;
}

// The estimate's translation, from Eidos's Se3Transform in whichever form it serializes.
function translationOf(transform: any): Vec3 | undefined {
  if (!transform) return undefined;
  const t = transform.translation ?? transform.t;
  if (Array.isArray(t) && t.length === 3) return t as Vec3;
  if (t && typeof t.x === "number") return [t.x, t.y, t.z];
  const m = transform.matrix ?? transform.m;
  if (Array.isArray(m) && m.length >= 3 && Array.isArray(m[0])) return [m[0][3], m[1][3], m[2][3]];
  return undefined;
}

// --- Fit ----------------------------------------------------------------------------------------

// A mapper that never aligned to the layout (`alignment.aligned` false) reports poses in its own
// map frame. Then the estimates are compared after the rigid transform (no scale) that best fits
// them to the truth (Horn's quaternion method), and the report says so. Jumps need no fit.
interface Sample {
  parsed: any;
  pose: any;
  frame?: TruthFrame;
  estimate?: Vec3;
}
const samples: Sample[] = lines.map((line) => {
  const parsed = JSON.parse(line);
  const pose = parsed.value ?? {};
  const frame = nearestTruth(Number(parsed.frame_ts_ns));
  return { parsed, pose, frame, estimate: translationOf((pose.pose ?? pose.estimate)?.reference_from_camera) };
});
const aligned = samples.some((sample) => sample.pose.alignment?.aligned);
const fitted = samples.filter((sample) => sample.frame && sample.estimate) as Required<Sample>[];
type Mat3 = [Vec3, Vec3, Vec3];
function rigidFit(from: Vec3[], to: Vec3[]): (p: Vec3) => Vec3 {
  const mean = (points: Vec3[]) => points.reduce((m, p) => [m[0] + p[0] / points.length, m[1] + p[1] / points.length, m[2] + p[2] / points.length] as Vec3, [0, 0, 0] as Vec3);
  const [mf, mt] = [mean(from), mean(to)];
  const S: number[][] = [0, 1, 2].map(() => [0, 0, 0]);
  from.forEach((p, i) => {
    const [a, b] = [sub(p, mf), sub(to[i], mt)];
    for (let r = 0; r < 3; r++) for (let c = 0; c < 3; c++) S[r][c] += a[r] * b[c];
  });
  const [[xx, xy, xz], [yx, yy, yz], [zx, zy, zz]] = S;
  const N = [
    [xx + yy + zz, yz - zy, zx - xz, xy - yx],
    [yz - zy, xx - yy - zz, xy + yx, zx + xz],
    [zx - xz, xy + yx, -xx + yy - zz, yz + zy],
    [xy - yx, zx + xz, yz + zy, -xx - yy + zz],
  ];
  // The largest eigenvector of N, by power iteration on N shifted to positive definite.
  const shift = N.reduce((m, row) => m + row.reduce((n, v) => n + Math.abs(v), 0), 0);
  let q = [1, 0, 0, 0];
  for (let i = 0; i < 200; i++) {
    const next = N.map((row, r) => row.reduce((sum, v, c) => sum + v * q[c], 0) + shift * q[r]);
    const length = Math.hypot(...next);
    q = next.map((v) => v / length);
  }
  const [w, x, y, z] = q;
  const R: Mat3 = [
    [1 - 2 * (y * y + z * z), 2 * (x * y - w * z), 2 * (x * z + w * y)],
    [2 * (x * y + w * z), 1 - 2 * (x * x + z * z), 2 * (y * z - w * x)],
    [2 * (x * z - w * y), 2 * (y * z + w * x), 1 - 2 * (x * x + y * y)],
  ];
  return (p: Vec3) => {
    const d = sub(p, mf);
    return [0, 1, 2].map((r) => R[r][0] * d[0] + R[r][1] * d[1] + R[r][2] * d[2] + mt[r]) as Vec3;
  };
}
const toLayout = aligned || fitted.length < 3 ? (p: Vec3) => p : rigidFit(fitted.map((s) => s.estimate), fitted.map((s) => s.frame.position));

// --- Report -----------------------------------------------------------------------------------

const rows: string[] = ["frame_ts_ns,state,source,aligned,tags,side_px,view_deg,true_x,true_y,true_z,est_x,est_y,est_z,error_cm,jump_cm,sigma_t_cm,phase,cal_estimated,cal_rms_px,cal_views,cal_frames,cal_model,cal_fx,cal_fy,cal_cx,cal_cy,nees,weak_source,weak_x,weak_y,weak_z,weak_error_cm,weak_nees"];
const errors: number[] = [];
let jumps = 0;
let worstJump = 0;
let unmatched = 0;
let posed = 0;
const states = new Map<string, number>();
// Position NEES (normalised estimation error squared): e' Σ⁻¹ e with Σ the translation block of
// the pose's 6x6 covariance ([rx, ry, rz, tx, ty, tz], row-major, the reference frame). An
// honest covariance gives a mean near 3. Only meaningful in the layout frame (aligned).
function positionNees(error: Vec3, covariance: unknown): number {
  if (!Array.isArray(covariance) || covariance.length !== 36) return Number.NaN;
  const c = (i: number, j: number) => Number(covariance[(i + 3) * 6 + (j + 3)]);
  const [a, b, d, e, g, h, k, l, m] = [c(0, 0), c(0, 1), c(0, 2), c(1, 0), c(1, 1), c(1, 2), c(2, 0), c(2, 1), c(2, 2)];
  const det = a * (g * m - h * l) - b * (e * m - h * k) + d * (e * l - g * k);
  if (!(Math.abs(det) > 0)) return Number.NaN;
  const inv = [
    [(g * m - h * l) / det, (d * l - b * m) / det, (b * h - d * g) / det],
    [(h * k - e * m) / det, (a * m - d * k) / det, (d * e - a * h) / det],
    [(e * l - g * k) / det, (b * k - a * l) / det, (a * g - b * e) / det],
  ];
  let sum = 0;
  for (let i = 0; i < 3; i++) for (let j = 0; j < 3; j++) sum += error[i] * inv[i][j] * error[j];
  return sum;
}
const neesMain: number[] = [];
const weakErrors: number[] = [];
const weakNees: number[] = [];
let weakFrames = 0;
const errorsBySize = new Map<string, number[]>();
const errorsByPhase = new Map<Phase, number[]>();
const jumpsBySize = new Map<string, number>();
let previous: { truth: Vec3; estimate: Vec3 } | undefined;
for (const { parsed, pose, frame, estimate: raw } of samples) {
  states.set(pose.state, (states.get(pose.state) ?? 0) + 1);
  if (!frame) {
    unmatched += 1;
    continue;
  }
  const solved = pose.pose ?? pose.estimate;
  const estimate = raw && toLayout(raw);
  const sizes = frame.tags.map((t) => sidePx(t.corners));
  const angles = frame.tags.map((t) => viewingAngleDeg(t.id, frame.position)).filter((a): a is number => a !== undefined);
  const side = sizes.length ? Math.min(...sizes) : Number.NaN;
  const view = angles.length ? Math.max(...angles) : Number.NaN;
  const bucket = Number.isNaN(side) ? "no tag" : side < 30 ? "<30 px" : side < 45 ? "30-45 px" : ">=45 px";
  const phase = phaseOf.get(frame) as Phase;
  const calibrationReport = pose.calibration ?? {};
  let error = Number.NaN;
  let jump = Number.NaN;
  if (estimate) {
    posed += 1;
    error = norm(sub(estimate, frame.position)) * 100;
    errors.push(error);
    errorsBySize.set(bucket, [...(errorsBySize.get(bucket) ?? []), error]);
    errorsByPhase.set(phase, [...(errorsByPhase.get(phase) ?? []), error]);
    if (previous) {
      jump = norm(sub(sub(estimate, previous.estimate), sub(frame.position, previous.truth))) * 100;
      if (jump > JUMP_CM) {
        jumps += 1;
        jumpsBySize.set(bucket, (jumpsBySize.get(bucket) ?? 0) + 1);
      }
      worstJump = Math.max(worstJump, jump);
    }
    previous = { truth: frame.position, estimate };
  } else {
    previous = undefined;
  }
  const truthError = (p: Vec3): Vec3 => [p[0] - frame.position[0], p[1] - frame.position[1], p[2] - frame.position[2]];
  const nees = aligned && estimate ? positionNees(truthError(estimate), solved?.covariance) : Number.NaN;
  if (Number.isFinite(nees)) neesMain.push(nees);
  const weak = pose.weak;
  const weakAt = weak ? translationOf(weak.reference_from_camera) : undefined;
  let weakError = Number.NaN;
  let weakNeesValue = Number.NaN;
  if (weak) weakFrames += 1;
  if (weakAt && aligned) {
    weakError = norm(truthError(weakAt)) * 100;
    weakErrors.push(weakError);
    weakNeesValue = positionNees(truthError(weakAt), weak.covariance);
    if (Number.isFinite(weakNeesValue)) weakNees.push(weakNeesValue);
  }
  const position = solved?.sigma?.position_m;
  const sigmaT = Array.isArray(position) ? Math.max(...position) * 100 : typeof position === "number" ? position * 100 : Number.NaN;
  const f = (v: number, digits = 3) => (Number.isFinite(v) ? v.toFixed(digits) : "");
  // CSV text field: quoted when it holds a comma, quote or newline (sources such as "lone small tag, not mapped").
  const text = (v: unknown) => {
    const t = String(v ?? "");
    return /[",\n]/.test(t) ? `"${t.replaceAll('"', '""')}"` : t;
  };
  rows.push(
    [
      parsed.frame_ts_ns,
      text(pose.state),
      text(solved?.source),
      pose.alignment?.aligned ? "yes" : "no",
      frame.tags.map((t) => t.id).join(" "),
      f(side, 1),
      f(view, 1),
      ...frame.position.map((v) => f(v)),
      ...(estimate ?? [Number.NaN, Number.NaN, Number.NaN]).map((v) => f(v)),
      f(error, 1),
      f(jump, 1),
      f(sigmaT, 1),
      phase,
      calibrationReport.estimated === undefined ? "" : String(calibrationReport.estimated),
      f(Number(calibrationReport.rms_px ?? Number.NaN), 3),
      calibrationReport.views ?? "",
      calibrationReport.frames ?? "",
      calibrationReport.model ?? "",
      ...["fx", "fy", "cx", "cy"].map((k) => f(Number(calibrationReport[k] ?? Number.NaN), 2)),
      f(nees, 2),
      text(weak?.source),
      ...(weakAt ?? [Number.NaN, Number.NaN, Number.NaN]).map((v) => f(v)),
      f(weakError, 1),
      f(weakNeesValue, 2),
    ].join(","),
  );
}
await Bun.write(`${OUT}/report.csv`, rows.join("\n") + "\n");

const percentile = (values: number[], p: number) => {
  if (!values.length) return Number.NaN;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor(p * sorted.length))];
};
console.log(`[scenario] ${lines.length} pose outputs, ${unmatched} without a truth frame, ${posed} posed; states ${JSON.stringify(Object.fromEntries(states))}`);
{
  const q = (values: number[], p: number) => (values.length ? [...values].sort((x, y) => x - y)[Math.min(values.length - 1, Math.floor(p * values.length))] : Number.NaN);
  console.log(`[scenario] main pose position NEES (honest ≈ 3): median ${q(neesMain, 0.5).toFixed(2)}, p90 ${q(neesMain, 0.9).toFixed(2)} over ${neesMain.length} frames`);
  console.log(`[scenario] weak pose: ${weakFrames} frames; error p50 ${q(weakErrors, 0.5).toFixed(1)} cm, p90 ${q(weakErrors, 0.9).toFixed(1)} cm; NEES median ${q(weakNees, 0.5).toFixed(2)}, p90 ${q(weakNees, 0.9).toFixed(2)}`);
}
console.log(aligned ? "[scenario] the map aligned to the layout: errors in the layout frame" : `[scenario] the map never aligned to the layout: errors after a rigid fit of ${fitted.length} poses to the truth`);
console.log(`[scenario] error cm: p50 ${percentile(errors, 0.5).toFixed(1)}, p95 ${percentile(errors, 0.95).toFixed(1)}, max ${percentile(errors, 1).toFixed(1)}`);
for (const [bucket, values] of [...errorsBySize].sort()) {
  console.log(`[scenario]   tag ${bucket}: ${values.length} frames, error p50 ${percentile(values, 0.5).toFixed(1)} cm, max ${percentile(values, 1).toFixed(1)} cm, jumps ${jumpsBySize.get(bucket) ?? 0}`);
}
console.log(`[scenario] jumps over ${JUMP_CM} cm: ${jumps}, worst ${worstJump.toFixed(1)} cm`);
console.log(`[scenario] pipeline: ${telemetry.ticks_processed ?? "?"} ticks, ${telemetry.ticks_failed ?? "?"} failed, last tick ${telemetry.last_tick_ms ?? "?"} ms (x86, not a board timing)`);
// Each phase's time in the recorded window: the truth frames' own intervals (Styx's frames are not
// evenly spaced), and the poses' errors.
const windowStart = Math.min(...samples.map((s) => Number(s.parsed.frame_ts_ns)));
const windowEnd = Math.max(...samples.map((s) => Number(s.parsed.frame_ts_ns)));
const phaseSeconds = new Map<Phase, number>();
for (let i = 0; i + 1 < truth.length; i++) {
  const phase = phaseOf.get(truth[i]);
  if (!phase || truth[i].ts < windowStart || truth[i].ts > windowEnd) continue;
  phaseSeconds.set(phase, (phaseSeconds.get(phase) ?? 0) + (truth[i + 1].ts - truth[i].ts) / 1e9);
}
for (const phase of ["tags", "tag-free", "tags again"] as Phase[]) {
  const values = errorsByPhase.get(phase) ?? [];
  console.log(
    `[scenario] phase ${phase}: ${(phaseSeconds.get(phase) ?? 0).toFixed(1)} s of truth, ${values.length} posed poses, error p50 ${percentile(values, 0.5).toFixed(1)} cm, p95 ${percentile(values, 0.95).toFixed(1)} cm, max ${percentile(values, 1).toFixed(1)} cm`,
  );
}
// The calibration the node reports: the final pose's `calibration` (provenance, and intrinsics if it has any).
const finalCalibration = [...samples].reverse().find((s) => s.pose.calibration)?.pose.calibration;
const TRUE_INTRINSICS: Record<string, number> = {
  fx: LENS.fx, fy: LENS.fy, cx: LENS.cx, cy: LENS.cy,
  ...Object.fromEntries(Object.entries(LENS.distortion ?? {}).map(([k, v]) => [k, Number(v)])),
};
// Eidos's map_pose.calibration: {estimated, rms_px, views, frames, model, fx, fy, cx, cy,
// distortion[8]} with distortion in the order k1..k6, p1, p2 (a fisheye uses k1..k4).
const DISTORTION_ORDER = ["k1", "k2", "k3", "k4", "k5", "k6", "p1", "p2"];
const reportedValues: Record<string, number> = {};
if (finalCalibration && finalCalibration.model) {
  for (const k of ["fx", "fy", "cx", "cy"]) if (typeof finalCalibration[k] === "number") reportedValues[k] = finalCalibration[k];
  (finalCalibration.distortion ?? []).forEach((v: number, i: number) => {
    if (DISTORTION_ORDER[i] in TRUE_INTRINSICS || v !== 0) reportedValues[DISTORTION_ORDER[i]] = v;
  });
}
const reportedIntrinsics = Object.keys(reportedValues);
const trueText = `true lens (${LENS.model}): ${Object.entries(TRUE_INTRINSICS).map(([k, v]) => `${k} ${Number(v).toPrecision(6)}`).join(", ")}`;
if (!finalCalibration) {
  console.log(`[scenario] reported calibration: none (no pose); ${trueText}`);
} else {
  console.log(
    `[scenario] reported calibration (final pose): estimated ${finalCalibration.estimated}, rms ${Number(finalCalibration.rms_px).toFixed(3)} px, ${finalCalibration.views} tag views over ${finalCalibration.frames} frames`,
  );
  if (reportedIntrinsics.length) {
    const compared = reportedIntrinsics.map((k) => `${k} ${reportedValues[k].toPrecision(6)} vs ${k in TRUE_INTRINSICS ? Number(TRUE_INTRINSICS[k]).toPrecision(6) : "n/a"}`);
    console.log(`[scenario] reported intrinsics (${finalCalibration.model}, reported vs true): ${compared.join(", ")}`);
  } else {
    console.log("[scenario] reported intrinsics: none (the pose's calibration has no intrinsics to compare)");
  }
  console.log(`[scenario] ${trueText}`);
}
console.log(`[scenario] wrote ${OUT}/report.csv and ${OUT}/poses.jsonl`);

// The mapper's own timing per stage (its `timing` output), milliseconds: `stages` as an object of
// numbers or a list of {name, ms}.
const stageTimes = new Map<string, number[]>();
for (const line of timingLines) {
  const stages = JSON.parse(line).value?.stages;
  const entries: [string, unknown][] = Array.isArray(stages) ? stages.map((stage: any) => [stage.name, stage.ms ?? stage.ms_p50 ?? stage.value]) : stages && typeof stages === "object" ? Object.entries(stages) : [];
  for (const [name, ms] of entries) if (typeof ms === "number") stageTimes.set(name, [...(stageTimes.get(name) ?? []), ms]);
}
if (stageTimes.size) {
  const summary = [...stageTimes].map(([name, values]) => `${name} ${percentile(values, 0.5).toFixed(2)}/${percentile(values, 0.95).toFixed(2)}`).join(", ");
  console.log(`[scenario] mapper stages ms p50/p95 (x86): ${summary}`);
} else if (timingLines.length) {
  console.log(`[scenario] mapper timing: ${timingLines.length} lines without numeric stages (see ${OUT}/timing.jsonl)`);
}
