// End-to-end smoke test of a running HeliOS sim (tools/sim/up.sh). Run through tools/sim/smoke.sh.
//
// Checks, in order, against helios-api at SIM_URL:
//   1. the API answers;
//   2. the simulated camera resource appears (helios-peripherals serves it from Styx's sim);
//   3. an AprilTag pipeline is created from the API's template on that camera;
//   4. the pipeline ticks, with no failed ticks;
//   5. GET /v1/live/ws delivers the pipeline's outputs, each with frame_ts_ns;
//   6. the camera's preview WebSocket (SPV1 frames) carries capture times that match live outputs,
//      which is what lets a viewer draw each frame's overlays with that frame;
//   7. with SIM_TRUTH (Styx's tag scene ground truth, which up.sh writes): the detections of each
//      frame match the frame's true tags, ids and corners (mean corner error under 2 px).
//
// Checks 5 and 6 record both streams in one window, so the capture times can meet.
// Prints a pass/fail line per check with its time, then the timings and a summary.
// Exit 1 when any check fails. Needs bun (WebSocket and fetch built in); no other dependencies.

const BASE = (process.env.SIM_URL ?? "http://127.0.0.1:5800").replace(/\/$/, "");
const WS_BASE = BASE.replace(/^http/, "ws");
const PIPELINE_ID = process.env.SIM_SMOKE_PIPELINE ?? "sim-smoke-apriltag";
const TEMPLATE_ID = process.env.SIM_SMOKE_TEMPLATE ?? "apriltag-36h11";
const TIMEOUT_MS = Number(process.env.SIM_SMOKE_TIMEOUT_S ?? 60) * 1000;
/** How long the live and preview streams are recorded together for the frame match. */
const WINDOW_MS = Number(process.env.SIM_SMOKE_WINDOW_MS ?? 4000);
/** Ticks the pipeline must run before it counts as ticking. */
const MIN_TICKS = Number(process.env.SIM_SMOKE_MIN_TICKS ?? 20);
/** Styx's ground truth (one JSON line per frame) and the calibration the sim renders with. */
const TRUTH = process.env.SIM_TRUTH ?? "";
const CALIBRATION = process.env.SIM_CALIBRATION ?? new URL("./ov9782-calibration.json", import.meta.url).pathname;
/** The mean corner error a detection may have against the truth, pixels. */
const MAX_CORNER_ERROR_PX = Number(process.env.SIM_SMOKE_MAX_CORNER_PX ?? 2);

const started = performance.now();
const since = () => Math.round(performance.now() - started);

interface Result {
  name: string;
  ok: boolean;
  ms: number;
  detail: string;
}
const results: Result[] = [];
const timings: Record<string, number> = {};

async function check(name: string, run: () => Promise<string>): Promise<boolean> {
  const t0 = performance.now();
  try {
    const detail = await run();
    results.push({ name, ok: true, ms: Math.round(performance.now() - t0), detail });
    return true;
  } catch (error) {
    results.push({ name, ok: false, ms: Math.round(performance.now() - t0), detail: error instanceof Error ? error.message : String(error) });
    return false;
  }
}

async function getJson(path: string): Promise<any> {
  const response = await fetch(BASE + path);
  if (!response.ok) throw new Error(`GET ${path}: HTTP ${response.status} ${await response.text()}`);
  return response.json();
}

/** Polls `read` until it returns a value or the timeout passes; `read` returns undefined to keep waiting. */
async function poll<T>(read: () => Promise<T | undefined>, what: string, timeoutMs = TIMEOUT_MS): Promise<T> {
  const deadline = performance.now() + timeoutMs;
  for (;;) {
    const value = await read();
    if (value !== undefined) return value;
    if (performance.now() > deadline) throw new Error(`timed out after ${Math.round(timeoutMs / 1000)} s waiting for ${what}`);
    await Bun.sleep(200);
  }
}

/** Records the messages of a WebSocket for `ms`, then closes it. Rejects if it cannot connect. */
function record(url: string, ms: number, onMessage: (data: string | ArrayBuffer) => void): Promise<void> {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(url);
    socket.binaryType = "arraybuffer";
    let open = false;
    const timer = setTimeout(() => {
      socket.close();
      resolve();
    }, ms);
    socket.onopen = () => {
      open = true;
    };
    socket.onmessage = (event) => onMessage(event.data as string | ArrayBuffer);
    socket.onerror = () => {
      clearTimeout(timer);
      reject(new Error(open ? `WebSocket ${url} failed` : `WebSocket ${url} did not connect`));
    };
  });
}

