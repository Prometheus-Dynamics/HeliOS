// The camera controls' names, labels, groups, units and maths. Pure (no stores, no DOM): the
// camera pane, the inspector's quick controls and their tests all read it, so a control means
// the same thing everywhere.
//
// Names come from Styx's native backend (`native_backend.rs`, the revision Backend/Cargo.lock
// pins): `controls::*` ids, `AE_FLICKER_DETECTED` (the light's flicker period in µs). A control's
// labelled values come from the device's `menu` when it lists one (the value is the index); the
// label maps below are only the fallback for a device that lists no menu.

import type { CameraControlInfo, CropRect } from "#lib/api/model.js";

export type Control = Pick<CameraControlInfo, "name" | "standard" | "min" | "max" | "default" | "value" | "writable" | "readOnly"> &
  Partial<Pick<CameraControlInfo, "effectiveMin" | "effectiveMax" | "menu" | "persisted">>;
export type Range = { min: number; max: number };
export type ControlGroup = "capture" | "exposure" | "white" | "flicker" | "crop" | "advanced";

/** Presets the frame-rate control offers; a preset outside the camera's range is not offered. */
export const FRAME_RATE_PRESETS = [30, 60, 90, 120] as const;
/** The AprilTag preset's exposure: short enough for a moving robot, long enough for the tag's edges. */
export const APRILTAG_EXPOSURE_US = 1500;
/** The range the UI assumes when a camera does not list one (Styx's OV9782 exposure range). */
export const EXPOSURE_FALLBACK: Range = { min: 9, max: 476_066 };

const LABELS: Record<string, string> = {
  exposure_time_us: "Exposure",
  gain: "Gain",
  frame_duration_us: "Frame duration",
  frame_rate: "Frame rate",
  ae_enable: "Auto exposure",
  exposure_value: "Exposure compensation",
  ae_state: "Auto exposure state",
  awb_enable: "Auto white balance",
  colour_temperature: "Colour temperature",
  red_gain: "Red gain",
  blue_gain: "Blue gain",
  ae_flicker_mode: "Flicker avoidance",
  ae_deflicker_mode: "Deflicker",
  ae_flicker_detected: "Flicker detected",
  output_crop: "Output crop",
  ae_exposure_max_us: "Max exposure (motion blur limit)",
  ae_gain_max: "Max gain",
  ae_exposure_mode: "Exposure mode",
  ae_metering_mode: "Metering",
  ae_metering_region: "Metering region",
  rotation: "Rotation",
  hflip: "Mirror left-right",
  vflip: "Mirror top-bottom",
};

/** What a control does, where the name alone does not say (shown as the row's help). */
const HELP: Record<string, string> = {
  rotation:
    "Turns the sensor's readout 180° for a camera mounted upside down. Restarts the capture once. The calibration and mount follow it.",
  hflip: "Mirrors the image left-right. AprilTags do not decode mirrored; for an upside-down camera use Rotation. Restarts the capture once.",
  vflip: "Mirrors the image top-bottom. AprilTags do not decode mirrored; for an upside-down camera use Rotation. Restarts the capture once.",
};

/** A control's help: what it does when known, and its device name. */
export function helpOf(control: Pick<CameraControlInfo, "name" | "standard">): string {
  const id = `${control.name}${control.standard ? ` · standard ${control.standard}` : ""}`;
  return HELP[control.name] ? `${HELP[control.name]} (${id})` : id;
}

const UNITS: Record<string, string> = {
  exposure_time_us: "µs",
  frame_duration_us: "µs",
  frame_rate: "fps",
  colour_temperature: "K",
  gain: "×",
  red_gain: "×",
  blue_gain: "×",
  exposure_value: "EV",
};

/** Group of each control the device lists, by name. Anything else is "advanced". */
const GROUP_BY_NAME: Record<string, ControlGroup> = {
  frame_rate: "capture",
  rotation: "capture",
  exposure_time_us: "exposure",
  gain: "exposure",
  ae_enable: "exposure",
  exposure_value: "exposure",
  ae_state: "exposure",
  awb_enable: "white",
  colour_temperature: "white",
  red_gain: "white",
  blue_gain: "white",
  ae_flicker_mode: "flicker",
  ae_deflicker_mode: "flicker",
  ae_flicker_detected: "flicker",
  output_crop: "crop",
  ae_exposure_max_us: "exposure",
  ae_gain_max: "exposure",
  ae_exposure_mode: "exposure",
  ae_metering_mode: "exposure",
  ae_metering_region: "exposure",
};

/** Fallback labels for a device that lists no menu for these (Styx's native menus, by index). */
const STATE_LABELS: Record<string, Record<number, string>> = {
  ae_state: { 1: "Searching", 2: "Converged" },
};

