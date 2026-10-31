import { describe, expect, test } from "bun:test";
import {
  aprilTagCapWrites,
  aprilTagPlan,
  clampedNote,
  clampExposure,
  disabledReason,
  enumOptions,
  EXPOSURE_FALLBACK,
  flickerLabel,
  formatMs,
  friendlyLabel,
  frameRateChoice,
  frameRatePresetsIn,
  groupOf,
  isChanged,
  limitOn,
  maxExposureUs,
  presetUndoChanges,
  rangeOr,
  resetTarget,
  rectText,
  unitOf,
  valueText,
} from "../src/lib/camera/controls";
import { fromLogFraction, toLogFraction } from "../src/lib/kit/scale";

// The OV9782 control list the device reports (Styx native_backend.rs at the pinned revision).
const OV9782 = [
  "exposure_time_us",
  "gain",
  "frame_duration_us",
  "frame_rate",
  "ae_enable",
  "exposure_value",
  "awb_enable",
  "colour_temperature",
  "red_gain",
  "blue_gain",
  "ae_flicker_mode",
  "ae_deflicker_mode",
  "ae_flicker_detected",
  "ae_state",
  "output_crop",
  ...Array.from({ length: 15 }, (_, i) => `region_crop_${i + 1}`),
];

const exposure = { min: 9, max: 476_066 };
const gain = { min: 1, max: 15.94 };

describe("labels and units", () => {
  test("every device control gets a friendly label, never a raw driver name", () => {
    for (const name of OV9782) expect(friendlyLabel(name)).not.toContain("_");
    expect(friendlyLabel("exposure_time_us")).toBe("Exposure");
    expect(friendlyLabel("ae_enable")).toBe("Auto exposure");
    expect(friendlyLabel("region_crop_3")).toBe("Region crop 3");
  });

  test("an unknown control is humanised from its name", () => {
    expect(friendlyLabel("sharpen_strength")).toBe("Sharpen strength");
  });

  test("units come from the name", () => {
    expect(unitOf("exposure_time_us")).toBe("µs");
    expect(unitOf("frame_rate")).toBe("fps");
    expect(unitOf("colour_temperature")).toBe("K");
    expect(unitOf("ae_enable")).toBeUndefined();
    expect(unitOf("some_delay_ms")).toBe("ms");
  });
});

describe("groups", () => {
  test("the device list lands in the sections the pane shows", () => {
    const group = (name: string) => groupOf({ name });
    expect(group("frame_rate")).toBe("capture");
    expect(group("exposure_time_us")).toBe("exposure");
    expect(group("gain")).toBe("exposure");
    expect(group("ae_enable")).toBe("exposure");
    expect(group("exposure_value")).toBe("exposure");
    expect(group("ae_state")).toBe("exposure");
    expect(group("awb_enable")).toBe("white");
    expect(group("colour_temperature")).toBe("white");
    expect(group("red_gain")).toBe("white");
    expect(group("blue_gain")).toBe("white");
    expect(group("ae_flicker_mode")).toBe("flicker");
    expect(group("ae_deflicker_mode")).toBe("flicker");
    expect(group("ae_flicker_detected")).toBe("flicker");
    expect(group("output_crop")).toBe("crop");
    expect(group("region_crop_15")).toBe("crop");
    expect(group("frame_duration_us")).toBe("advanced");
  });

  test("a device's max-exposure limit is an exposure control, crops never are", () => {
    expect(groupOf({ name: "ae_max_exposure_us" })).toBe("exposure");
    expect(groupOf({ name: "region_crop_2" })).toBe("crop");
  });

  test("every control of the device list is in exactly one group", () => {
    const groups = new Set(["capture", "exposure", "white", "flicker", "crop", "advanced"]);
    for (const name of OV9782) expect(groups.has(groupOf({ name }))).toBe(true);
  });
});

