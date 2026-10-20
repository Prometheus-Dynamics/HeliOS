import { describe, expect, test } from "bun:test";
import type { PlaybackStatus } from "../src/lib/api/types";
import {
  canStepForward,
  clampMs,
  fractionOf,
  formatTime,
  frameAt,
  frameReadout,
  isEnded,
  isPlaying,
  loopChange,
  msAtFraction,
  msPerFrame,
  nextSpeed,
  playAction,
  planAction,
  planPlay,
  playChange,
  previousFrame,
  previousSpeed,
  seekFrameChange,
  seekMsChange,
  SEEK_STEP_MS,
  shortcutFor,
  speedLabel,
  speedOptions,
  stepChange,
  timeOfFrame,
  timeReadout,
  transportKnown,
} from "../src/lib/playback/transport";

/** A 10 s recording of 300 frames (30 fps), paused at 2.5 s (frame 75). */
function status(patch: Partial<PlaybackStatus> = {}): PlaybackStatus {
  return { state: "paused", position_ms: 2500, frame: 75, duration_ms: 10_000, frame_count: 300, speed: 1, loop: false, ...patch };
}

describe("time formatting", () => {
  test("m:ss.mmm under an hour", () => {
    expect(formatTime(0)).toBe("0:00.000");
    expect(formatTime(1250)).toBe("0:01.250");
    expect(formatTime(61_005)).toBe("1:01.005");
    expect(formatTime(59_999)).toBe("0:59.999");
  });

  test("h:mm:ss once the recording is an hour or longer", () => {
    expect(formatTime(3_600_000)).toBe("1:00:00");
    expect(formatTime(3_725_400)).toBe("1:02:05");
    // A position under an hour still reads as h:mm:ss when the recording is longer than an hour.
    expect(formatTime(90_000, 7_200_000)).toBe("0:01:30");
  });

  test("a negative or fractional time is clamped and rounded", () => {
    expect(formatTime(-40)).toBe("0:00.000");
    expect(formatTime(1.6)).toBe("0:00.002");
  });

  test("position / duration readout in the same units", () => {
    expect(timeReadout(status())).toBe("0:02.500 / 0:10.000");
    expect(timeReadout(status({ position_ms: 3_600_000, duration_ms: 7_200_000 }))).toBe("1:00:00 / 2:00:00");
  });

  test("frame readout is frame / frame_count, 0-based", () => {
    expect(frameReadout(status())).toBe("75 / 300");
  });
});

describe("frame and time conversion", () => {
  test("milliseconds per frame from the length and the count", () => {
    expect(msPerFrame(status())).toBeCloseTo(33.333, 2);
    expect(msPerFrame(status({ frame_count: 0 }))).toBe(0);
    expect(msPerFrame(status({ duration_ms: 0 }))).toBe(0);
  });

  test("the frame at a time, clamped to the last frame", () => {
    expect(frameAt(2500, status())).toBe(75);
    expect(frameAt(0, status())).toBe(0);
    expect(frameAt(10_000, status())).toBe(299);
    expect(frameAt(99_000, status())).toBe(299);
    expect(frameAt(500, status({ frame_count: 0 }))).toBe(0);
  });

  test("the time a frame starts at, clamped to the duration", () => {
    expect(timeOfFrame(75, status())).toBe(2500);
    expect(timeOfFrame(0, status())).toBe(0);
    expect(timeOfFrame(10_000, status())).toBe(10_000);
  });

  test("fraction and time at a fraction (the scrubber)", () => {
    expect(fractionOf(2500, 10_000)).toBe(0.25);
    expect(fractionOf(20_000, 10_000)).toBe(1);
    expect(fractionOf(5, 0)).toBe(0);
    expect(msAtFraction(0.5, 10_000)).toBe(5000);
    expect(msAtFraction(1.7, 10_000)).toBe(10_000);
    expect(msAtFraction(-0.2, 10_000)).toBe(0);
  });
});

describe("seek and step targets", () => {
  test("seeks clamp to [0, duration]", () => {
    expect(clampMs(-1, 10_000)).toBe(0);
    expect(clampMs(12_000, 10_000)).toBe(10_000);
    expect(clampMs(4000, 10_000)).toBe(4000);
    expect(clampMs(4000, 0)).toBe(0);
  });

  test("J and L move five seconds, clamped at both ends", () => {
    expect(SEEK_STEP_MS).toBe(5000);
    expect(planAction("seek-back", status())).toEqual([seekMsChange(0)]);
    expect(planAction("seek-back", status({ position_ms: 8000 }))).toEqual([seekMsChange(3000)]);
    expect(planAction("seek-forward", status())).toEqual([seekMsChange(7500)]);
    expect(planAction("seek-forward", status({ position_ms: 9000 }))).toEqual([seekMsChange(10_000)]);
  });

  test("step back is replay_seek_frame to frame - 1, and nothing at the first frame", () => {
    expect(previousFrame(status())).toBe(74);
    expect(previousFrame(status({ frame: 0 }))).toBeNull();
    expect(planAction("step-back", status())).toEqual([seekFrameChange(74)]);
    expect(planAction("step-back", status({ frame: 0 }))).toEqual([]);
  });

  test("step forward is replay_step, and nothing at the last frame", () => {
    expect(canStepForward(status())).toBe(true);
    expect(canStepForward(status({ frame: 298 }))).toBe(true);
    expect(canStepForward(status({ frame: 299 }))).toBe(false);
    expect(planAction("step-forward", status())).toEqual([stepChange()]);
    expect(planAction("step-forward", status({ frame: 299 }))).toEqual([]);
  });

  test("pause is a single replay_playing false", () => {
    expect(planAction("pause", status({ state: "playing" }))).toEqual([playChange(false)]);
  });
});