/** Fallback choices for a device that lists no menu for these int controls. */
const ENUM_OPTIONS: Record<string, { value: number; label: string }[]> = {
  ae_flicker_mode: [
    { value: 0, label: "Off" },
    { value: 1, label: "50 Hz" },
    { value: 2, label: "60 Hz" },
    { value: 3, label: "Auto" },
  ],
  rotation: [
    { value: 0, label: "0°" },
    { value: 180, label: "180°" },
  ],
  ae_deflicker_mode: [
    { value: 0, label: "Off" },
    { value: 1, label: "On" },
    { value: 2, label: "On, avoids flicker" },
  ],
};

export const isCropName = (name: string): boolean => name === "output_crop" || /^region_crop_\d+$/.test(name);
/** The device's maximum-exposure limit (for motion blur), if it lists one. */
export const isMaxExposureName = (name: string): boolean => /exposure/.test(name) && /max/.test(name) && !isCropName(name);

/** A friendly label; the raw name is kept for tooltips. */
export function friendlyLabel(name: string): string {
  if (LABELS[name]) return LABELS[name];
  const region = /^region_crop_(\d+)$/.exec(name);
  if (region) return `Region crop ${region[1]}`;
  const words = name.replace(/_/g, " ").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** The unit a control's value is in (shown beside it). Undefined when it has none. */
export function unitOf(name: string): string | undefined {
  if (UNITS[name]) return UNITS[name];
  if (name.endsWith("_us")) return "µs";
  if (name.endsWith("_ms")) return "ms";
  return undefined;
}

/** The group of each standard role: a device's control answers it whatever the device calls it. */
const ROLE_GROUP: Record<string, ControlGroup> = {
  fps: "capture",
  exposure_us: "exposure",
  gain: "exposure",
  ae: "exposure",
  ev: "exposure",
  awb: "white",
  colour_temperature: "white",
  red_gain: "white",
  blue_gain: "white",
};

/** The pane section a control belongs to, by its standard role, else its name. Crop regions and unknown controls are never lost. */
export function groupOf(control: Pick<CameraControlInfo, "name"> & Partial<Pick<CameraControlInfo, "standard">>): ControlGroup {
  if (isCropName(control.name)) return "crop";
  const byRole = control.standard ? ROLE_GROUP[control.standard] : undefined;
  if (byRole) return byRole;
  const named = GROUP_BY_NAME[control.name];
  if (named) return named;
  if (isMaxExposureName(control.name)) return "exposure";
  return "advanced";
}

/** The value a control takes when it is put back to its default; null when there is none to take. */
export function resetTarget(control: Control): number | boolean | null {
  if (!control.writable || control.default === null || control.default === undefined) return null;
  if (typeof control.default === "boolean") return control.default;
  const d = control.default;
  if (control.min !== undefined && d < control.min) return null;
  if (control.max !== undefined && d > control.max) return null;
  return d;
}

/** The control is away from its default (so the row shows a reset). */
export function isChanged(control: Control): boolean {
  const target = resetTarget(control);
  if (target === null || control.value === null) return false;
  if (typeof target === "number" && typeof control.value === "number") return Math.abs(target - control.value) > 1e-6;
  return target !== control.value;
}

/**
 * The control was changed by the user, not just left away from its default. A camera in auto keeps
 * its own live value for what the auto mode sets: the colour temperature under auto white balance is
 * its estimate, and the exposure and gain under auto exposure are its live values.
 */
export function changedByUser(controls: CameraControlInfo[], control: CameraControlInfo): boolean {
  if (!isChanged(control)) return false;
  if (control === controlFor(controls, "colour_temperature", "colour_temperature") && autoOn(controlFor(controls, "awb", "awb_enable")) === true) return false;
  const exposureAuto = autoOn(controlFor(controls, "ae", "ae_enable")) === true;
  if (exposureAuto && (control === controlFor(controls, "exposure_us", "exposure_time_us") || control === controlFor(controls, "gain", "gain"))) return false;
  return true;
}

/** Why the control cannot be changed here, or null when it can. */
export function disabledReason(control: Pick<CameraControlInfo, "writable" | "readOnly">): string | null {
  if (control.readOnly) return "Read only: the camera reports it";
  if (!control.writable) return "The camera service does not let the UI change it";
  return null;
}

/** Choices for a control with labelled values: its menu (by index), a known label map, or none. */
export function enumOptions(control: Pick<CameraControlInfo, "name" | "menu">): { value: number; label: string }[] | null {
  if (control.menu && control.menu.length > 0) return control.menu.map((label, value) => ({ value, label }));
  return ENUM_OPTIONS[control.name] ?? null;
}

/** A state value's words ("Converged"), or null for an unknown value. */
export function stateLabel(name: string, value: number | boolean | null): string | null {
  if (typeof value !== "number") return null;
  return STATE_LABELS[name]?.[value] ?? null;
}

/** Mains flicker from the detected light-flicker period: the light flickers at twice the mains. */
export function flickerLabel(periodUs: number | null): string {
  if (periodUs === null || periodUs <= 0) return "None yet";
  return `${Math.round(1_000_000 / (2 * periodUs))} Hz mains`;
}

/** A crop rectangle, or "Off" when it is zero-sized (the whole frame, or no region). */
export function rectText(value: CropRect | CropRect[] | null | undefined): string {
  if (value === null || value === undefined) return "not readable";
  const rects = Array.isArray(value) ? value : [value];
  const live = rects.filter((r) => r.width > 0 && r.height > 0);
  if (live.length === 0) return "Off";
  const r = live[0];
  return `${r.width}×${r.height} at ${r.x},${r.y}`;
}

/** A number with at most four significant figures (six from 1000 up), no trailing zeros. */
export function formatNumber(value: number): string {
  if (!Number.isFinite(value)) return String(value);
  return String(Number(value.toPrecision(Math.abs(value) >= 1000 ? 6 : 4)));
}

/** A millisecond figure for an exposure in µs: 1.5, 16.7, 0.009. */
export function formatMs(us: number): string {
  return formatNumber(us / 1000);
}

/** The value a control reads as text: a read-only row, or the value beside its control. */
export function valueText(control: Pick<CameraControlInfo, "name" | "value" | "rect"> & Partial<Pick<CameraControlInfo, "menu">>): string {
  if (control.rect !== undefined) return rectText(control.rect);
  const value = control.value;
  if (value === null) return "not readable";
  if (typeof value === "boolean") return value ? "On" : "Off";
  if (control.name === "ae_flicker_detected") return flickerLabel(value);
  const labelled = control.menu?.[value];
  if (labelled !== undefined) return labelled;
  const state = stateLabel(control.name, value);
  if (state) return state;
  const unit = unitOf(control.name);
  return `${formatNumber(value)}${unit ? ` ${unit}` : ""}`;
}

/**
 * The range a control takes now: the camera's effective range when it reports one (AE's limits
 * at the current frame duration), else the device's own limits, else the fallback.
 */
export function rangeOr(control: Partial<Pick<CameraControlInfo, "min" | "max" | "effectiveMin" | "effectiveMax">> | undefined, fallback: Range): Range {
  return {
    min: control?.effectiveMin ?? control?.min ?? fallback.min,
    max: control?.effectiveMax ?? control?.max ?? fallback.max,
  };
}

/** A short note for a write the camera clamped: what was asked for, and the value it set. */
export function clampedNote(control: { name: string }, requested: number | boolean | null, value: number | boolean | null): string {
  return `${friendlyLabel(control.name)}: ${valueText({ name: control.name, value: requested })} is outside what the camera takes now, so it is ${valueText({ name: control.name, value })}`;
}

/** A control an AprilTag preset writes, before it did: its value and whether the camera kept it. */
export type PresetBefore = Record<string, { kept: boolean; previous: number | boolean | null }>;

/**
 * What undoing a preset writes: each key it wrote that the camera kept before goes back to its
 * previous value, and each one it did not keep is forgotten (`null`), so the camera's stored values
 * end up exactly as they were. A kept key whose previous value is unknown is left alone.
 */
export function presetUndoChanges(before: PresetBefore): Record<string, number | boolean | null> {
  const changes: Record<string, number | boolean | null> = {};
  for (const [key, { kept, previous }] of Object.entries(before)) {
    if (!kept) changes[key] = null;
    else if (previous !== null) changes[key] = previous;
  }
  return changes;
}

/**
 * The AprilTag preset with an exposure ceiling (`ae_exposure_max_us`): auto exposure on, held to
 * the tag exposure, gain up to the device's gain cap (or 16). Keys are the camera's write keys;
 * `gainMax` is absent when the camera has no gain cap.
 */
export function aprilTagCapWrites(keys: { ae: string; exposureMax: string; gainMax?: string }, gainCapMax: number | undefined): Record<string, number | boolean> {
  const writes: Record<string, number | boolean> = { [keys.ae]: true, [keys.exposureMax]: APRILTAG_EXPOSURE_US };
  if (keys.gainMax) writes[keys.gainMax] = gainCapMax ?? 16;
  return writes;
}

/** A limit control where 0 means off (`ae_exposure_max_us`, `ae_gain_max`): whether it limits now. */
export function limitOn(value: number | boolean | null | undefined): boolean {
  return typeof value === "number" && value > 0;
}

/** The V4L2 exposure menu's manual value; aperture priority is 3, and any other value is auto. */
const V4L2_MANUAL = 1;

/**
 * Whether an auto mode (auto exposure, auto white balance) is on. A boolean control says so. A menu
 * whose entries name "Manual" is the V4L2 exposure menu (manual is 1, so any other value is auto).
 * Null when the device says neither: the UI then reads the mode as manual and still offers both
 * writes (as booleans).
 */
export function autoOn(control: Pick<CameraControlInfo, "value" | "menu"> | undefined): boolean | null {
  if (!control || control.value === null) return null;
  if (typeof control.value === "boolean") return control.value;
  const menuHasManual = control.menu?.some((label) => /manual/i.test(label)) ?? false;
  return menuHasManual ? control.value !== V4L2_MANUAL : null;
}

/** The standard roles the fields use (the API's names for them, see `CameraStandardControl`). */
export type Role = "exposure_us" | "gain" | "ae" | "ev" | "fps" | "awb" | "colour_temperature" | "red_gain" | "blue_gain";

/**
 * The control that plays a role. A device's control for it carries the standard role, whatever
 * the device calls it (a UVC "Exposure Time, Absolute" is `exposure_us`); `name` is only the
 * fallback for a control the device lists without a role.
 */
export function controlFor(controls: CameraControlInfo[], role: Role, name?: string): CameraControlInfo | undefined {
  return controls.find((c) => c.standard === role) ?? (name ? controlNamed(controls, name) : undefined);
}

/** A control with no standard role, by its name (`ae_state`, `ae_gain_max`...). */
export function controlNamed(controls: CameraControlInfo[], name: string): CameraControlInfo | undefined {
  return controls.find((c) => c.name === name && !c.standard);
}

/** The roles the camera fields show, with the name each falls back to (see `controlFor`). */
const FIELD_ROLES: [Role, string][] = [
  ["ae", "ae_enable"],
  ["exposure_us", "exposure_time_us"],
  ["gain", "gain"],
  ["ev", "exposure_value"],
  ["fps", "frame_rate"],
  ["awb", "awb_enable"],
  ["colour_temperature", "colour_temperature"],
  ["red_gain", "red_gain"],
  ["blue_gain", "blue_gain"],
];

/** The controls the camera fields show, so a pane's generic lists leave them out. */
export function fieldControlsOf(controls: CameraControlInfo[]): Set<CameraControlInfo> {
  return new Set(FIELD_ROLES.flatMap(([role, name]) => controlFor(controls, role, name) ?? []));
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/** The frame-rate presets the camera's range allows. */
export function frameRatePresetsIn(range: Range): number[] {
  return FRAME_RATE_PRESETS.filter((p) => p >= range.min && p <= range.max);
}

/** The preset a frame rate equals, or "custom". */
export function frameRateChoice(fps: number | null): string {
  if (fps === null) return "custom";
  const rounded = Math.round(fps);
  return (FRAME_RATE_PRESETS as readonly number[]).includes(rounded) && Math.abs(fps - rounded) < 1e-6 ? String(rounded) : "custom";
}

/** The longest exposure a frame rate allows: one frame, within the sensor's own limits. */
export function maxExposureUs(exposure: Range, fps: number | null): number {
  const frame = fps !== null && fps > 0 ? 1_000_000 / fps : Infinity;
  return Math.max(exposure.min, Math.min(exposure.max, frame));
}

/** An exposure in µs, kept within the sensor's range and one frame at this frame rate. */
export function clampExposure(us: number, exposure: Range, fps: number | null): number {
  return Math.round(clamp(us, exposure.min, maxExposureUs(exposure, fps)));
}

/**
 * The AprilTag preset: a short manual exposure, with gain raised by the same factor so the
 * image keeps its brightness (exposure x gain). `capped` when the gain limit is hit first.
 */
export function aprilTagPlan(
  now: { exposureUs: number | null; gain: number | null },
  ranges: { exposure: Range; gain: Range; fps: number | null },
): { exposureUs: number; gain: number; capped: boolean } {
  const exposureUs = Math.round(clamp(APRILTAG_EXPOSURE_US, ranges.exposure.min, maxExposureUs(ranges.exposure, ranges.fps)));
  const brightness = (now.exposureUs ?? 0) * (now.gain ?? 0);
  const wanted = brightness > 0 ? brightness / exposureUs : ranges.gain.min;
  const gain = Math.round(clamp(wanted, ranges.gain.min, ranges.gain.max) * 100) / 100;
  return { exposureUs, gain, capped: wanted > ranges.gain.max };
}