describe("enum choices", () => {
  test("flicker avoidance is labelled from the Styx meaning (0 off, 1 50 Hz, 2 60 Hz, 3 auto)", () => {
    expect(enumOptions({ name: "ae_flicker_mode", menu: null })?.map((o) => o.label)).toEqual(["Off", "50 Hz", "60 Hz", "Auto"]);
  });

  test("deflicker is labelled (0 off, 1 on, 2 with flicker avoidance)", () => {
    expect(enumOptions({ name: "ae_deflicker_mode", menu: null })?.map((o) => o.label)).toEqual(["Off", "On", "On, avoids flicker"]);
  });

  test("a device menu wins over the label map", () => {
    expect(enumOptions({ name: "ae_flicker_mode", menu: [{ value: 0, label: "a", kind: "name" }, { value: 1, label: "b", kind: "name" }] })).toEqual([
      { value: 0, label: "a" },
      { value: 1, label: "b" },
    ]);
  });

  test("a control with no labels has no choices", () => {
    expect(enumOptions({ name: "gain", menu: null })).toBeNull();
  });

  test("detected flicker reads as mains frequency", () => {
    expect(flickerLabel(10_000)).toBe("50 Hz mains");
    expect(flickerLabel(8_333)).toBe("60 Hz mains");
    expect(flickerLabel(0)).toBe("None yet");
  });

  test("auto exposure state reads as words", () => {
    expect(valueText({ name: "ae_state", value: 2 })).toBe("Converged");
    expect(valueText({ name: "ae_state", value: 1 })).toBe("Searching");
  });
});

describe("exposure maths", () => {
  test("one frame caps the exposure at the frame rate", () => {
    expect(maxExposureUs(exposure, 60)).toBeCloseTo(16_666.67, 1);
    expect(maxExposureUs(exposure, 30)).toBeCloseTo(33_333.33, 1);
    expect(maxExposureUs(exposure, 120)).toBeCloseTo(8_333.33, 1);
  });

  test("the device's own range caps it when the frame is longer", () => {
    expect(maxExposureUs(exposure, 2.1)).toBe(476_066);
  });

  test("the device's minimum floors it at the fastest frame rate", () => {
    expect(maxExposureUs(exposure, 311.7)).toBeGreaterThanOrEqual(9);
  });

  test("clampExposure keeps a requested exposure within one frame", () => {
    expect(clampExposure(50_000, exposure, 60)).toBe(16_667);
    expect(clampExposure(1, exposure, 60)).toBe(9);
    expect(clampExposure(2_200, exposure, 60)).toBe(2_200);
  });

  test("the log scale maps the ends and round-trips", () => {
    expect(toLogFraction(9, 9, 1000)).toBe(0);
    expect(toLogFraction(1000, 9, 1000)).toBe(1);
    expect(fromLogFraction(0, 9, 1000)).toBeCloseTo(9, 6);
    expect(fromLogFraction(1, 9, 1000)).toBeCloseTo(1000, 6);
    const v = fromLogFraction(0.37, 9, 16_000);
    expect(toLogFraction(v, 9, 16_000)).toBeCloseTo(0.37, 9);
  });

  test("the log scale is midway at the geometric mean, not the arithmetic one", () => {
    expect(fromLogFraction(0.5, 1, 100)).toBeCloseTo(10, 6);
  });

  test("ms formatting keeps a few significant figures", () => {
    expect(formatMs(1500)).toBe("1.5");
    expect(formatMs(16_666.67)).toBe("16.67");
    expect(formatMs(9)).toBe("0.009");
  });
});

describe("frame rate", () => {
  test("presets are offered only inside the camera's range", () => {
    expect(frameRatePresetsIn({ min: 2.1, max: 311.7 })).toEqual([30, 60, 90, 120]);
    expect(frameRatePresetsIn({ min: 1, max: 100 })).toEqual([30, 60, 90]);
  });

  test("a rate that is a preset selects it, anything else is custom", () => {
    expect(frameRateChoice(60)).toBe("60");
    expect(frameRateChoice(60.0000001)).toBe("60");
    expect(frameRateChoice(75)).toBe("custom");
    expect(frameRateChoice(null)).toBe("custom");
  });
});