// Frame timestamps are u64 nanoseconds: beyond 2^53, so they stay strings (never JSON.parse numbers).
function frameTimestampsOf(text: string): string[] {
  return [...text.matchAll(/"frame_ts_ns":(\d+)/g)].map((match) => match[1]);
}

let cameraId = "";

// 1. The API answers.
await check("api health", async () => {
  const health = await getJson("/v1/health");
  timings.api_ready_ms = since();
  return `version ${health.version ?? "?"}, api ${health.api_version ?? "?"}`;
});

// 2. The camera resource appears.
const cameraOk = await check("sim camera appears", async () => {
  const camera = await poll(async () => {
    const cameras: any[] = await getJson("/v1/cameras");
    return cameras.find((c) => String(c.name).includes("simulated") && c.frames_endpoint);
  }, "a simulated camera with a frame service");
  cameraId = camera.id;
  timings.camera_appears_ms = since();
  return `${camera.name} (${camera.id}), health ${camera.health}`;
});

// The camera gets the calibration the sim renders with, so the pipeline's poses are calibrated.
if (cameraOk) {
  await check("camera calibrated like the sim's lens", async () => {
    const calibration = await Bun.file(CALIBRATION).json();
    const response = await fetch(`${BASE}/v1/cameras/${encodeURIComponent(cameraId)}/calibration`, { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify(calibration) });
    if (!response.ok) throw new Error(`PUT calibration: HTTP ${response.status} ${await response.text()}`);
    return `${calibration.model} ${calibration.width}x${calibration.height}, f ${calibration.fx.toFixed(1)} px`;
  });
}

