// The recordings settings form and the stored list, as pure data. The form
// shows bytes in MiB and the frame rate limit as a number or "as the camera
// delivers"; the device takes bytes and `null`. Fields the form does not show
// (USB roots, file caps, queue and sensor periods) pass through from the
// settings the device gave, so a save writes the whole object back unchanged.

import type { RecorderSettings, RecordFrames, StoredRecording } from "../api/recording-types";

const MIB = 1 << 20;
export const CAPTURE_LAST_RANGE = { min: 1, max: 600 } as const;
export const MAX_FPS_RANGE = { min: 1, max: 240 } as const;
/** The smallest budget the device takes for the recordings folder (its `max_total_bytes`). */
export const MIN_TOTAL_MIB = 1;

export interface RecordingForm {
  /** Empty: the device picks a mounted USB drive, else its fallback directory. */
  storageDir: string;
  /** The free space the recorder keeps, in MiB (fractions allowed; a MiB is exact in bytes). */
  reserveMiB: number;
  /** The most the recordings folder may hold, in MiB; `null`: the device's default for its volume. */
  maxTotalMiB: number | null;
  frames: RecordFrames;
  /** `null`: as the camera delivers. */
  maxFps: number | null;
  captureLastSeconds: number;
  captureLastCameras: string[];
  autoRecord: boolean;
  ntControl: boolean;
}

export type FormErrors = Partial<Record<keyof RecordingForm, string>>;

export function formFromSettings(s: RecorderSettings): RecordingForm {
  return {
    storageDir: s.storage_dir ?? "",
    reserveMiB: s.reserve_bytes / MIB,
    maxTotalMiB: s.max_total_bytes === null ? null : s.max_total_bytes / MIB,
    frames: s.frames,
    maxFps: s.max_fps,
    captureLastSeconds: s.capture_last_seconds,
    captureLastCameras: [...s.capture_last_cameras],
    autoRecord: s.auto_record,
    ntControl: s.nt_control,
  };
}

/** The whole settings object: the form's fields over `base` (what the device gave). */
export function settingsFromForm(form: RecordingForm, base: RecorderSettings): RecorderSettings {
  const storageDir = form.storageDir.trim();
  return {
    ...base,
    storage_dir: storageDir === "" ? null : storageDir,
    reserve_bytes: Math.round(form.reserveMiB * MIB),
    max_total_bytes: form.maxTotalMiB === null ? null : Math.round(form.maxTotalMiB * MIB),
    frames: form.frames,
    max_fps: form.maxFps === null || form.maxFps <= 0 ? null : form.maxFps,
    capture_last_seconds: form.captureLastSeconds,
    capture_last_cameras: [...form.captureLastCameras],
    auto_record: form.autoRecord,
    nt_control: form.ntControl,
  };
}

/** What is wrong with the form, field by field (empty when it can be saved). */
export function validateForm(form: RecordingForm): FormErrors {
  const errors: FormErrors = {};
  const dir = form.storageDir.trim();
  if (dir !== "" && !dir.startsWith("/")) errors.storageDir = "Use an absolute path, such as /media/usb/helios";
  if (!Number.isFinite(form.reserveMiB) || form.reserveMiB < 0) errors.reserveMiB = "Enter a size of 0 MiB or more";
  if (form.maxTotalMiB !== null && (!Number.isFinite(form.maxTotalMiB) || form.maxTotalMiB < MIN_TOTAL_MIB))
    errors.maxTotalMiB = `Enter a size of ${MIN_TOTAL_MIB} MiB or more, or use the device's default`;
  if (form.maxFps !== null && (!Number.isFinite(form.maxFps) || form.maxFps < MAX_FPS_RANGE.min || form.maxFps > MAX_FPS_RANGE.max))
    errors.maxFps = `Between ${MAX_FPS_RANGE.min} and ${MAX_FPS_RANGE.max} frames per second, or as the camera delivers`;
  const seconds = form.captureLastSeconds;
  if (!Number.isInteger(seconds) || seconds < CAPTURE_LAST_RANGE.min || seconds > CAPTURE_LAST_RANGE.max) errors.captureLastSeconds = `Whole seconds from ${CAPTURE_LAST_RANGE.min} to ${CAPTURE_LAST_RANGE.max}`;
  return errors;
}

/** True when the form would save nothing new. */
export function isUnchanged(form: RecordingForm, base: RecorderSettings): boolean {
  return JSON.stringify(settingsFromForm(form, base)) === JSON.stringify(base);
}

/** The list with `id` added, or removed when it is there (a checkbox). */
export function toggleInList(list: string[], id: string): string[] {
  return list.includes(id) ? list.filter((x) => x !== id) : [...list, id];
}

/** When a stored recording was made: its first frame, or its last write for a part without a summary. */
export function recordedAt(item: StoredRecording): number {
  return item.summary?.started_at_ms ?? item.modified_at_ms;
}

/** Newest first; equal times keep the id order (newest id first), so the list never shuffles. */
export function sortStored(list: StoredRecording[]): StoredRecording[] {
  return [...list].sort((a, b) => recordedAt(b) - recordedAt(a) || (a.id < b.id ? 1 : a.id > b.id ? -1 : 0));
}
