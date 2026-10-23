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
//   SIM_SCENARIO_S (default 64), SIM_SCENARIO_OUT (default: the truth file's directory),
//   SIM_CALIBRATION (default tools/sim/ov9782-calibration.json),
//   SIM_SCENARIO_JUMP_CM (a jump above this counts, default 10).

const BASE = (process.env.SIM_URL ?? "http://127.0.0.1:5800").replace(/\/$/, "");
const WS_BASE = BASE.replace(/^http/, "ws");
const TRUTH = process.env.SIM_TRUTH ?? "";
const NAME = process.env.SIM_SCENARIO ?? "";
const SECONDS = Number(process.env.SIM_SCENARIO_S ?? 64);
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
await api("PUT", `/v1/cameras/${encodeURIComponent(camera.id)}/calibration`, await Bun.file(CALIBRATION).json());
const layoutText = await Bun.file(`${DIR}layout.json`).text();
await fetch(`${BASE}/v1/field-layouts/${LAYOUT_ID}`, { method: "DELETE" });
await api("POST", `/v1/field-layouts?id=${LAYOUT_ID}&name=${encodeURIComponent(`Sim ${NAME}`)}`, layoutText);
const templates: any[] = await api("GET", "/v1/pipelines/templates");
const template = templates.find((t) => t.id === "live-map");
if (!template) throw new Error("no live-map template");
await fetch(`${BASE}/v1/pipelines/${PIPELINE_ID}`, { method: "DELETE" });
await api("POST", "/v1/pipelines", {
  id: PIPELINE_ID,
  name: `Sim scenario ${NAME}`,
  graph: template.graph,
  bindings: { [template.camera_input]: { resource_id: camera.id, pyramid: template.pyramid } },
  field_layout: LAYOUT_ID,
  enabled: true,
});
console.log(`[scenario] ${NAME}: live-map on ${camera.id} with layout ${LAYOUT_ID}, recording ${SECONDS} s`);

// --- Record -----------------------------------------------------------------------------------

const lines: string[] = [];
await new Promise<void>((resolve, reject) => {
  const socket = new WebSocket(`${WS_BASE}/v1/live/ws?pipelines=${PIPELINE_ID}&ports=pose`);
  const timer = setTimeout(() => {
    socket.close();
    resolve();
  }, SECONDS * 1000);
  socket.onmessage = (event) => {
    if (typeof event.data === "string") lines.push(...event.data.split("\n").filter(Boolean));
  };
  socket.onerror = () => {
    clearTimeout(timer);
    reject(new Error("live WebSocket failed"));
  };
});
const telemetry = (await api("GET", `/v1/pipelines/${PIPELINE_ID}`)).telemetry ?? {};
await Bun.write(`${OUT}/poses.jsonl`, lines.join("\n") + "\n");

// --- Truth ------------------------------------------------------------------------------------

interface TruthFrame {
  ts: number;
  position: Vec3;
  tags: { id: number; corners: [number, number][] }[];
}
const truth: TruthFrame[] = (await Bun.file(TRUTH).text())
  .split("\n")
  .filter(Boolean)
  .map((line) => JSON.parse(line))
  .map((f) => ({ ts: f.timestamp_ns, position: f.camera.position as Vec3, tags: f.tags ?? [] }))
  .sort((a, b) => a.ts - b.ts);
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

const rows: string[] = ["frame_ts_ns,state,source,aligned,tags,side_px,view_deg,true_x,true_y,true_z,est_x,est_y,est_z,error_cm,jump_cm,sigma_t_cm"];
const errors: number[] = [];
let jumps = 0;
let worstJump = 0;
let unmatched = 0;
let posed = 0;
const states = new Map<string, number>();
const errorsBySize = new Map<string, number[]>();
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
  let error = Number.NaN;
  let jump = Number.NaN;
  if (estimate) {
    posed += 1;
    error = norm(sub(estimate, frame.position)) * 100;
    errors.push(error);
    errorsBySize.set(bucket, [...(errorsBySize.get(bucket) ?? []), error]);
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
  const position = solved?.sigma?.position_m;
  const sigmaT = Array.isArray(position) ? Math.max(...position) * 100 : typeof position === "number" ? position * 100 : Number.NaN;
  const f = (v: number, digits = 3) => (Number.isFinite(v) ? v.toFixed(digits) : "");
  rows.push(
    [
      parsed.frame_ts_ns,
      pose.state ?? "",
      solved?.source ?? "",
      pose.alignment?.aligned ? "yes" : "no",
      frame.tags.map((t) => t.id).join(" "),
      f(side, 1),
      f(view, 1),
      ...frame.position.map((v) => f(v)),
      ...(estimate ?? [Number.NaN, Number.NaN, Number.NaN]).map((v) => f(v)),
      f(error, 1),
      f(jump, 1),
      f(sigmaT, 1),
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
console.log(aligned ? "[scenario] the map aligned to the layout: errors in the layout frame" : `[scenario] the map never aligned to the layout: errors after a rigid fit of ${fitted.length} poses to the truth`);
console.log(`[scenario] error cm: p50 ${percentile(errors, 0.5).toFixed(1)}, p95 ${percentile(errors, 0.95).toFixed(1)}, max ${percentile(errors, 1).toFixed(1)}`);
for (const [bucket, values] of [...errorsBySize].sort()) {
  console.log(`[scenario]   tag ${bucket}: ${values.length} frames, error p50 ${percentile(values, 0.5).toFixed(1)} cm, max ${percentile(values, 1).toFixed(1)} cm, jumps ${jumpsBySize.get(bucket) ?? 0}`);
}
console.log(`[scenario] jumps over ${JUMP_CM} cm: ${jumps}, worst ${worstJump.toFixed(1)} cm`);
console.log(`[scenario] pipeline: ${telemetry.ticks_processed ?? "?"} ticks, ${telemetry.ticks_failed ?? "?"} failed, last tick ${telemetry.last_tick_ms ?? "?"} ms (x86, not a board timing)`);
console.log(`[scenario] wrote ${OUT}/report.csv and ${OUT}/poses.jsonl`);