describe("AprilTag preset", () => {
  const ranges = { exposure, gain, fps: 60 };

  test("manual 1.5 ms, with gain raised so exposure times gain is kept", () => {
    // 8 ms at 4x is the same brightness as 1.5 ms at 4 * 8 / 1.5 = 21.3x, which the camera caps.
    const plan = aprilTagPlan({ exposureUs: 2_200, gain: 1.5 }, ranges);
    expect(plan.exposureUs).toBe(1_500);
    expect(plan.gain).toBeCloseTo((2_200 * 1.5) / 1_500, 2);
    expect(plan.capped).toBe(false);
  });

  test("gain at the camera's limit is reported, so the UI can warn the image is darker", () => {
    const plan = aprilTagPlan({ exposureUs: 16_000, gain: 8 }, ranges);
    expect(plan.gain).toBe(15.94);
    expect(plan.capped).toBe(true);
  });

  test("at a high frame rate the exposure stays within one frame", () => {
    const plan = aprilTagPlan({ exposureUs: 2_000, gain: 2 }, { exposure, gain, fps: 240 });
    expect(plan.exposureUs).toBeLessThanOrEqual(1_000_000 / 240);
  });

  test("with no brightness to keep, the gain is the lowest allowed", () => {
    expect(aprilTagPlan({ exposureUs: null, gain: null }, ranges).gain).toBe(1);
  });
});

describe("reset, changed and disabled", () => {
  const base = { name: "gain", standard: "gain", min: 1, max: 15.94, default: 1, value: 4, writable: true, readOnly: false } as const;

  test("a control is changed when it is off its default", () => {
    expect(isChanged(base)).toBe(true);
    expect(isChanged({ ...base, value: 1 })).toBe(false);
    expect(isChanged({ ...base, value: 1 + 1e-9 })).toBe(false);
  });

  test("a default outside the range offers no reset", () => {
    // The device reports colour temperature's default as 0, below its 1000 K minimum.
    const colour = { ...base, name: "colour_temperature", min: 1000, max: 20_000, default: 0, value: 5600 };
    expect(resetTarget(colour)).toBeNull();
    expect(isChanged(colour)).toBe(false);
  });

  test("a boolean default is a valid reset target", () => {
    expect(resetTarget({ ...base, default: true, min: undefined, max: undefined })).toBe(true);
  });

  test("read-only and unwritable controls offer no reset, and say why", () => {
    expect(resetTarget({ ...base, readOnly: true, writable: false })).toBeNull();
    expect(disabledReason({ writable: false, readOnly: true })).toMatch(/Read only/);
    expect(disabledReason({ writable: false, readOnly: false })).toMatch(/does not let the UI change/);
    expect(disabledReason({ writable: true, readOnly: false })).toBeNull();
  });
});

describe("read-only values", () => {
  test("rectangles show their size and place, and zero means off", () => {
    expect(rectText(null)).toBe("not readable");
    expect(rectText({ x: 0, y: 0, width: 0, height: 0 })).toBe("Off");
    expect(rectText({ x: 16, y: 8, width: 640, height: 400 })).toBe("640×400 at 16,8");
    expect(rectText([{ x: 0, y: 0, width: 0, height: 0 }])).toBe("Off");
  });

  test("a value is shown with its unit", () => {
    expect(valueText({ name: "exposure_time_us", value: 2200 })).toBe("2200 µs");
    expect(valueText({ name: "ae_enable", value: false })).toBe("Off");
    expect(valueText({ name: "gain", value: null })).toBe("not readable");
  });
});

describe("device menus", () => {
  test("a value shows the device's menu label before any fallback map", () => {
    const menu = [{ value: 0, label: "inactive", kind: "name" as const }, { value: 1, label: "searching", kind: "name" as const }, { value: 2, label: "converged", kind: "name" as const }];
    expect(valueText({ name: "ae_state", value: 2, menu })).toBe("converged");
    expect(valueText({ name: "ae_state", value: 0, menu })).toBe("inactive");
    // Without a menu the fallback words apply (searching and converged only).
    expect(valueText({ name: "ae_state", value: 0 })).toBe("0");
  });
});