// 3. The AprilTag pipeline, from the template, on the camera.
let pipelineOk = false;
if (cameraOk) {
  pipelineOk = await check("pipeline created from template", async () => {
    const templates: any[] = await getJson("/v1/pipelines/templates");
    const template = templates.find((t) => t.id === TEMPLATE_ID);
    if (!template) throw new Error(`no template ${TEMPLATE_ID}`);
    // A rerun replaces the last smoke pipeline (it is this script's own).
    await fetch(`${BASE}/v1/pipelines/${PIPELINE_ID}`, { method: "DELETE" });
    const body = {
      id: PIPELINE_ID,
      name: "Sim smoke (AprilTag)",
      graph: template.graph,
      bindings: { [template.camera_input]: { resource_id: cameraId, pyramid: template.pyramid } },
      enabled: true,
    };
    const response = await fetch(`${BASE}/v1/pipelines`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
    if (!response.ok) throw new Error(`POST /v1/pipelines: HTTP ${response.status} ${await response.text()}`);
    timings.pipeline_created_ms = since();
    return `${template.id} (${template.family}, ${template.variant}), input ${template.camera_input}, pyramid ${template.pyramid}`;
  });
}

// 4. The pipeline ticks, with no failed ticks.
if (pipelineOk) {
  await check("pipeline ticks, 0 failed", async () => {
    const telemetry = await poll(async () => {
      const pipeline = await getJson(`/v1/pipelines/${PIPELINE_ID}`);
      if (pipeline.state === "failed") throw new Error(`pipeline failed: ${JSON.stringify(pipeline.session ?? pipeline.state)}`);
      const t = pipeline.telemetry;
      return t && t.ticks_processed >= MIN_TICKS ? t : undefined;
    }, `${MIN_TICKS} ticks`);
    timings.first_ticks_ms = since();
    if (telemetry.ticks_failed !== 0) throw new Error(`ticks_failed ${telemetry.ticks_failed}: ${telemetry.last_failure ?? telemetry.last_error ?? "no message"}`);
    return `${telemetry.ticks_processed} ticks, ${telemetry.ticks_failed} failed, ${telemetry.fps} fps, last tick ${telemetry.last_tick_ms} ms`;
  });
}

// 5 and 6. Live outputs and preview frames, recorded in one window so their capture times can match.
if (pipelineOk) {
  const decoder = new TextDecoder();
  const liveTimestamps = new Set<string>();
  const previewTimestamps: string[] = [];
  const detectionsByFrame = new Map<number, { id: number; corners: { x: number; y: number }[] }[]>();
  let liveLines = 0;
  let previewFirstMs: number | undefined;
  const live = record(`${WS_BASE}/v1/live/ws?pipelines=${PIPELINE_ID}`, WINDOW_MS, (data) => {
    if (typeof data !== "string") return;
    for (const line of data.split("\n").filter(Boolean)) {
      liveLines += 1;
      for (const ts of frameTimestampsOf(line)) liveTimestamps.add(ts);
      if (line.includes('"port":"detections"')) {
        try {
          const parsed = JSON.parse(line);
          if (typeof parsed.frame_ts_ns === "number") detectionsByFrame.set(parsed.frame_ts_ns, parsed.value?.detections ?? []);
        } catch {
          // A line that does not parse is counted above and left out here.
        }
      }
    }
  });
  const preview = record(`${WS_BASE}/v1/cameras/${encodeURIComponent(cameraId)}/preview/ws`, WINDOW_MS, (data) => {
    // Styx SPV1 (docs/preview.md): magic "SPV1" at 0, header length u16 at 4, capture timestamp u64 at 16.
    if (!(data instanceof ArrayBuffer) || data.byteLength < 32 || decoder.decode(new Uint8Array(data, 0, 4)) !== "SPV1") return;
    previewFirstMs ??= since();
    previewTimestamps.push(new DataView(data).getBigUint64(16, true).toString());
  });
  const [liveRun, previewRun] = await Promise.allSettled([live, preview]);

  await check("live outputs carry frame_ts_ns", async () => {
    if (liveRun.status === "rejected") throw liveRun.reason;
    if (liveLines === 0) throw new Error(`no output lines in ${WINDOW_MS} ms`);
    if (liveTimestamps.size === 0) throw new Error(`${liveLines} lines, none with a frame_ts_ns`);
    timings.first_live_ms = since();
    return `${liveLines} lines in ${WINDOW_MS} ms, ${liveTimestamps.size} distinct frame times`;
  });

  await check("preview frames match live outputs", async () => {
    if (previewRun.status === "rejected") throw previewRun.reason;
    if (previewTimestamps.length === 0) throw new Error(`no SPV1 frames in ${WINDOW_MS} ms`);
    const matches = previewTimestamps.filter((ts) => liveTimestamps.has(ts)).length;
    if (matches === 0) throw new Error(`${previewTimestamps.length} preview frames, none matches a live frame_ts_ns`);
    timings.first_preview_ms = previewFirstMs ?? 0;
    return `${matches} of ${previewTimestamps.length} preview frames match a live output (${new Set(previewTimestamps).size} distinct capture times)`;
  });

  if (TRUTH) {
    await check("detections match the ground truth", async () => {
      // Styx's truth: {timestamp_ns, tags: [{id, corners: [[x, y] x4]}]} per frame, frame start on
      // the same clock as the outputs' frame_ts_ns.
      const truth = new Map<number, { id: number; corners: [number, number][] }[]>();
      for (const line of (await Bun.file(TRUTH).text()).split("\n").filter(Boolean)) {
        const frame = JSON.parse(line);
        truth.set(frame.timestamp_ns, frame.tags ?? []);
      }
      let frames = 0;
      let tags = 0;
      let missed = 0;
      let errorSum = 0;
      let corners = 0;
      for (const [ts, detections] of detectionsByFrame) {
        const expected = truth.get(ts);
        if (!expected) continue;
        frames += 1;
        for (const tag of expected) {
          tags += 1;
          const found = detections.find((d) => d.id === tag.id);
          if (!found) {
            missed += 1;
            continue;
          }
          // Corner order may differ: each true corner against its nearest detected one.
          for (const [x, y] of tag.corners) {
            errorSum += Math.min(...found.corners.map((c) => Math.hypot(c.x - x, c.y - y)));
            corners += 1;
          }
        }
      }
      if (frames === 0) throw new Error(`no detection frame matched a truth frame (${detectionsByFrame.size} detection frames, ${truth.size} truth frames)`);
      const mean = corners ? errorSum / corners : Number.NaN;
      if (missed > 0) throw new Error(`${missed} of ${tags} true tags not detected over ${frames} frames`);
      if (!(mean <= MAX_CORNER_ERROR_PX)) throw new Error(`mean corner error ${mean.toFixed(2)} px over ${corners} corners (max ${MAX_CORNER_ERROR_PX})`);
      return `${frames} frames, ${tags} tags all detected, mean corner error ${mean.toFixed(2)} px`;
    });
  }
}

// Summary.
const width = Math.max(...results.map((r) => r.name.length));
for (const r of results) {
  console.log(`${r.ok ? "PASS" : "FAIL"}  ${r.name.padEnd(width)}  ${String(r.ms).padStart(6)} ms  ${r.detail}`);
}
const passed = results.filter((r) => r.ok).length;
console.log("");
console.log("timings (ms since start):");
for (const [key, value] of Object.entries(timings)) console.log(`  ${key.padEnd(20)} ${value}`);
console.log(`  ${"total".padEnd(20)} ${since()}`);
console.log("");
console.log(`${passed}/${results.length} checks passed${passed === results.length ? "" : " (SMOKE FAILED)"}`);
process.exit(passed === results.length && results.length > 0 ? 0 : 1);
