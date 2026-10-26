import { describe, expect, test } from "bun:test";
import { CAMERAS, WORKLOADS } from "../src/lib/api/mock";
import type { CameraStatus, StatusStage } from "../src/lib/api/types";
import { blockingStage, fraction, STATE_TONE, visibleStages, whyOf } from "../src/lib/status/chain";

function stage(id: string, state: StatusStage["state"], reason = id, hint: string | null = null, progress: StatusStage["progress"] = null): StatusStage {
  return { stage: id, label: id[0].toUpperCase() + id.slice(1), state, reason, hint, progress };
}

function chain(stages: StatusStage[], blocking: string | null): CameraStatus {
  return { stages, blocking, summary: "summary" };
}

describe("the status chain", () => {
  const status = chain([stage("camera", "ok"), stage("frames", "blocked", "In use by visionserver"), stage("calibration", "waiting"), stage("map", "off")], "frames");

  test("the blocking stage is the one the backend names", () => {
    expect(blockingStage(status)?.stage).toBe("frames");
    expect(blockingStage(chain([stage("camera", "ok")], null))).toBeNull();
    expect(blockingStage(null)).toBeNull();
  });

  test("a pane shows the stages that apply, in chain order, or only the ones it names", () => {
    expect(visibleStages(status).map((s) => s.stage)).toEqual(["camera", "frames", "calibration"]);
    expect(visibleStages(status, ["calibration", "map"]).map((s) => s.stage)).toEqual(["calibration"]);
    expect(visibleStages(null)).toEqual([]);
  });

  test("the line that says why is the stage's label and reason, with its hint", () => {
    const why = whyOf(stage("frames", "blocked", "In use by visionserver", "Turn it off"));
    expect(why).toEqual({ title: "Frames: In use by visionserver", hint: "Turn it off" });
  });

  test("progress is a fraction of its target, clamped", () => {
    expect(fraction({ value: 40, target: 150, unit: "frames" })).toBeCloseTo(0.2667, 3);
    expect(fraction({ value: 400, target: 150, unit: "frames" })).toBe(1);
    expect(fraction({ value: 3, target: 0, unit: "frames" })).toBe(0);
    expect(fraction(null)).toBe(0);
  });

  test("every state has a tone", () => {
    expect(STATE_TONE).toEqual({ ok: "success", waiting: "info", blocked: "error", off: "neutral" });
  });
});

describe("the simulated robot's status chains", () => {
  const reasonOf = (id: string) => blockingStage(CAMERAS.find((c) => c.resourceId === id)?.status)?.reason;

  test("a busy camera says which program holds it", () => {
    expect(reasonOf("camera.raze-back.cam0")).toBe("In use by visionserver (Limelight vision)");
  });

  test("a tag family the field does not use is the blocking reason of the tags", () => {
    const right = CAMERAS.find((c) => c.resourceId === "camera.raze-right.cam0");
    expect(blockingStage(right?.status)?.stage).toBe("detections");
    expect(reasonOf("camera.raze-right.cam0")).toContain("the pipeline decodes apriltag_16h5");
  });

  test("bootstrapping shows the calibration's progress toward its window", () => {
    const front = CAMERAS.find((c) => c.resourceId === "camera.raze-front.cam0");
    const stop = blockingStage(front?.status);
    expect(stop?.stage).toBe("calibration");
    expect(stop?.progress).toEqual({ value: 40, target: 150, unit: "frames" });
  });

  test("depth that needs motion says so", () => {
    const left = CAMERAS.find((c) => c.resourceId === "camera.raze-left.cam0");
    const stop = blockingStage(left?.status);
    expect(stop?.stage).toBe("depth");
    expect(stop?.hint).toContain("Depth needs motion");
  });

  test("a quarantined pipeline names its failure", () => {
    const workload = WORKLOADS.find((w) => w.id === "tags-left-exp");
    expect(workload?.status?.reason).toBe("Failed: the CLAHE pre-filter raised an error");
  });
});
