import { describe, expect, test } from "bun:test";
import { mockPoseView, MOCK_INTRINSICS } from "../src/lib/api/mock";
import { fieldOfView, parseFieldPose, parseTagPoses, rotate, scaleIntrinsics, tagAxes, type Intrinsics } from "../src/lib/vision/pose";
import type { MapTransform } from "../src/lib/vision/map";

const identity: MapTransform = { translation: { x: 0, y: 0, z: 0 }, rotation: { w: 1, x: 0, y: 0, z: 0 } };
const K: Intrinsics = { fx: 100, fy: 100, cx: 320, cy: 240, width: 640, height: 480 };

const pose = (x: number, y: number, z: number, w = 1, qx = 0, qy = 0, qz = 0): MapTransform => ({
  translation: { x, y, z },
  rotation: { w, x: qx, y: qy, z: qz },
});

describe("tag axes", () => {
  test("a tag straight ahead projects its centre to the principal point, and its axes to the image", () => {
    // A tag 2 m straight ahead, unrotated: its +x runs right in the image, +y down, +z toward the camera.
    const axes = tagAxes(pose(0, 0, 2), 0.2, K);
    expect(axes).not.toBeNull();
    const byAxis = Object.fromEntries(axes!.map((a) => [a.axis, a]));
    expect(byAxis.x.x0).toBeCloseTo(320);
    expect(byAxis.x.y0).toBeCloseTo(240);
    // Each axis is one tag edge (0.2 m) long: the x end is 0.2 m right, at 2 m: 100 * 0.2 / 2 = 10 px.
    expect(byAxis.x.x1).toBeCloseTo(330);
    expect(byAxis.y.y1).toBeCloseTo(250);
    // The z end points at the camera: its image point is the centre (it is a point on the optical axis).
    expect(byAxis.z.x1).toBeCloseTo(320);
  });

  test("a tag behind the camera has no axes", () => {
    expect(tagAxes(pose(0, 0, -1), 0.2, K)).toBeNull();
  });

  test("the rotation turns the axes: a quarter turn about z sends the tag's x down the image", () => {
    const q = Math.SQRT1_2;
    // A rotation of 90 degrees about the optical z axis: the tag's x points at image +y.
    const axes = tagAxes(pose(0, 0, 2, q, 0, 0, q), 0.2, K)!;
    const x = axes.find((a) => a.axis === "x")!;
    expect(x.x1).toBeCloseTo(320, 5);
    expect(x.y1).toBeCloseTo(250, 5);
  });

  test("the rotation of a vector is its quarter turn about z", () => {
    const q = Math.SQRT1_2;
    const v = rotate({ w: q, x: 0, y: 0, z: q }, { x: 1, y: 0, z: 0 });
    expect(v.x).toBeCloseTo(0, 6);
    expect(v.y).toBeCloseTo(1, 6);
    expect(rotate(identity.rotation, { x: 1, y: 2, z: 3 })).toEqual({ x: 1, y: 2, z: 3 });
  });

  test("intrinsics scale to another frame with the same field of view", () => {
    const half = scaleIntrinsics(K, 320, 240);
    expect(half.fx).toBeCloseTo(50);
    expect(half.cx).toBeCloseTo(160);
    expect(fieldOfView(K).fovY).toBeCloseTo(fieldOfView(half).fovY);
    expect(fieldOfView(K).aspect).toBeCloseTo(640 / 480);
  });
});

describe("pose outputs", () => {
  test("pose_solutions gives each tag's best pose, and skips a tag without one", () => {
    const view = parseTagPoses({
      status: "calibrated",
      tags: [
        { id: 7, best: { translation: { x: 0, y: 0, z: 1 }, rotation: { w: 1, x: 0, y: 0, z: 0 } }, best_error_px: 0.3, ambiguity: 0.1 },
        { id: 9 },
      ],
    });
    expect(view?.status).toBe("calibrated");
    expect(view?.tags.map((t) => t.id)).toEqual([7]);
    expect(view?.tags[0].errorPx).toBe(0.3);
    expect(parseTagPoses({ status: "calibrated" })).toBeNull();
  });

  test("multi_tag_pose gives the camera in the field only while valid, and the robot with a mount", () => {
    const pose3 = { translation: { x: 1, y: 2, z: 0.5 }, rotation: { w: 1, x: 0, y: 0, z: 0 } };
    const valid = parseFieldPose({ status: "calibrated", valid: true, has_rig: true, reference_from_camera: pose3, reference_from_rig: pose3, rms_px: 0.4, inlier_tags: 3, ambiguity: 0.02 });
    expect(valid?.cameraInField?.translation).toEqual({ x: 1, y: 2, z: 0.5 });
    expect(valid?.robotInField).not.toBeNull();
    expect(valid?.inliers).toBe(3);

    const unmounted = parseFieldPose({ valid: true, has_rig: false, reference_from_camera: pose3 });
    expect(unmounted?.robotInField).toBeNull();

    const invalid = parseFieldPose({ valid: false, reference_from_camera: pose3 });
    expect(invalid?.cameraInField).toBeNull();
    expect(parseFieldPose({ status: "x" })).toBeNull();
  });

  test("the mock's tag poses are the tags the camera faces, in its frame, and its field pose is the camera", () => {
    const view = mockPoseView(40);
    expect(view.field?.valid).toBe(true);
    expect(view.field?.cameraInField?.translation).toBeDefined();
    expect(view.tags?.tags.length).toBeGreaterThan(0);
    // Every tag the mock lists is in front of the camera, and projects into the mock's frame.
    for (const tag of view.tags!.tags) {
      expect(tag.pose.translation.z).toBeGreaterThan(0);
      expect(tagAxes(tag.pose, 0.1651, MOCK_INTRINSICS)).not.toBeNull();
    }
  });
});
