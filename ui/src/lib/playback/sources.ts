// What a player replays and what a run used: parsing what the user typed, the
// labels the lists show, and the calibration and mount sources of a run.

import type { PlaybackSource, RunContextFrom, StoredRecording } from "#lib/api/types.js";

export type SourceParse = { ok: true; source: PlaybackSource } | { ok: false; error: string };

/** A typed path (an absolute path to a .mcap file or an image folder) as a source. */
export function parsePath(text: string): SourceParse {
  const value = text.trim();
  if (!value) return { ok: false, error: "Pick a recording, or type the path of a .mcap file or an image folder." };
  if (!value.startsWith("/")) return { ok: false, error: "The path must be absolute, starting with /." };
  return { ok: true, source: { path: value } };
}

export const recordingSource = (id: string): PlaybackSource => ({ recording: id });

/** The source as the lists name it: the recording's id, or the path. */
export function sourceLabel(source: PlaybackSource): string {
  return "recording" in source ? source.recording : source.path;
}

/** Stored recordings newest first (the device sorts them so; this keeps the order when they arrive unsorted). */
export function newestFirst(list: StoredRecording[]): StoredRecording[] {
  return [...list].sort((a, b) => b.modified_at_ms - a.modified_at_ms || b.id.localeCompare(a.id));
}

/** One run's context, as a sentence: where its calibration or mount came from. */
export function contextText(what: "calibration" | "mount", from: RunContextFrom): string {
  switch (from.from) {
    case "recording":
      return `${what}: from the recording`;
    case "camera":
      return `${what}: from live camera ${from.camera}`;
    case "none":
      return `${what}: none (neither the recording nor the live camera has one)`;
  }
}
