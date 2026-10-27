// The sim's restore check (`changesFrom` in e2e/device.ts): what a test left changed on the device
// is named, field by field, and nothing is named when the device is as found.
import { describe, expect, test } from "bun:test";
import { changesFrom, type DeviceSnapshot, type DevicePipeline } from "../e2e/device";

function pipeline(overrides: Partial<DevicePipeline> = {}): DevicePipeline {
  return {
    id: "default",
    name: "Localization",
    enabled: true,
    state: "running",
    revision: 1,
    graph: { format: "daedalus.graph", schema_version: 1, requires: [{ id: "eidos" }], metadata: {}, graph: { nodes: [], edges: [] } },
    bindings: { frame: { resource_id: "camera.a", output_width: 320, output_height: 200, pyramid: 2 } },
    dictionary: "apriltag_36h11",
    search_mode: "tracked",
    full_search_every: 5,
    loss_full_search_after: 0,
    margin_growth_misses: 6,
    field_layout: null,
    cpu: null,
    nice: null,
    managed: true,
    ...overrides,
  };
}

function snapshot(pipelines: DevicePipeline[]): DeviceSnapshot {
  return { onboarded: true, pipelines, cameras: [{ id: "camera.a", name: "OV9782", persisted: {}, mode: "NV12 640x400", flowing: true }] };
}

describe("changesFrom", () => {
  test("is empty when the device reads as found", () => {
    const before = snapshot([pipeline()]);
    expect(changesFrom(before, snapshot([pipeline({ revision: 9 })]))).toEqual([]);
  });

  test("names a tag family left behind", () => {
    const before = snapshot([pipeline()]);
    const changes = changesFrom(before, snapshot([pipeline({ dictionary: "apriltag_16h5" })]));
    expect(changes).toEqual(['pipeline default: tag family "apriltag_36h11" -> "apriltag_16h5"']);
  });

  test("names a search, an input size, a disabled pipeline, a pipeline added or gone, and kept camera settings", () => {
    const before = snapshot([pipeline()]);
    const after: DeviceSnapshot = {
      onboarded: false,
      pipelines: [pipeline({ search_mode: "full", enabled: false, bindings: { frame: { resource_id: "camera.a" } } }), pipeline({ id: "extra" })],
      cameras: [{ id: "camera.a", name: "OV9782", persisted: { exposure_value: 0.5 }, mode: "NV12 1280x800", flowing: true }],
    };
    const changes = changesFrom(before, after);
    expect(changes).toContain("setup flag: true -> false");
    expect(changes.some((line) => line.startsWith("pipeline default: search mode"))).toBe(true);
    expect(changes.some((line) => line.startsWith("pipeline default: enabled"))).toBe(true);
    expect(changes.some((line) => line.startsWith("pipeline default: input"))).toBe(true);
    expect(changes).toContain("pipeline extra was added");
    expect(changes.some((line) => line.startsWith("camera OV9782: kept settings"))).toBe(true);
  });

  test("names a pipeline that is gone", () => {
    expect(changesFrom(snapshot([pipeline()]), snapshot([]))).toEqual(["pipeline default is gone"]);
  });
});
