import { describe, expect, test } from "bun:test";
import { mockMapView } from "../src/lib/api/mock";
import { parseMapPose, parseMapSnapshot, parseMapTiming, TimingWindow } from "../src/lib/vision/map";

/** Little-endian bytes of `values`, as the host sends them: a JSON array of byte values. */
const bytes = (view: DataView) => Array.from(new Uint8Array(view.buffer));

describe("live map outputs", () => {
  test("decodes the packed point buffers little-endian, from JSON byte arrays", () => {
    const ids = new DataView(new ArrayBuffer(8));
    ids.setUint32(0, 3, true);
    ids.setUint32(4, 7, true);
    const xyz = new DataView(new ArrayBuffer(24));
    [0.5, -0.25, 2, 1, 0, 4.5].forEach((v, i) => xyz.setFloat32(i * 4, v, true));
    const observations = new DataView(new ArrayBuffer(4));
    observations.setUint16(0, 2, true);
    observations.setUint16(2, 9, true);
    const snapshot = parseMapSnapshot({
      seq: 4,
      run_id: 9,
      tag_size_m: 0.1651,
      aligned: false,
      estimated: true,
      trajectory: [],
      keyframes: [],
      tags: [],
      points: {
        count: 2,
        min_xyz: [0.5, -0.25, 2],
        max_xyz: [1, 0, 4.5],
        ids: bytes(ids),
        xyz: bytes(xyz),
        observations: bytes(observations),
        grey: [40, 201],
      },
    });
    expect(snapshot).not.toBeNull();
    const points = snapshot!.points;
    expect(points.count).toBe(2);
    expect(Array.from(points.ids)).toEqual([3, 7]);
    expect(Array.from(points.xyz)).toEqual([0.5, -0.25, 2, 1, 0, 4.5]);
    expect(Array.from(points.observations)).toEqual([2, 9]);
    expect(Array.from(points.grey)).toEqual([40, 201]);
  });

  test("cuts short point buffers to whole rows", () => {
    const points = parseMapSnapshot({ trajectory: [], points: { count: 2, ids: [1, 0, 0, 0, 9], xyz: [], observations: [], grey: [1, 2] } })!.points;
    expect(points.count).toBe(0);
    expect(points.ids.length).toBe(0);
  });

  test("an invalid pose has no estimate, and a valid one carries its covariance and sigma", () => {
    const invalid = parseMapPose({ frame: 3, t_ns: 9, state: "bootstrapping", reason: "calibrating 3/150 frames", valid: false, pose: {}, calibration: {}, alignment: {} });
    expect(invalid?.valid).toBe(false);
    expect(invalid?.estimate).toBeNull();

    const valid = parseMapPose({
      frame: 4,
      t_ns: 10,
      state: "tracking",
      reason: "tags",
      valid: true,
      pose: {
        reference_from_camera: { translation: { x: 1, y: 2, z: 3 }, rotation: { w: 1, x: 0, y: 0, z: 0 } },
        covariance: new Array(36).fill(0.1),
        sigma: { position_m: [0.1, 0.2, 0.3], rotation_deg: [1, 2, 3] },
        source: "tags",
        rms_px: 0.4,
        points: 12,
        hypotheses: { count: 1, chosen: 0, why: "", weights: [1] },
        rig: { reference_from_rig: { translation: { x: 0, y: 0, z: 0 }, rotation: { w: 1, x: 0, y: 0, z: 0 } }, covariance: [] },
      },
      calibration: { estimated: true, rms_px: 0.4, views: 90, frames: 150 },
      alignment: { aligned: true, matched_tags: 2, residual_m: 0.01 },
    });
    expect(valid?.estimate?.reference_from_camera.translation).toEqual({ x: 1, y: 2, z: 3 });
    expect(valid?.estimate?.covariance.length).toBe(36);
    expect(valid?.estimate?.sigma.position_m).toEqual([0.1, 0.2, 0.3]);
    expect(valid?.alignment).toEqual({ aligned: true, matched_tags: 2, residual_m: 0.01 });
  });

  test("the timing output keeps its stages, and the window takes the p50 of the last second", () => {
    const timing = parseMapTiming({ frame: 1, t_ns: 2, frame_ms: 3.5, map_ms: 1.25, stages: [{ name: "solve", ms: 0.8 }] });
    expect(timing?.stages).toEqual([{ name: "solve", ms: 0.8 }]);

    const window = new TimingWindow(1000);
    [1, 5, 3].forEach((ms, i) => window.push(i * 100, ms, ms / 2));
    expect(window.p50(300)).toEqual({ frameMs: 3, mapMs: 1.5, count: 3 });
    // The samples older than the window drop out.
    expect(window.p50(1150)?.count).toBe(1);
    expect(window.p50(5000)).toBeNull();
  });

  test("the mock live map is a valid view of the same shapes", () => {
    const view = mockMapView(40);
    expect(view.pose?.valid).toBe(true);
    expect(view.pose?.estimate?.reference_from_camera.rotation.w).toBeGreaterThan(-1.0001);
    expect(view.snapshot?.points.count).toBeGreaterThan(1000);
    expect(view.snapshot?.points.xyz.length).toBe(view.snapshot!.points.count * 3);
    expect(view.snapshot?.tags.length).toBe(6);
    expect(view.timing?.stages.length).toBeGreaterThan(0);
  });
});
