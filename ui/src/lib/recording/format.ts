// Recording labels, tones and numbers as the screens show them. Pure: no
// state, no stores, so the tests can check it directly.

import { bytes, duration } from "../format";
import type { RecordingState, RecordingStatus, RingStatus, StorageFacts } from "../api/recording-types";

export type RecordingTone = "neutral" | "ok" | "warn" | "err" | "info";

const STATES: Record<RecordingState, { label: string; tone: RecordingTone }> = {
  starting: { label: "Starting", tone: "info" },
  recording: { label: "Recording", tone: "ok" },
  finishing: { label: "Finishing", tone: "warn" },
  stopped: { label: "Stopped", tone: "neutral" },
  failed: { label: "Failed", tone: "err" },
};

export function stateLabel(state: RecordingState): string {
  return STATES[state]?.label ?? state;
}

export function stateTone(state: RecordingState): RecordingTone {
  return STATES[state]?.tone ?? "neutral";
}

/** A recording that is writing (or about to): it holds the camera's slot until it stops. */
export function isActive(state: RecordingState): boolean {
  return state === "starting" || state === "recording" || state === "finishing";
}

const STOP_REASONS: Record<string, string> = {
  requested: "Stopped by you",
  storage_reserve: "Stopped: the storage reserve was reached",
  camera_lost: "Stopped: the camera was lost",
  cap: "Stopped: the size or time limit was reached",
};

/** Why a recording ended, in words (null while it has no reason). */
export function stopReasonText(reason: string | null): string | null {
  if (!reason) return null;
  return STOP_REASONS[reason] ?? reason.replace(/_/g, " ");
}

/** A running clock: `0:07`, `12:03`, `1:02:44`. */
export function clockText(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/**
 * How long a recording has run. The device reports `duration_ms` about once a second; while it
 * runs, the time since that report arrived is counted on from it (so the clock never depends on
 * the device's clock).
 */
export function elapsedMs(status: RecordingStatus, receivedAt: number, now: number): number {
  return isActive(status.state) ? status.duration_ms + Math.max(0, now - receivedAt) : status.duration_ms;
}

/** A duration in milliseconds, as the lists show it (`45s`, `3m 12s`, `2h 5m`). */
export function durationText(ms: number): string {
  return duration(ms / 1000);
}

/** When a recording was made, for a list (`Oct 19, 09:41`). */
export function whenText(ms: number): string {
  return new Date(ms).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

/** A size in bytes, as the screens show it (`9.2 GiB`, `512 KiB`). */
export function sizeText(n: number): string {
  return bytes(n);
}

export interface DropSummary {
  dropped: number;
  written: number;
  /** Dropped over all frames the recorder saw, 0 to 1. */
  ratio: number;
  tone: RecordingTone;
  text: string;
}

/** Frames the recorder could not keep up with, as a count and a share. Under 1% is a warning, more is an error. */
export function dropSummary(written: number, dropped: number): DropSummary {
  const total = written + dropped;
  const ratio = total > 0 ? dropped / total : 0;
  if (dropped === 0) return { dropped, written, ratio, tone: "ok", text: "No drops" };
  const tone: RecordingTone = ratio < 0.01 ? "warn" : "err";
  return { dropped, written, ratio, tone, text: `${dropped} dropped (${percentText(ratio)})` };
}

function percentText(ratio: number): string {
  const p = ratio * 100;
  return p < 10 ? `${p.toFixed(1)}%` : `${Math.round(p)}%`;
}

/** How much room is left against the reserve the recorder keeps free: `reserve` is past it, `low` is within twice it. */
export type SpaceLevel = "unknown" | "ok" | "low" | "reserve";

export function spaceLevel(free: number | null, reserve: number): SpaceLevel {
  if (free === null) return "unknown";
  if (free <= reserve) return "reserve";
  if (free <= reserve * 2) return "low";
  return "ok";
}

export function spaceTone(level: SpaceLevel): RecordingTone {
  return level === "reserve" ? "err" : level === "low" ? "warn" : level === "ok" ? "ok" : "neutral";
}

/** `180 GiB free, 2 GiB kept in reserve`. */
export function spaceText(free: number | null, reserve: number): string {
  if (free === null) return `Free space unknown, ${bytes(reserve)} kept in reserve`;
  return `${bytes(free)} free, ${bytes(reserve)} kept in reserve`;
}

/** The storage line: where recordings go, and whether it can take them now. */
export function storageText(storage: StorageFacts | null, error: string | null): { tone: RecordingTone; text: string } {
  if (error) return { tone: "err", text: error };
  if (!storage) return { tone: "neutral", text: "No storage yet" };
  const level = spaceLevel(storage.free_bytes, storage.reserve_bytes);
  return { tone: spaceTone(level), text: spaceText(storage.free_bytes, storage.reserve_bytes) };
}

export interface RingSummary {
  tone: "ok" | "info" | "err";
  text: string;
  /** How full the ring is, 0 to 1. */
  filled: number;
}

/** A capture-last ring: full (holding its configured seconds), filling, or failed. */
export function ringSummary(ring: RingStatus): RingSummary {
  if (ring.state === "failed") return { tone: "err", text: `Failed${ring.error ? `: ${ring.error}` : ""}`, filled: 0 };
  const filled = ring.seconds > 0 ? Math.min(1, ring.window_seconds / ring.seconds) : 0;
  if (filled >= 0.999) return { tone: "ok", text: `Holding the last ${ring.seconds} s`, filled: 1 };
  return { tone: "info", text: `Filling: ${Math.floor(ring.window_seconds)} of ${ring.seconds} s`, filled };
}

/** What an Eidos export's camera model is, in words. */
export function eidosCameraText(camera: "calibrated" | "nominal"): string {
  return camera === "calibrated" ? "Calibrated camera model" : "Nominal camera model (not calibrated)";
}
