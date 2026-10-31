import { describe, expect, test } from "bun:test";
import type { CameraControlInfo } from "../src/lib/api/model";
import { autoOn, changedByUser, controlFor, controlNamed, fieldControlsOf, groupOf } from "../src/lib/camera/controls";

// A control as the camera service lists it. Only what the tests read is set.
const control = (fields: Partial<CameraControlInfo> & { name: string }): CameraControlInfo =>
  ({ id: 0, kind: "int", default: null, value: null, standard: null, writable: true, readOnly: false, ...fields }) as CameraControlInfo;

// A UVC camera (the SystemCore's Logitech C270): its own names, and the standard roles on the
// controls that have one. Auto exposure is a menu (its entries are the values' labels).
const uvc: CameraControlInfo[] = [
  control({ name: "Brightness", kind: "int", value: 128 }),
  control({ name: "White Balance, Automatic", kind: "bool", standard: "awb", value: true, default: true }),
  control({ name: "Gain", kind: "int", standard: "gain", value: 192, default: 64, min: 0, max: 255 }),
  control({ name: "White Balance Temperature", kind: "int", standard: "colour_temperature", value: 1690, default: 4000, min: 0, max: 10000 }),
  control({ name: "Auto Exposure", kind: "menu", standard: "ae", value: 3, default: 3, menu: [{ value: 1, label: "Manual Mode", kind: "name" }, { value: 3, label: "Aperture Priority Mode", kind: "name" }] }),
  control({ name: "Exposure Time, Absolute", kind: "int", standard: "exposure_us", value: 419, default: 166, min: 1, max: 10000 }),
  control({ name: "Exposure, Dynamic Framerate", kind: "bool", value: true, default: false }),
  control({ name: "Backlight Compensation", kind: "int", value: 0, default: 0, min: 0, max: 1 }),
  control({ name: "frame_rate", kind: "float", standard: "fps", value: 30, default: 30, min: 1, max: 30 }),
];

describe("roles", () => {
  test("a control is found by its standard role, whatever the device calls it", () => {
    expect(controlFor(uvc, "exposure_us", "exposure_time_us")?.name).toBe("Exposure Time, Absolute");
    expect(controlFor(uvc, "ae", "ae_enable")?.name).toBe("Auto Exposure");
    expect(controlFor(uvc, "awb", "awb_enable")?.name).toBe("White Balance, Automatic");
    expect(controlFor(uvc, "fps", "frame_rate")?.name).toBe("frame_rate");
  });

  test("a control with no role is found by its name only when the device names none", () => {
    expect(controlFor([control({ name: "ae_enable" })], "ae", "ae_enable")?.name).toBe("ae_enable");
    expect(controlFor([control({ name: "gain", standard: "fps" })], "gain", "gain")).toBeUndefined();
    expect(controlNamed([control({ name: "ae_state", standard: "ae" })], "ae_state")).toBeUndefined();
    expect(controlFor(uvc, "red_gain", "red_gain")).toBeUndefined();
  });

  test("auto is read from a boolean, or from a menu by the entry named Manual", () => {
    expect(autoOn(uvc[1])).toBe(true);
    expect(autoOn(control({ name: "ae_enable", kind: "bool", value: false }))).toBe(false);
    // V4L2 values: manual is 1 and aperture priority is 3.
    expect(autoOn(uvc[4])).toBe(true);
    expect(autoOn(control({ name: "Auto Exposure", kind: "menu", value: 1, menu: [{ value: 1, label: "Manual Mode", kind: "name" }, { value: 3, label: "Aperture Priority Mode", kind: "name" }] }))).toBe(false);
  });

  test("a number the device does not label as manual or auto is unknown", () => {
    expect(autoOn(control({ name: "Auto Exposure", kind: "menu", value: 1, menu: [{ value: 0, label: "Off", kind: "name" }, { value: 1, label: "On", kind: "name" }] }))).toBeNull();
    expect(autoOn(control({ name: "Auto Exposure", kind: "int", value: 1 }))).toBeNull();
    expect(autoOn(control({ name: "Auto Exposure", kind: "bool", value: null }))).toBeNull();
    expect(autoOn(undefined)).toBeNull();
  });

  test("standard roles group a device's own names where the pane shows them", () => {
    expect(groupOf({ name: "Exposure Time, Absolute", standard: "exposure_us" })).toBe("exposure");
    expect(groupOf({ name: "Gain", standard: "gain" })).toBe("exposure");
    expect(groupOf({ name: "Auto Exposure", standard: "ae" })).toBe("exposure");
    expect(groupOf({ name: "White Balance Temperature", standard: "colour_temperature" })).toBe("white");
    expect(groupOf({ name: "frame_rate", standard: "fps" })).toBe("capture");
    expect(groupOf({ name: "Backlight Compensation", standard: null })).toBe("advanced");
    expect(groupOf({ name: "output_crop", standard: null })).toBe("crop");
  });

  test("a camera's own live values in auto are not changes the user made", () => {
    // Auto white balance: the colour temperature is the camera's estimate (far from its default).
    const colour = uvc[3];
    expect(changedByUser(uvc, colour)).toBe(false);
    const manualWhite = uvc.map((c) => (c === uvc[1] ? control({ ...c, value: false }) : c));
    expect(changedByUser(manualWhite, manualWhite[3])).toBe(true);
    // Auto exposure: the exposure and gain are live; in manual they are the user's.
    const exposure = uvc[5];
    expect(changedByUser(uvc, exposure)).toBe(false);
    const manualExposure = uvc.map((c) => (c === uvc[4] ? control({ ...c, value: 1 }) : c));
    expect(changedByUser(manualExposure, manualExposure[5])).toBe(true);
    expect(changedByUser(manualExposure, manualExposure[2])).toBe(true);
    // A control at its default is not changed, in any mode.
    expect(changedByUser(manualExposure, control({ name: "Brightness", kind: "int", value: 128, default: 128 }))).toBe(false);
  });

  test("the generic lists leave out what the fields show, and keep the rest", () => {
    const shown = fieldControlsOf(uvc);
    expect(shown.size).toBe(6);
    for (const name of ["Auto Exposure", "Exposure Time, Absolute", "Gain", "White Balance, Automatic", "White Balance Temperature", "frame_rate"]) {
      expect([...shown].some((c) => c.name === name)).toBe(true);
    }
    expect(shown.has(uvc[6])).toBe(false);
    expect(shown.has(uvc[7])).toBe(false);
    expect(shown.has(uvc[8])).toBe(true);
  });
});