describe("speed", () => {
  test("the menu offers 0.25x, 0.5x, 1x, 2x, 4x and every frame (0), slowest first", () => {
    expect(speedOptions(1).map((o) => o.value)).toEqual([0.25, 0.5, 1, 2, 4, 0]);
    expect(speedOptions(1).map((o) => o.label)).toEqual(["0.25×", "0.5×", "1×", "2×", "4×", "Every frame"]);
  });

  test("a speed the menu does not list is added, in order", () => {
    expect(speedOptions(1.5).map((o) => o.value)).toEqual([0.25, 0.5, 1, 1.5, 2, 4, 0]);
    expect(speedOptions(0).map((o) => o.value)).toEqual([0.25, 0.5, 1, 2, 4, 0]);
  });

  test("next and previous speed step through the menu and stop at the ends", () => {
    expect(nextSpeed(1)).toBe(2);
    expect(previousSpeed(1)).toBe(0.5);
    expect(nextSpeed(0.25)).toBe(0.5);
    expect(previousSpeed(0.25)).toBe(0.25);
    expect(nextSpeed(4)).toBe(0);
    expect(previousSpeed(4)).toBe(2);
    expect(nextSpeed(0)).toBe(0);
    expect(previousSpeed(0)).toBe(4);
  });

  test("next and previous from a speed between two options", () => {
    expect(nextSpeed(1.5)).toBe(2);
    expect(previousSpeed(1.5)).toBe(1);
  });

  test("a missing speed reads as 1x", () => {
    expect(nextSpeed(null)).toBe(2);
    expect(speedLabel(null)).toBe("1×");
    expect(speedLabel(3)).toBe("3×");
    expect(speedLabel(0)).toBe("Every frame");
  });
});

describe("play state", () => {
  test("playing, paused, ended and unknown", () => {
    expect(isPlaying(status({ state: "playing" }))).toBe(true);
    expect(isPlaying(status())).toBe(false);
    expect(isEnded(status({ state: "ended" }))).toBe(true);
    expect(isEnded(null)).toBe(false);
  });

  test("the play button pauses while playing, starts over once ended, and plays otherwise", () => {
    expect(playAction(status({ state: "playing" }))).toBe("pause");
    expect(playAction(status({ state: "ended" }))).toBe("replay");
    expect(playAction(status())).toBe("play");
    expect(playAction(status({ state: "unknown" }))).toBe("play");
    expect(playAction(null)).toBe("play");
  });

  test("playing from the end seeks to the start before playing", () => {
    expect(planPlay(status({ state: "ended", position_ms: 10_000 }))).toEqual([seekMsChange(0), playChange(true)]);
    expect(planPlay(status({ state: "playing" }))).toEqual([playChange(false)]);
    expect(planPlay(status())).toEqual([playChange(true)]);
  });

  test("a status the camera service did not answer for is not driven", () => {
    expect(transportKnown(status())).toBe(true);
    expect(transportKnown(status({ state: "unknown" }))).toBe(false);
    expect(transportKnown(null)).toBe(false);
  });

  test("loop toggles through replay_loop", () => {
    expect(loopChange(true)).toEqual({ replay_loop: true });
    expect(loopChange(false)).toEqual({ replay_loop: false });
  });
});

describe("keyboard", () => {
  test("space toggles, K pauses, arrows and comma/period step, J and L seek", () => {
    expect(shortcutFor(" ")).toBe("toggle");
    expect(shortcutFor("k")).toBe("pause");
    expect(shortcutFor("K")).toBe("pause");
    expect(shortcutFor("ArrowLeft")).toBe("step-back");
    expect(shortcutFor(",")).toBe("step-back");
    expect(shortcutFor("ArrowRight")).toBe("step-forward");
    expect(shortcutFor(".")).toBe("step-forward");
    expect(shortcutFor("j")).toBe("seek-back");
    expect(shortcutFor("L")).toBe("seek-forward");
  });

  test("any other key is not a playback shortcut", () => {
    expect(shortcutFor("a")).toBeNull();
    expect(shortcutFor("ArrowUp")).toBeNull();
    expect(shortcutFor("Enter")).toBeNull();
  });

  test("space on a paused player plans a play; on a playing one, a pause", () => {
    expect(planAction("toggle", status())).toEqual([playChange(true)]);
    expect(planAction("toggle", status({ state: "playing" }))).toEqual([playChange(false)]);
  });
});
