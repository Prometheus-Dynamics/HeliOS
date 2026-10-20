import { describe, expect, test } from "bun:test";
import type { StoredRecording } from "../src/lib/api/types";
import { contextText, newestFirst, parsePath, recordingSource, sourceLabel } from "../src/lib/playback/sources";

describe("typed sources", () => {
  test("an absolute path is a path source, trimmed", () => {
    expect(parsePath("  /data/run-1.mcap  ")).toEqual({ ok: true, source: { path: "/data/run-1.mcap" } });
    expect(parsePath("/data/frames")).toEqual({ ok: true, source: { path: "/data/frames" } });
  });

  test("an empty path asks for one", () => {
    const parsed = parsePath("   ");
    expect(parsed.ok).toBe(false);
  });

  test("a relative path is refused with the reason", () => {
    expect(parsePath("data/run-1.mcap")).toEqual({ ok: false, error: "The path must be absolute, starting with /." });
  });

  test("a recording source and the labels the lists show", () => {
    expect(recordingSource("cam-20260101")).toEqual({ recording: "cam-20260101" });
    expect(sourceLabel({ recording: "cam-20260101" })).toBe("cam-20260101");
    expect(sourceLabel({ path: "/data/frames" })).toBe("/data/frames");
  });
});

describe("recording order", () => {
  const part = (id: string, modified: number): StoredRecording => ({ id, file: `${id}.mcap`, bytes: 1, modified_at_ms: modified, summary: null });

  test("newest first, ties by id", () => {
    const sorted = newestFirst([part("a", 10), part("c", 30), part("b", 30), part("z", 5)]);
    expect(sorted.map((p) => p.id)).toEqual(["c", "b", "a", "z"]);
  });

  test("does not change the list it was given", () => {
    const list = [part("a", 1), part("b", 2)];
    newestFirst(list);
    expect(list.map((p) => p.id)).toEqual(["a", "b"]);
  });
});

describe("run context", () => {
  test("says where the calibration and the mount came from", () => {
    expect(contextText("calibration", { from: "recording" })).toBe("calibration: from the recording");
    expect(contextText("mount", { from: "camera", camera: "cam.a" })).toBe("mount: from live camera cam.a");
    expect(contextText("mount", { from: "none" })).toBe("mount: none (neither the recording nor the live camera has one)");
  });
});
