// The camera controls' names, labels, groups, units and maths. Pure (no stores, no DOM): the
// camera pane, the inspector's quick controls and their tests all read it, so a control means
// the same thing everywhere.
//
// Names and enum meanings come from Styx's native backend (`native_backend.rs`, the revision
// Backend/Cargo.lock pins): `controls::*` ids, `AE_FLICKER_MODE` (0 off, 1 50 Hz, 2 60 Hz,
// 3 automatic), `AE_DEFLICKER_MODE` (0 off, 1 on, 2 with flicker avoidance), `AE_STATE`
// (1 searching, 2 converged), `AE_FLICKER_DETECTED` (the light's flicker period in µs).

import type { CameraControlInfo, CropRect } from "#lib/api/model.js";

export type Control = Pick<CameraControlInfo, "name" | "standard" | "min" | "max" | "default" | "value" | "writable" | "readOnly">;
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
};

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
};

const STATE_LABELS: Record<string, Record<number, string>> = {
  ae_state: { 1: "Searching", 2: "Converged" },
};

/** Labelled choices for the int controls the device lists without a menu. */
const ENUM_OPTIONS: Record<string, { value: number; label: string }[]> = {
  ae_flicker_mode: [
    { value: 0, label: "Off" },
    { value: 1, label: "50 Hz" },
    { value: 2, label: "60 Hz" },
    { value: 3, label: "Auto" },
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

/** The pane section a control belongs to. Crop regions and unknown controls are never lost. */
export function groupOf(control: Pick<CameraControlInfo, "name">): ControlGroup {
  if (isCropName(control.name)) return "crop";
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
export function valueText(control: Pick<CameraControlInfo, "name" | "value" | "rect">): string {
  if (control.rect !== undefined) return rectText(control.rect);
  const value = control.value;
  if (value === null) return "not readable";
  if (typeof value === "boolean") return value ? "On" : "Off";
  if (control.name === "ae_flicker_detected") return flickerLabel(value);
  const state = stateLabel(control.name, value);
  if (state) return state;
  const unit = unitOf(control.name);
  return `${formatNumber(value)}${unit ? ` ${unit}` : ""}`;
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
