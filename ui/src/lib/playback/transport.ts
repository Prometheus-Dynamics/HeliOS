// Playback transport logic: pure functions over a player's replay status, so the
// transport bar, the keyboard and the tests all make the same decisions. Times
// are milliseconds from the start of the recording; frames are 0-based, as the
// API counts them. A "change" is one PATCH of camera controls (the replay_*
// names of /v1/cameras/{resource_id}/settings).

import type { PlaybackStatus } from "#lib/api/types.js";

/** One PATCH of replay controls, by control name. */
export type ReplayChange = Record<string, boolean | number>;

/** Keyboard seeks move this far. */
export const SEEK_STEP_MS = 5_000;
const HOUR_MS = 3_600_000;

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

// --- Time and frames ----------------------------------------------------------

/** m:ss.mmm, or h:mm:ss once the recording (or the position in it) is an hour or longer. */
export function formatTime(ms: number, durationMs: number = ms): string {
  const total = Math.max(0, Math.round(ms));
  const hours = Math.floor(total / HOUR_MS);
  const minutes = Math.floor(total / 60_000) % 60;
  const seconds = Math.floor(total / 1000) % 60;
  if (Math.max(total, durationMs) >= HOUR_MS) return `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
  return `${minutes}:${String(seconds).padStart(2, "0")}.${String(total % 1000).padStart(3, "0")}`;
}

/** "position / duration", in the same units. */
export function timeReadout(status: Pick<PlaybackStatus, "position_ms" | "duration_ms">): string {
  return `${formatTime(status.position_ms, status.duration_ms)} / ${formatTime(status.duration_ms)}`;
}

/** "frame / frame_count", 0-based as the API counts frames. */
export function frameReadout(status: Pick<PlaybackStatus, "frame" | "frame_count">): string {
  return `${status.frame} / ${status.frame_count}`;
}

/** Milliseconds per frame from the recording's length and frame count (0 when either is unknown). */
export function msPerFrame(status: Pick<PlaybackStatus, "duration_ms" | "frame_count">): number {
  return status.frame_count > 0 && status.duration_ms > 0 ? status.duration_ms / status.frame_count : 0;
}

/** The frame that shows at `ms`, clamped to the recording's frames. */
export function frameAt(ms: number, status: Pick<PlaybackStatus, "duration_ms" | "frame_count">): number {
  if (!msPerFrame(status)) return 0;
  // Multiply before dividing: exact for whole frames, where ms / (duration / count) can fall just short.
  return clamp(Math.floor((ms * status.frame_count) / status.duration_ms), 0, Math.max(0, status.frame_count - 1));
}

/** The time a frame starts at, clamped to [0, duration]. */
export function timeOfFrame(frame: number, status: Pick<PlaybackStatus, "duration_ms" | "frame_count">): number {
  if (!msPerFrame(status)) return 0;
  return clamp(Math.round((frame * status.duration_ms) / status.frame_count), 0, Math.max(0, status.duration_ms));
}

/** Clamps a time to [0, duration]. */
export function clampMs(ms: number, durationMs: number): number {
  return clamp(ms, 0, Math.max(0, durationMs));
}

/** Position as a fraction of the duration, in [0, 1] (0 when the duration is unknown). */
export function fractionOf(positionMs: number, durationMs: number): number {
  return durationMs > 0 ? clamp(positionMs / durationMs, 0, 1) : 0;
}

/** The time at a fraction of the duration (a scrubber's value), clamped. */
export function msAtFraction(fraction: number, durationMs: number): number {
  return Math.round(clampMs(fraction * durationMs, durationMs));
}

/** The frame one step back, or null at the first frame. */
export function previousFrame(status: Pick<PlaybackStatus, "frame">): number | null {
  return status.frame > 0 ? status.frame - 1 : null;
}

/** Whether a step forward has a frame to go to (the step itself is replay_step). */
export function canStepForward(status: Pick<PlaybackStatus, "frame" | "frame_count">): boolean {
  return status.frame + 1 < status.frame_count;
}

// --- Speed and loop -----------------------------------------------------------

/** The speeds the menu offers, slowest first. 0 is every frame (as fast as the consumers take them), so it goes last. */
export const SPEEDS: readonly { value: number; label: string }[] = [
  { value: 0.25, label: "0.25×" },
  { value: 0.5, label: "0.5×" },
  { value: 1, label: "1×" },
  { value: 2, label: "2×" },
  { value: 4, label: "4×" },
  { value: 0, label: "Every frame" },
];

/** A speed's label; `null` (the device did not say) reads as 1×. */
export function speedLabel(value: number | null | undefined): string {
  if (value === null || value === undefined) return "1×";
  const known = SPEEDS.find((s) => s.value === value);
  if (known) return known.label;
  return `${Number(value.toFixed(2))}×`;
}

/** The menu's options for the current speed; a speed the menu does not list (set elsewhere) is added so it shows. */
export function speedOptions(current: number | null | undefined): { value: number; label: string }[] {
  if (current === null || current === undefined || SPEEDS.some((s) => s.value === current)) return [...SPEEDS];
  return [...SPEEDS, { value: current, label: speedLabel(current) }].sort((a, b) => rank(a.value) - rank(b.value));
}

/** Position in the speed order: every frame sorts after 4×. */
function rank(speed: number): number {
  return speed === 0 ? Number.POSITIVE_INFINITY : speed;
}

/** The next faster speed, or the current one when already the fastest. */
export function nextSpeed(current: number | null | undefined): number {
  const from = rank(current ?? 1);
  const next = SPEEDS.find((s) => rank(s.value) > from);
  return next ? next.value : (current ?? 1);
}

/** The next slower speed, or the current one when already the slowest. */
export function previousSpeed(current: number | null | undefined): number {
  const from = rank(current ?? 1);
  const below = SPEEDS.filter((s) => rank(s.value) < from);
  return below.length ? below[below.length - 1].value : (current ?? 1);
}

// --- Play state ---------------------------------------------------------------

/** Whether the status can be driven at all (a player whose camera service did not answer has none). */
export function transportKnown(status: PlaybackStatus | null | undefined): status is PlaybackStatus {
  return !!status && status.state !== "unknown";
}

export const isPlaying = (status: PlaybackStatus | null | undefined): boolean => status?.state === "playing";

export const isEnded = (status: PlaybackStatus | null | undefined): boolean => status?.state === "ended";

/** What the play button does now: pause while playing, start over once ended, play otherwise. */
export function playAction(status: PlaybackStatus | null | undefined): "pause" | "play" | "replay" {
  if (isPlaying(status)) return "pause";
  if (isEnded(status)) return "replay";
  return "play";
}

// --- Changes ------------------------------------------------------------------

export const playChange = (playing: boolean): ReplayChange => ({ replay_playing: playing });
export const speedChange = (speed: number): ReplayChange => ({ replay_speed: speed });
export const loopChange = (loop: boolean): ReplayChange => ({ replay_loop: loop });
export const stepChange = (): ReplayChange => ({ replay_step: true });
export const seekMsChange = (ms: number): ReplayChange => ({ replay_seek_ms: Math.round(ms) });
export const seekFrameChange = (frame: number): ReplayChange => ({ replay_seek_frame: frame });

// --- Keyboard -----------------------------------------------------------------

export type ShortcutAction = "toggle" | "pause" | "step-back" | "step-forward" | "seek-back" | "seek-forward";

/** The action for a key, or null when the key is not a playback shortcut. */
export function shortcutFor(key: string): ShortcutAction | null {
  switch (key) {
    case " ":
    case "Spacebar":
      return "toggle";
    case "k":
    case "K":
      return "pause";
    case "ArrowLeft":
    case ",":
      return "step-back";
    case "ArrowRight":
    case ".":
      return "step-forward";
    case "j":
    case "J":
      return "seek-back";
    case "l":
    case "L":
      return "seek-forward";
    default:
      return null;
  }
}

/** The shortcut legend, as the help shows it. */
export const SHORTCUTS: { keys: string[]; action: string }[] = [
  { keys: ["Space"], action: "Play or pause" },
  { keys: ["K"], action: "Pause" },
  { keys: ["←", ","], action: "Step back one frame" },
  { keys: ["→", "."], action: "Step forward one frame" },
  { keys: ["J"], action: "Back 5 seconds" },
  { keys: ["L"], action: "Forward 5 seconds" },
];

/**
 * The PATCHes an action sends, in order (empty when there is nothing to do). Steps are sent one
 * after another: a play from the end starts from the beginning.
 */
export function planAction(action: ShortcutAction, status: PlaybackStatus): ReplayChange[] {
  switch (action) {
    case "toggle":
      return planPlay(status);
    case "pause":
      return [playChange(false)];
    case "step-back": {
      const frame = previousFrame(status);
      return frame === null ? [] : [seekFrameChange(frame)];
    }
    case "step-forward":
      return canStepForward(status) ? [stepChange()] : [];
    case "seek-back":
      return [seekMsChange(clampMs(status.position_ms - SEEK_STEP_MS, status.duration_ms))];
    case "seek-forward":
      return [seekMsChange(clampMs(status.position_ms + SEEK_STEP_MS, status.duration_ms))];
  }
}

/** The PATCHes for the play button (see `planAction`). */
export function planPlay(status: PlaybackStatus): ReplayChange[] {
  switch (playAction(status)) {
    case "pause":
      return [playChange(false)];
    case "replay":
      return [seekMsChange(0), playChange(true)];
    case "play":
      return [playChange(true)];
  }
}