describe("effective ranges", () => {
  test("the range a control takes now is the effective one when the camera reports it", () => {
    expect(rangeOr({ min: 9, max: 476_066, effectiveMin: 9, effectiveMax: 8_100 }, EXPOSURE_FALLBACK)).toEqual({ min: 9, max: 8_100 });
    expect(rangeOr({ min: 1, max: 15.94 }, { min: 1, max: 16 })).toEqual({ min: 1, max: 15.94 });
    expect(rangeOr(undefined, EXPOSURE_FALLBACK)).toEqual(EXPOSURE_FALLBACK);
  });

  test("the exposure max shows the effective max at the frame rate: 8.1 ms at 120 fps", () => {
    const range = rangeOr({ min: 9, max: 476_066, effectiveMax: 8_100 }, EXPOSURE_FALLBACK);
    expect(formatMs(maxExposureUs(range, 120))).toBe("8.1");
  });

  test("the frame-duration maths applies when no effective range is reported", () => {
    const range = rangeOr({ min: 9, max: 476_066 }, EXPOSURE_FALLBACK);
    expect(formatMs(maxExposureUs(range, 120))).toBe("8.333");
  });

  test("an exposure beyond the effective max is clamped to it", () => {
    const range = rangeOr({ min: 9, max: 476_066, effectiveMax: 8_100 }, EXPOSURE_FALLBACK);
    expect(clampExposure(50_000, range, 120)).toBe(8_100);
  });

  test("a clamped write says what was asked and what was set", () => {
    expect(clampedNote({ name: "exposure_time_us" }, 50_000, 8_100)).toBe("Exposure: 50000 µs is outside what the camera takes now, so it is 8100 µs");
  });
});

describe("AE limits", () => {
  test("0 is no limit; any other value limits", () => {
    expect(limitOn(0)).toBe(false);
    expect(limitOn(null)).toBe(false);
    expect(limitOn(false)).toBe(false);
    expect(limitOn(1500)).toBe(true);
    expect(limitOn(6)).toBe(true);
  });

  test("the limits, modes and metering region are exposure controls, with friendly names", () => {
    for (const name of ["ae_exposure_max_us", "ae_gain_max", "ae_exposure_mode", "ae_metering_mode", "ae_metering_region"]) {
      expect(groupOf({ name })).toBe("exposure");
    }
    expect(friendlyLabel("ae_exposure_max_us")).toBe("Max exposure (motion blur limit)");
    expect(friendlyLabel("ae_gain_max")).toBe("Max gain");
  });
});

describe("AprilTag preset with an exposure ceiling", () => {
  test("AE on, held to 1.5 ms, gain up to the device's gain cap", () => {
    const writes = aprilTagCapWrites({ ae: "ae", exposureMax: "ae_exposure_max_us", gainMax: "ae_gain_max" }, 15.94);
    expect(writes).toEqual({ ae: true, ae_exposure_max_us: 1500, ae_gain_max: 15.94 });
  });

  test("with no gain cap to set, only AE and the ceiling are written; 16 when the cap has no maximum", () => {
    expect(aprilTagCapWrites({ ae: "ae", exposureMax: "ae_exposure_max_us" }, 15.94)).toEqual({ ae: true, ae_exposure_max_us: 1500 });
    expect(aprilTagCapWrites({ ae: "ae", exposureMax: "ae_exposure_max_us", gainMax: "ae_gain_max" }, undefined)).toEqual({ ae: true, ae_exposure_max_us: 1500, ae_gain_max: 16 });
  });

  test("undo puts back what was kept, and forgets what was not stored before the preset", () => {
    // AE was manual and not stored; the ceiling was not stored; the gain cap was stored at 4.
    const undo = presetUndoChanges({
      ae: { kept: false, previous: false },
      ae_exposure_max_us: { kept: false, previous: 0 },
      ae_gain_max: { kept: true, previous: 4 },
    });
    expect(undo).toEqual({ ae: null, ae_exposure_max_us: null, ae_gain_max: 4 });
  });

  test("undo leaves a kept key alone when its previous value is unknown", () => {
    expect(presetUndoChanges({ exposure_us: { kept: true, previous: null }, gain: { kept: false, previous: 2 } })).toEqual({ gain: null });
  });

  test("the manual plan's undo restores kept values and forgets the rest", () => {
    const undo = presetUndoChanges({
      ae: { kept: true, previous: true },
      exposure_us: { kept: false, previous: 2200 },
      gain: { kept: true, previous: 1.5 },
    });
    expect(undo).toEqual({ ae: true, exposure_us: null, gain: 1.5 });
  });
});
