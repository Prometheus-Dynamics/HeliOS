import { expect, test } from "bun:test";
import { clockText, dropSummary, durationText, elapsedMs, eidosCameraText, isActive, ringSummary, sizeText, spaceLevel, spaceTone, stateLabel, stateTone, stopReasonText, storageText } from "../src/lib/recording/format";
import type { RecordingStatus, RingStatus } from "../src/lib/api/recording-types";

const MIB = 1 << 20;
const GIB = 1 << 30;

function status(overrides: Partial<RecordingStatus> = {}): RecordingStatus {
  return {
    id: "2026-10-19T09-41-12-front",
    camera: "camera.raze-front.cam0",
    state: "recording",
    started_at_ms: 1_700_000_000_000,
    duration_ms: 10_000,
    dir: "/media/usb/helios/recordings",
    file: "2026-10-19T09-41-12-front.mcap",
    parts: [],
    bytes: 500 * MIB,
    frames_written: 300,
    frames_dropped: 0,
    messages_written: 900,
    messages_dropped: 0,
    free_bytes: 100 * GIB,
    reserve_bytes: 2 * GIB,
    max_fps: 30,
    frames: "raw",
    format: "BYR2 1280x800",
    compression: false,
    stop_reason: null,
    error: null,
    ...overrides,
  };
}

function ring(overrides: Partial<RingStatus> = {}): RingStatus {
  return { camera: "camera.raze-front.cam0", state: "running", error: null, seconds: 10, window_seconds: 10, frames: 300, bytes: 0, budget_bytes: 1024 * MIB, format: "BYR2 1280x800", ...overrides };
}

test("every recording state has a label and a tone", () => {
  expect(stateLabel("recording")).toBe("Recording");
  expect(stateLabel("finishing")).toBe("Finishing");
  expect(stateTone("recording")).toBe("ok");
  expect(stateTone("starting")).toBe("info");
  expect(stateTone("finishing")).toBe("warn");
  expect(stateTone("stopped")).toBe("neutral");
  expect(stateTone("failed")).toBe("err");
});

test("only starting, recording and finishing are active", () => {
  expect(isActive("starting")).toBe(true);
  expect(isActive("recording")).toBe(true);
  expect(isActive("finishing")).toBe(true);
  expect(isActive("stopped")).toBe(false);
  expect(isActive("failed")).toBe(false);
});

test("stop reasons read as sentences, and unknown ones pass through", () => {
  expect(stopReasonText("requested")).toBe("Stopped by you");
  expect(stopReasonText("storage_reserve")).toContain("storage reserve");
  expect(stopReasonText("camera_lost")).toContain("camera");
  expect(stopReasonText("something_new")).toBe("something new");
  expect(stopReasonText(null)).toBeNull();
});

test("the clock shows minutes and seconds, and hours once there are some", () => {
  expect(clockText(0)).toBe("0:00");
  expect(clockText(7_400)).toBe("0:07");
  expect(clockText(12 * 60_000 + 3_000)).toBe("12:03");
  expect(clockText(3_764_000)).toBe("1:02:44");
  expect(clockText(-5)).toBe("0:00");
});

test("elapsed time counts on from the last report while the recording runs, and stops with it", () => {
  const s = status({ duration_ms: 10_000 });
  expect(elapsedMs(s, 1_000, 3_500)).toBe(12_500);
  expect(elapsedMs(status({ state: "stopped", duration_ms: 10_000 }), 1_000, 3_500)).toBe(10_000);
  // A report from the future (clock skew between this and the device) never runs the clock backwards.
  expect(elapsedMs(s, 5_000, 4_000)).toBe(10_000);
});

test("sizes and durations use the shared formatting", () => {
  expect(sizeText(9_420 * MIB)).toBe("9.2 GiB");
  expect(sizeText(512 * 1024)).toBe("512 KiB");
  expect(durationText(45_000)).toBe("45s");
  expect(durationText(192_000)).toBe("3m 12s");
});

test("dropped frames: none is ok, a tiny share is a warning, more is an error", () => {
  expect(dropSummary(1000, 0)).toMatchObject({ tone: "ok", text: "No drops", dropped: 0 });
  const small = dropSummary(999, 1);
  expect(small.tone).toBe("warn");
  expect(small.text).toBe("1 dropped (0.1%)");
  const big = dropSummary(90, 10);
  expect(big.tone).toBe("err");
  expect(big.ratio).toBeCloseTo(0.1);
  expect(big.text).toBe("10 dropped (10%)");
  expect(dropSummary(0, 0).ratio).toBe(0);
});

test("free space is past the reserve, low within twice it, or ok", () => {
  const reserve = 2 * GIB;
  expect(spaceLevel(null, reserve)).toBe("unknown");
  expect(spaceLevel(2 * GIB, reserve)).toBe("reserve");
  expect(spaceLevel(3 * GIB, reserve)).toBe("low");
  expect(spaceLevel(4 * GIB, reserve)).toBe("low");
  expect(spaceLevel(5 * GIB, reserve)).toBe("ok");
  expect(spaceTone("reserve")).toBe("err");
  expect(spaceTone("low")).toBe("warn");
  expect(spaceTone("ok")).toBe("ok");
});

test("the storage line names the error first, then the room left", () => {
  expect(storageText(null, "The recordings directory is not writable")).toEqual({ tone: "err", text: "The recordings directory is not writable" });
  expect(storageText(null, null).text).toBe("No storage yet");
  const roomy = storageText({ dir: "/x", free_bytes: 100 * GIB, reserve_bytes: 2 * GIB }, null);
  expect(roomy.tone).toBe("ok");
  expect(roomy.text).toBe("100 GiB free, 2.0 GiB kept in reserve");
});

test("a capture-last ring reads as full, filling or failed", () => {
  expect(ringSummary(ring())).toEqual({ tone: "ok", text: "Holding the last 10 s", filled: 1 });
  const filling = ringSummary(ring({ window_seconds: 4.6 }));
  expect(filling.tone).toBe("info");
  expect(filling.text).toBe("Filling: 4 of 10 s");
  expect(filling.filled).toBeCloseTo(0.46);
  expect(ringSummary(ring({ state: "failed", error: "no frames", window_seconds: 0 }))).toEqual({ tone: "err", text: "Failed: no frames", filled: 0 });
  expect(ringSummary(ring({ state: "failed", error: null })).text).toBe("Failed");
});

test("the Eidos camera model says whether it is calibrated", () => {
  expect(eidosCameraText("calibrated")).toBe("Calibrated camera model");
  expect(eidosCameraText("nominal")).toContain("not calibrated");
});
