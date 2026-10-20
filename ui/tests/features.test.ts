import { describe, expect, test } from "bun:test";
import { mockCameraFeatures, mockFeatureTracks } from "../src/lib/api/mock";
import { featureSummaryText, toFeatureTracks, toFeedSpace, TrailBook, trackFill, TRAIL_LENGTH, MAX_TRAIL_STEP, type CameraFeatures } from "../src/lib/vision/features";

const track = (id: number, x: number, y: number, age = 1, status: "new" | "tracked" = "tracked") => ({ id, x, y, age, status });

describe("feature tracks output", () => {
  test("parses the eidos:feature_tracks value and keeps its summary", () => {
    const value = {
      tracks: [track(1, 10, 20, 0, "new"), track(2, 30, 40, 4)],
      summary: { count: 2, tracked: 1, new: 1, lost: 3 },
    };
    const parsed = toFeatureTracks(value, { w: 1280, h: 800 });
    expect(parsed?.tracks).toEqual(value.tracks);
    expect(parsed?.summary).toEqual({ count: 2, tracked: 1, new: 1, lost: 3 });
    expect(parsed?.frame).toEqual({ w: 1280, h: 800 });
  });

  test("drops malformed tracks and rejects values that are not tracks", () => {
    const parsed = toFeatureTracks({ tracks: [track(1, 1, 2), { id: 2, x: "3", y: 4, age: 0, status: "new" }, { id: 3, x: 1, y: 1, age: 0, status: "lost" }] });
    expect(parsed?.tracks.map((t) => t.id)).toEqual([1]);
    expect(parsed?.summary).toEqual({ count: 1, tracked: 1, new: 0, lost: 0 });
    expect(toFeatureTracks(null)).toBeNull();
    expect(toFeatureTracks({ detections: [] })).toBeNull();
  });
});

describe("positions in the feed", () => {
  const features = (frame?: { w: number; h: number }): CameraFeatures => {
    const tracks = toFeatureTracks({ tracks: [track(1, 640, 400)], summary: { count: 1, tracked: 1, new: 0, lost: 0 } }, frame)!;
    return { tracks, trails: new Map([[1, [{ x: 320, y: 200 }, { x: 640, y: 400 }]]]), signature: "" };
  };

  test("scale from the frame size the way detection corners do", () => {
    // A 1280x800 frame shown in a 640x400 feed: half the coordinates.
    const view = toFeedSpace(features({ w: 1280, h: 800 }), 640, 400);
    expect(view.tracks[0]).toMatchObject({ x: 320, y: 200 });
    expect(view.trails[0]).toEqual([1, [{ x: 160, y: 100 }, { x: 320, y: 200 }]]);
  });

  test("are feed pixels without a known frame", () => {
    const view = toFeedSpace(features(), 640, 400);
    expect(view.tracks[0]).toMatchObject({ x: 640, y: 400 });
  });
});

describe("trails", () => {
  test("keep the last positions of each id, newest last", () => {
    const book = new TrailBook();
    let trails = new Map<number, { x: number; y: number }[]>();
    for (let t = 0; t < TRAIL_LENGTH + 5; t++) trails = book.advance([track(7, t, 0)]);
    const trail = trails.get(7)!;
    expect(trail.length).toBe(TRAIL_LENGTH);
    expect(trail[trail.length - 1]).toEqual({ x: TRAIL_LENGTH + 4, y: 0 });
    expect(trail[0]).toEqual({ x: 5, y: 0 });
  });

  test("drop an id the tick no longer lists, and a reappearing id starts over", () => {
    const book = new TrailBook();
    book.advance([track(1, 0, 0), track(2, 0, 0)]);
    book.advance([track(1, 1, 0), track(2, 1, 0)]);
    const without = book.advance([track(1, 2, 0)]);
    expect([...without.keys()]).toEqual([1]);
    const back = book.advance([track(1, 3, 0), track(2, 9, 9)]);
    expect(back.get(2)).toEqual([{ x: 9, y: 9 }]);
  });

  test("start again after a jump, so no line crosses the frame", () => {
    const book = new TrailBook();
    book.advance([track(3, 100, 100)]);
    book.advance([track(3, 110, 100)]);
    const jumped = book.advance([track(3, 110 + MAX_TRAIL_STEP + 1, 100)]);
    expect(jumped.get(3)).toEqual([{ x: 110 + MAX_TRAIL_STEP + 1, y: 100 }]);
  });

  test("hold their points as state: an advance never mutates an earlier map", () => {
    const book = new TrailBook();
    const first = book.advance([track(1, 0, 0)]);
    book.advance([track(1, 5, 5)]);
    expect(first.get(1)).toEqual([{ x: 0, y: 0 }]);
  });
});

describe("drawing", () => {
  test("a new track is brighter than a settled one", () => {
    expect(trackFill({ status: "new", age: 0 })).toBe("#fef08a");
    expect(trackFill({ status: "tracked", age: 1 })).not.toBe(trackFill({ status: "tracked", age: 40 }));
    expect(trackFill({ status: "tracked", age: 40 })).toBe(trackFill({ status: "tracked", age: 30 }));
  });

  test("the summary chip reads the counts", () => {
    expect(featureSummaryText({ count: 12, tracked: 9, new: 3, lost: 1 })).toBe("12 tracks · 9 tracked · 3 new · 1 lost");
  });
});

describe("mock tracks", () => {
  test("counts agree with the list and ids are unique within a tick", () => {
    for (const index of [0, 1, 24, 25, 200, 479]) {
      const value = mockFeatureTracks(index);
      const ids = value.tracks.map((t) => t.id);
      expect(new Set(ids).size).toBe(ids.length);
      expect(value.summary.count).toBe(value.tracks.length);
      expect(value.summary.tracked + value.summary.new).toBe(value.summary.count);
      expect(value.tracks.every((t) => t.status === (t.age === 0 ? "new" : "tracked"))).toBe(true);
    }
  });

  test("tracks end and restart with new ids, so trails are dropped and reborn", () => {
    const before = mockFeatureTracks(24);
    const after = mockFeatureTracks(25);
    const gone = before.tracks.map((t) => t.id).filter((id) => !after.tracks.some((t) => t.id === id));
    expect(gone.length).toBe(after.summary.new);
    expect(after.summary.lost).toBe(after.summary.new);
    const features = mockCameraFeatures(100);
    expect(features.trails.size).toBe(features.tracks.tracks.length);
    expect([...features.trails.values()].every((points) => points.length <= TRAIL_LENGTH)).toBe(true);
  });
});
