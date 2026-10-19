import { describe, expect, test } from "bun:test";
import { LED_STATES } from "../src/lib/api/leds";
import { LOOKS } from "../src/lib/leds/looks";
import { STATE_HELP, STATE_LOOK, frameFor, statePreview, sweepProgress } from "../src/lib/leds/states";
import { renderLook } from "../src/lib/leds/render";
import { validateLeds } from "../src/lib/leds/settings";
import type { LedSettings } from "../src/lib/api/leds";

const SETTINGS: LedSettings = {
  enabled: true,
  brightness: 0.5,
  ambient: { enabled: false, min: 0.1, max: 0.9 },
  forward_led: 0,
  direction: "cw",
  fov_deg: 90,
  tally: true,
  boost: false,
};

const sum = (frame: number[][]) => frame.reduce((a, c) => a + c[0] + c[1] + c[2], 0);

describe("state preview", () => {
  test("every state has a help line and a look entry", () => {
    for (const state of LED_STATES) {
      expect(STATE_HELP[state].length).toBeGreaterThan(10);
      expect(Object.prototype.hasOwnProperty.call(STATE_LOOK, state)).toBe(true);
    }
  });

  test("every look a state names is in the table", () => {
    for (const state of LED_STATES) {
      const name = STATE_LOOK[state];
      if (name !== null) expect(Object.prototype.hasOwnProperty.call(LOOKS, name)).toBe(true);
    }
  });

  test("every state gives sixteen colours in 0 to 1, at any time", () => {
    for (const state of LED_STATES) {
      for (const t of [0, 250, 1700, 9999]) {
        const frame = statePreview(state, t);
        expect(frame.length).toBe(16);
        for (const c of frame) for (const v of c) expect(v >= 0 && v <= 1 && Number.isFinite(v)).toBe(true);
      }
    }
  });

  test("idle is dark", () => {
    expect(sum(statePreview("idle", 500))).toBe(0);
  });

  test("a state with a look previews that look, with the calibration sweep for its arc", () => {
    const t = 1234;
    expect(statePreview("searching", t)).toEqual(renderLook("helios.searching", t, sweepProgress(t))!);
  });

  test("the locate rainbow uses a different hue on each LED", () => {
    const frame = statePreview("locate", 0);
    expect(new Set(frame.map((c) => c.map((v) => v.toFixed(3)).join(","))).size).toBeGreaterThan(8);
  });

  test("the calibration sweep stays within 0 and 1", () => {
    for (let t = 0; t <= 24000; t += 250) {
      const p = sweepProgress(t);
      expect(p >= 0 && p <= 1).toBe(true);
    }
  });
});

describe("frameFor (what the ring draws)", () => {
  const hex = Array.from({ length: 16 }, (_, i) => (i === 0 ? "ff0000" : "000000"));

  test("device frames are drawn exactly", () => {
    const frame = frameFor({ state: "locate", look: null, brightness: 0.1, leds: hex }, 0);
    expect(frame[0]).toEqual([1, 0, 0]);
    expect(frame[1]).toEqual([0, 0, 0]);
  });

  test("a frame of the wrong length falls back to the state's preview", () => {
    const frame = frameFor({ state: "idle", look: null, brightness: 1, leds: ["ff0000"] }, 0);
    expect(sum(frame)).toBe(0);
  });

  test("a named look is scaled by the device's ring brightness", () => {
    const full = frameFor({ state: "recording", look: "helios.recording", brightness: 1, leds: null }, 0);
    const half = frameFor({ state: "recording", look: "helios.recording", brightness: 0.5, leds: null }, 0);
    expect(half[0][0]).toBeCloseTo(full[0][0] * 0.5, 9);
  });

  test("a look the table does not know falls back to the state", () => {
    const frame = frameFor({ state: "searching", look: "helios.unknown", brightness: 1, leds: null }, 1000);
    expect(frame).toEqual(statePreview("searching", 1000));
  });
});

describe("settings validation", () => {
  test("the default settings are valid", () => {
    expect(validateLeds(SETTINGS)).toBeNull();
  });

  test("the ambient minimum cannot be above the maximum", () => {
    expect(validateLeds({ ...SETTINGS, ambient: { enabled: true, min: 0.8, max: 0.2 } })).toContain("minimum");
  });

  test("brightness, the forward LED and the field of view have their ranges", () => {
    expect(validateLeds({ ...SETTINGS, brightness: 1.2 })).not.toBeNull();
    expect(validateLeds({ ...SETTINGS, forward_led: 16 })).not.toBeNull();
    expect(validateLeds({ ...SETTINGS, forward_led: 2.5 })).not.toBeNull();
    expect(validateLeds({ ...SETTINGS, fov_deg: 19 })).not.toBeNull();
    expect(validateLeds({ ...SETTINGS, fov_deg: 180 })).toBeNull();
  });

  test("the direction is cw or ccw", () => {
    expect(validateLeds({ ...SETTINGS, direction: "up" as "cw" })).not.toBeNull();
    expect(validateLeds({ ...SETTINGS, direction: "ccw" })).toBeNull();
  });
});
