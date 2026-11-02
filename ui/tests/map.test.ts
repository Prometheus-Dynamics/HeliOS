import { describe, expect, test } from "bun:test";
import { mockMapView } from "../src/lib/api/mock";
import { StreamedTrajectory, TRAJECTORY_BYTES_PER_POSE, parseMapPose, parseMapSnapshot, parseMapTiming, TimingWindow, type MapTrajectoryDelta } from "../src/lib/vision/map";

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
    const points = parseMapSnapshot({ points: { count: 2, ids: [1, 0, 0, 0, 9], xyz: [], observations: [], grey: [1, 2] } })!.points;
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

  test("the mock live map's path comes from its trajectory stream, not the snapshot", () => {
    const view = mockMapView(40);
    expect(view.trajectory?.poses().length).toBeGreaterThan(0);
    expect(view.snapshot && "trajectory" in view.snapshot).toBe(false);
  });
});

/** A trajectory delta of poses at `frames` (keyframes at the given frames), at `revision` 0. */
function delta(frames: number[], keyframes: number[] = [], extra: Partial<MapTrajectoryDelta> = {}): MapTrajectoryDelta {
  const identity = { translation: { x: 0, y: 0, z: 0 }, rotation: { w: 1, x: 0, y: 0, z: 0 } };
  return {
    run_id: 1,
    epoch: 1,
    from_seq: frames[0] ?? 0,
    to_seq: frames[frames.length - 1] ?? 0,
    poses: frames.map((frame) => ({ frame, pose: { ...identity, translation: { x: frame, y: 0, z: 0 } }, sigma: { position_m: [], rotation_deg: [] }, keyframe: keyframes.includes(frame), revision: 0 })),
    corrections: [],
    ...extra,
  };
}

describe("streamed trajectory: decimation and the byte cap", () => {
  test("every fourth frame is kept by default, and keyframes always are", () => {
    const store = new StreamedTrajectory();
    store.apply(delta([0, 1, 2, 3, 4, 5, 6, 7, 8], [5]));
    expect(store.poses().map((p) => p.frame)).toEqual([0, 4, 5, 8]);
  });

  test("the stream repeats its last ticks: a pose is stored once", () => {
    const store = new StreamedTrajectory({ decimate: 1 });
    store.apply(delta([0, 1, 2]));
    const version = store.version;
    store.apply(delta([1, 2, 3]));
    expect(store.poses().map((p) => p.frame)).toEqual([0, 1, 2, 3]);
    expect(store.version).toBeGreaterThan(version);
    const again = store.version;
    store.apply(delta([1, 2, 3]));
    expect(store.version).toBe(again, "nothing new is a change to nobody");
  });

  test("past the byte cap the oldest plain poses go first, then the oldest keyframes, and dropped frames do not come back", () => {
    const store = new StreamedTrajectory({ decimate: 1, maxBytes: 4 * TRAJECTORY_BYTES_PER_POSE });
    store.apply(delta([0, 1, 2, 3, 4, 5], [1]));
    expect(store.poses().map((p) => p.frame)).toEqual([1, 3, 4, 5], "keyframe 1 stays, the oldest plain poses went");
    expect(store.bytes).toBeLessThanOrEqual(4 * TRAJECTORY_BYTES_PER_POSE);
    // The stream's repeat of frame 0 (dropped) is ignored; a new frame is kept and trims again.
    store.apply(delta([0, 1, 2, 3, 4, 5, 6]));
    expect(store.poses().map((p) => p.frame)).toEqual([1, 4, 5, 6]);
  });

  test("a frame promoted to a keyframe after it was streamed is marked, stored or not", () => {
    const store = new StreamedTrajectory();
    store.apply(delta([0, 1, 2, 3, 4, 5]));
    expect(store.keyframes().map((p) => p.frame)).toEqual([]);
    const version = store.version;
    // Frame 4 was kept as a decimated pose, frame 5 was not kept at all.
    store.apply(delta([4, 5], [4, 5]));
    expect(store.keyframes().map((p) => p.frame)).toEqual([4, 5]);
    expect(store.poses().map((p) => p.frame)).toEqual([0, 4, 5]);
    expect(store.version).toBeGreaterThan(version);
    // A later plain repeat does not demote it.
    store.apply(delta([4, 5]));
    expect(store.keyframes().map((p) => p.frame)).toEqual([4, 5]);
  });

  test("a keyframe is dropped only when plain poses cannot make the room", () => {
    const store = new StreamedTrajectory({ decimate: 1, maxBytes: 2 * TRAJECTORY_BYTES_PER_POSE });
    store.apply(delta([0, 1, 2], [0, 1, 2]));
    expect(store.poses().map((p) => p.frame)).toEqual([1, 2]);
  });

  test("corrections still move the poses kept, and a new run starts over", () => {
    const store = new StreamedTrajectory({ decimate: 1 });
    store.apply(delta([0, 1, 2]));
    store.apply(delta([3], [], { corrections: [{ epoch: 1, from_seq: 0, to_seq: 2, scale: 1, rotation: { w: 1, x: 0, y: 0, z: 0 }, translation: { x: 10, y: 0, z: 0 }, revision: 1 }] }));
    expect(store.poses().map((p) => p.pose.translation.x)).toEqual([10, 11, 12, 3]);
    store.apply(delta([7], [], { run_id: 2 }));
    expect(store.poses().map((p) => p.frame)).toEqual([7]);
  });
});
