// The HeliOS looks, mirrored from the Gaia asset `etc/lemnos/looks.d/helios.toml` (lemnosd reads
// that file). The keys keep the TOML's names so the two read side by side. The look names must
// match the file exactly: `tests/leds-looks.test.ts` fails when one is added or removed on one
// side only. Change a value here when the file changes.

export type Mode = "max" | "add" | "over";

export type Envelope =
  | "solid"
  | { kind: "breathe"; period_ms: number; depth: number; easing?: "ease-in-out" }
  | { kind: "blink"; period_ms: number; duty: number }
  | { kind: "pulse"; attack_ms: number; hold_ms: number; decay_ms: number; repeat?: number };

/** One layer of a look: a block and its parameters (the keys the TOML uses). */
export interface Layer {
  block: "fill" | "comet" | "arc" | "ripple" | "frame" | "wash" | "sparkle" | "spots";
  color?: string;
  // spots (`width` is shared with the ripple)
  follow_ms?: number;
  max_spots?: number;
  spots?: "input";
  colors?: string[];
  brightness?: number;
  mode?: Mode;
  envelope?: Envelope;
  // comet
  period_ms?: number;
  tail?: number;
  heads?: number;
  base?: number;
  reverse?: boolean;
  // arc
  fraction?: "input" | number;
  track?: string;
  head?: number;
  sheen?: boolean;
  // ripple
  origin?: number;
  speed?: number;
  width?: number;
  settle_ms?: number;
  glow?: number;
  // frame
  pixels?: string[];
  // sparkle
  density?: number;
  min_ms?: number;
  max_ms?: number;
}

export interface Look {
  envelope?: Envelope;
  brightness?: number;
  layers: Layer[];
}

const BREATHE_ERROR: Envelope = { kind: "breathe", period_ms: 3000, depth: 0.7, easing: "ease-in-out" };
const BREATHE_STARTING: Envelope = { kind: "breathe", period_ms: 3000, depth: 0.55, easing: "ease-in-out" };
const ARC_CALIBRATING: Layer = { block: "arc", fraction: "input", color: "2bd47d", track: "101012", head: 0.35, sheen: true };
const RIPPLE_ENABLED: Layer = { block: "ripple", color: "28c8ff", origin: 0, speed: 12, width: 2.2, settle_ms: 1400, glow: 0.15 };
const TALLY: Layer = { block: "frame", pixels: ["ff2828"], brightness: 0.6, mode: "max" };
const TAGS_BREATHE: Envelope = { kind: "breathe", period_ms: 4000, depth: 0.2, easing: "ease-in-out" };
const SPOTS_TAGS: Layer = { block: "spots", color: "28c8ff", width: 1.5, follow_ms: 120, spots: "input", max_spots: 8 };
const SPOTS_LOCKED: Layer = { ...SPOTS_TAGS, color: "d8f4ff" };
const GLOW_LOCKED: Layer = { block: "fill", color: "28c8ff", brightness: 0.25 };
const SHEEN_LOCKED: Layer = { block: "comet", color: "ffffff", period_ms: 6000, tail: 3, base: 0, brightness: 0.35 };

export const LOOKS = {
  "helios.error": { envelope: BREATHE_ERROR, brightness: 0.7, layers: [{ block: "fill", color: "ff2828" }] },
  "helios.calibrating": { envelope: "solid", layers: [ARC_CALIBRATING] },
  "helios.calibrating-view": { envelope: "solid", layers: [ARC_CALIBRATING, { ...RIPPLE_ENABLED, color: "2bd47d" }] },
  "helios.robot-wait": { envelope: "solid", layers: [{ block: "comet", color: "ff9614", period_ms: 2400, tail: 6, base: 0.18, heads: 2 }] },
  "helios.robot-wait-tags": {
    envelope: "solid",
    layers: [
      { block: "fill", color: "28c8ff", brightness: 0.25 },
      { block: "comet", color: "ff9614", period_ms: 2400, tail: 6, base: 0, heads: 2, mode: "over" },
    ],
  },
  "helios.searching": { envelope: "solid", layers: [{ block: "comet", color: "965aff", period_ms: 1600, tail: 7, base: 0.18 }] },
  "helios.starting": {
    envelope: "solid",
    layers: [
      { block: "wash", color: "ffffff", brightness: 0.09, envelope: BREATHE_STARTING },
      { block: "sparkle", color: "ffffff", density: 1.2, base: 0.03, min_ms: 350, max_ms: 950 },
    ],
  },
  "helios.robot-enabled": { envelope: "solid", layers: [RIPPLE_ENABLED] },
  "helios.recording": { envelope: "solid", layers: [{ block: "frame", pixels: ["ff2828"], brightness: 0.6 }] },
  // The tally variants: the state's layers, then the tally (at most four layers in all).
  "helios.rec.error": { envelope: BREATHE_ERROR, brightness: 0.7, layers: [{ block: "fill", color: "ff2828" }, TALLY] },
  "helios.rec.calibrating": { envelope: "solid", layers: [ARC_CALIBRATING, TALLY] },
  "helios.rec.calibrating-view": { envelope: "solid", layers: [ARC_CALIBRATING, { ...RIPPLE_ENABLED, color: "2bd47d" }, TALLY] },
  "helios.rec.robot-wait": { envelope: "solid", layers: [{ block: "comet", color: "ff9614", period_ms: 2400, tail: 6, base: 0.18, heads: 2 }, TALLY] },
  "helios.rec.robot-wait-tags": {
    envelope: "solid",
    layers: [
      { block: "fill", color: "28c8ff", brightness: 0.25 },
      { block: "comet", color: "ff9614", period_ms: 2400, tail: 6, base: 0, heads: 2, mode: "over" },
      TALLY,
    ],
  },
  "helios.rec.searching": { envelope: "solid", layers: [{ block: "comet", color: "965aff", period_ms: 1600, tail: 7, base: 0.18 }, TALLY] },
  "helios.rec.starting": {
    envelope: "solid",
    layers: [
      { block: "wash", color: "ffffff", brightness: 0.09, envelope: BREATHE_STARTING },
      { block: "sparkle", color: "ffffff", density: 1.2, base: 0.03, min_ms: 350, max_ms: 950 },
      TALLY,
    ],
  },
  "helios.rec.robot-enabled": { envelope: "solid", layers: [RIPPLE_ENABLED, TALLY] },
  "helios.tags": { envelope: TAGS_BREATHE, brightness: 0.7, layers: [{ block: "fill", color: "28c8ff" }, SPOTS_TAGS] },
  "helios.tags-locked": { envelope: "solid", layers: [GLOW_LOCKED, SHEEN_LOCKED, SPOTS_LOCKED] },
  "helios.rec.tags": { envelope: TAGS_BREATHE, brightness: 0.7, layers: [{ block: "fill", color: "28c8ff" }, SPOTS_TAGS, TALLY] },
  "helios.rec.tags-locked": { envelope: "solid", layers: [GLOW_LOCKED, SHEEN_LOCKED, SPOTS_LOCKED, TALLY] },
} satisfies Record<string, Look>;

export type LookName = keyof typeof LOOKS;

export const LOOK_NAMES = Object.keys(LOOKS) as LookName[];
