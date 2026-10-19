// What the ring shows for a state, and what it shows while it is being previewed or animated
// client-side. Pure: no DOM, no clock of its own (the caller passes the elapsed time).

import type { LedState } from "#lib/api/leds.js";
import { LOOKS, type Look, type LookName } from "./looks";
import { blackFrame, renderLook, renderSpec, rainbow, type Frame } from "./render";
import { rgbOf, scale, type Rgb } from "./colour";
import { LEDS } from "./ring";

/** The look each state shows (null: the device draws the state as frames, or the ring is dark). */
export const STATE_LOOK: Record<LedState, LookName | null> = {
  locate: null,
  error: "helios.error",
  calibrating: "helios.calibrating",
  "robot-wait": "helios.robot-wait",
  "robot-wait-tags": "helios.robot-wait-tags",
  tags: null,
  "tags-locked": null,
  searching: "helios.searching",
  "robot-enabled": "helios.robot-enabled",
  starting: "helios.starting",
  recording: "helios.recording",
  idle: null,
};

/** One short line per state, for the picker. Cool colours are healthy, warm are trouble. */
export const STATE_HELP: Record<LedState, string> = {
  locate: "A rainbow chase round the ring, so you can tell which device is which.",
  error: "Red breathing: something failed, or the camera was lost.",
  calibrating: "A green arc fills as the calibration views are captured.",
  "robot-wait": "Orange twin comets: waiting for the robot code to connect.",
  "robot-wait-tags": "Cyan glow under orange comets: tags are seen, the robot code is not connected.",
  tags: "Cyan spots mark the tags the camera sees.",
  "tags-locked": "White: the field pose is locked from the tags.",
  searching: "A violet comet: looking for tags.",
  "robot-enabled": "A blue ripple out from the top: the robot is enabled.",
  starting: "A soft white breath with sparkles while HeliOS starts.",
  recording: "A red glow at the top while recording.",
  idle: "Ring dark: nothing to show.",
};

/** Looks for the states the device draws as frames, approximated for the preview. */
const FALLBACK: Partial<Record<LedState, Look>> = {
  tags: { envelope: { kind: "breathe", period_ms: 4000, depth: 0.2 }, brightness: 0.7, layers: [{ block: "fill", color: "28c8ff" }] },
  "tags-locked": { brightness: 0.5, layers: [{ block: "fill", color: "ffffff" }] },
};

/** The calibration arc's progress: the event does not carry it, so it eases back and forth. */
export function sweepProgress(elapsedMs: number): number {
  return 0.5 - 0.5 * Math.cos((2 * Math.PI * elapsedMs) / 12000);
}

/** A state's frame at `elapsedMs` since the state began, for previewing it. */
export function statePreview(state: LedState, elapsedMs: number): Frame {
  const name = STATE_LOOK[state];
  if (name) return renderLook(name, elapsedMs, sweepProgress(elapsedMs)) ?? blackFrame();
  if (state === "locate") return rainbow(elapsedMs);
  const look = FALLBACK[state];
  return look ? renderSpec(look, elapsedMs) : blackFrame();
}

/** What the ring is showing: the state, and the look or frame the device reported. */
export interface LedView {
  state: LedState;
  look: string | null;
  brightness: number;
  leds: string[] | null;
}

/**
 * The 16 colours to draw for a view at `elapsedMs` since its look started. Device frames are
 * drawn as they are; a named look is animated here with the device's ring-wide brightness; a
 * state with neither falls back to its preview.
 */
export function frameFor(view: LedView, elapsedMs: number): Frame {
  if (view.leds && view.leds.length === LEDS) return view.leds.map((hex) => rgbOf(hex));
  const progress = view.state === "calibrating" || (view.look ?? "").includes("calibrating") ? sweepProgress(elapsedMs) : 0;
  const looked = view.look ? renderLook(view.look, elapsedMs, progress) : null;
  const shown = looked ?? statePreview(view.state, elapsedMs);
  return shown.map((c: Rgb) => scale(c, view.brightness));
}
