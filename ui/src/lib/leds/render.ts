// Renders a look to the 16 LEDs, the way lemnos-light does it (docs/looks.md in Lemnos), so the
// preview matches the ring: blocks, envelopes, the three blend modes, and the brightness steps.
// Approximate where the device's exact timing is not reproduced (the sparkle's random seeds,
// the ripple's settle); no DOM here.

import { LOOKS, type Envelope, type Layer, type Look, type LookName, type Mode } from "./looks";
import { BLACK, clamp01, hueOf, mix, rgbOf, scale, WHITE, type Rgb } from "./colour";
import { LEDS, mod, ringDistance } from "./ring";

export type Frame = Rgb[];

export function blackFrame(): Frame {
  return Array.from({ length: LEDS }, () => BLACK);
}

/** A look's envelope at `t` ms: 1 for solid, a value 0 to 1 otherwise. */
export function envelopeAt(envelope: Envelope | undefined, t: number): number {
  if (envelope === undefined || envelope === "solid") return 1;
  switch (envelope.kind) {
    case "breathe": {
      // Eases from full brightness down by `depth` and back once per period.
      const x = mod(t, envelope.period_ms) / envelope.period_ms;
      return 1 - envelope.depth * (0.5 - 0.5 * Math.cos(2 * Math.PI * x));
    }
    case "blink":
      return mod(t, envelope.period_ms) / envelope.period_ms < envelope.duty ? 1 : 0;
    case "pulse": {
      // Rises over attack, holds, then decays on a quadratic ease-out to 0. `repeat` runs
      // that many times (0 keeps going); after the last one the level is 0.
      const cycle = envelope.attack_ms + envelope.hold_ms + envelope.decay_ms;
      if (cycle <= 0) return 0;
      const repeat = envelope.repeat ?? 1;
      const n = Math.floor(t / cycle);
      if (repeat > 0 && n >= repeat) return 0;
      const x = t - n * cycle;
      if (x < envelope.attack_ms) return envelope.attack_ms > 0 ? x / envelope.attack_ms : 1;
      if (x < envelope.attack_ms + envelope.hold_ms) return 1;
      const y = envelope.decay_ms > 0 ? (x - envelope.attack_ms - envelope.hold_ms) / envelope.decay_ms : 1;
      return (1 - clamp01(y)) ** 2;
    }
  }
}

/** Composites `layer` over `below` per LED: `max`, `add` (saturating) or `over` (alpha). */
export function blend(below: Frame, layer: Frame, mode: Mode): Frame {
  return below.map((b, i) => {
    const l = layer[i];
    if (mode === "add") return [clamp01(b[0] + l[0]), clamp01(b[1] + l[1]), clamp01(b[2] + l[2])];
    if (mode === "over") {
      const a = Math.max(l[0], l[1], l[2]);
      return [b[0] * (1 - a) + l[0], b[1] * (1 - a) + l[1], b[2] * (1 - a) + l[2]];
    }
    return [Math.max(b[0], l[0]), Math.max(b[1], l[1]), Math.max(b[2], l[2])];
  });
}

/** A deterministic hash of an integer to [0, 1): the sparkle's stand-in for a seeded random. */
function unit(n: number): number {
  let x = Math.imul(n | 0, 0x9e3779b1) ^ 0x85ebca6b;
  x = Math.imul(x ^ (x >>> 15), 0x2c1b3c6d);
  x = Math.imul(x ^ (x >>> 12), 0x297a2d39);
  x ^= x >>> 15;
  return (x >>> 0) / 4294967296;
}

function comet(layer: Layer, t: number): Frame {
  const colour = rgbOf(layer.color ?? "ffffff");
  const period = layer.period_ms ?? 1200;
  const tail = layer.tail ?? 5;
  const heads = Math.max(1, Math.round(layer.heads ?? 1));
  const base = layer.base ?? 0;
  // One turn per period; `reverse` runs it the other way.
  const turn = mod(t, period) / period;
  const position = (layer.reverse ? 1 - turn : turn) * LEDS;
  const level = Array.from({ length: LEDS }, () => base);
  for (let h = 0; h < heads; h++) {
    const head = mod(position + (h * LEDS) / heads, LEDS);
    for (let i = 0; i < LEDS; i++) {
      // The head is anti-aliased: the LED it has reached is lit by the fraction it covers.
      const splat = Math.max(0, 1 - ringDistance(i, head));
      const behind = layer.reverse ? mod(i - head, LEDS) : mod(head - i, LEDS);
      const tailLevel = tail > 0 && behind < tail ? (1 - behind / tail) ** 2.2 : 0;
      level[i] = Math.max(level[i], splat, tailLevel);
    }
  }
  return level.map((v) => scale(colour, v));
}

function arc(layer: Layer, progress: number): Frame {
  const fraction = layer.fraction === undefined || layer.fraction === "input" ? progress : layer.fraction;
  const filled = clamp01(fraction) * LEDS;
  const track = rgbOf(layer.track ?? "101012");
  const colour = rgbOf(layer.color ?? "ffffff");
  const head = layer.head ?? 0.35;
  const leading = Math.ceil(filled) - 1;
  return Array.from({ length: LEDS }, (_, i) => {
    const cover = clamp01(filled - i);
    const lit = mix(track, colour, cover);
    // The leading LED is mixed toward white.
    return cover > 0 && i === leading ? mix(lit, WHITE, head) : lit;
  });
}

function ripple(layer: Layer, t: number): Frame {
  const colour = rgbOf(layer.color ?? "ffffff");
  const origin = layer.origin ?? 0;
  const speed = layer.speed ?? 12;
  const halfWidth = (layer.width ?? 2.2) / 2;
  const settle = Math.max(layer.settle_ms ?? 1400, 1);
  const glow = layer.glow ?? 0.15;
  // The wave runs out from the origin both ways, fading as it settles. The glow then peaks at
  // `settle` and fades to nothing by `2 * settle`.
  const front = (speed * t) / 1000;
  const wave = t < settle ? 1 - t / settle : 0;
  const settling = t < settle ? glow * (t / settle) : glow * Math.max(0, 2 - t / settle);
  return Array.from({ length: LEDS }, (_, i) => {
    const distance = ringDistance(i, origin);
    const crest = Math.max(0, 1 - Math.abs(distance - front) / halfWidth) * wave;
    return scale(colour, Math.max(crest, settling));
  });
}

function frame(layer: Layer): Frame {
  const pixels = layer.pixels ?? [];
  return Array.from({ length: LEDS }, (_, i) => (i < pixels.length ? rgbOf(pixels[i]) : BLACK));
}

function sparkle(layer: Layer, t: number): Frame {
  const colour = rgbOf(layer.color ?? layer.colors?.[0] ?? "ffffff");
  const density = Math.max(layer.density ?? 1.2, 0);
  const base = layer.base ?? 0;
  const minMs = layer.min_ms ?? 350;
  const maxMs = Math.max(layer.max_ms ?? 950, minMs);
  // Each LED starts a twinkle `density` times a second: one per slot, at a hashed place in it.
  const slot = density > 0 ? 1000 / density : Infinity;
  const now = Math.max(t, 0);
  const first = Number.isFinite(slot) ? Math.floor((now - maxMs) / slot) : 0;
  const last = Number.isFinite(slot) ? Math.floor(now / slot) : -1;
  return Array.from({ length: LEDS }, (_, i) => {
    let level = base;
    for (let k = first; k <= last; k++) {
      const start = (k + unit(i * 1000003 + k * 7919 + 1)) * slot;
      const length = minMs + unit(i * 999983 + k * 104729 + 2) * (maxMs - minMs);
      const x = (now - start) / length;
      if (x >= 0 && x < 1) level = Math.max(level, x < 0.2 ? x / 0.2 : (1 - (x - 0.2) / 0.8) ** 2);
    }
    return scale(colour, level);
  });
}

function fill(layer: Layer): Frame {
  return Array.from({ length: LEDS }, () => rgbOf(layer.color ?? "ffffff"));
}

/** A spot as the device reports it: a position in LEDs (0 to 16) and a level (0 to 1). */
export interface SpotInput {
  at: number;
  level: number;
}

/** A spots block: each spot a raised-cosine glow of `width` LEDs, combined by max. */
function spotsBlock(layer: Layer, spots: readonly SpotInput[]): Frame {
  const width = layer.width ?? 1.5;
  const colour = rgbOf(layer.color ?? "ffffff");
  let acc = blackFrame();
  for (const spot of spots.slice(0, layer.max_spots ?? 8)) {
    const one = Array.from({ length: LEDS }, (_, i) => {
      const reach = Math.max(0, 1 - ringDistance(i, spot.at) / width);
      return scale(colour, clamp01(spot.level) * reach * reach);
    });
    acc = blend(acc, one, "max");
  }
  return acc;
}

/** One layer's LEDs at `t` ms, with its own brightness and envelope applied. `spots` feed the spots block. */
export function renderLayer(layer: Layer, t: number, progress: number, spots: readonly SpotInput[] = []): Frame {
  let out: Frame;
  switch (layer.block) {
    case "fill":
    case "wash":
      out = fill(layer);
      break;
    case "spots":
      out = spotsBlock(layer, spots);
      break;
    case "comet":
      out = comet(layer, t);
      break;
    case "arc":
      out = arc(layer, progress);
      break;
    case "ripple":
      out = ripple(layer, t);
      break;
    case "frame":
      out = frame(layer);
      break;
    case "sparkle":
      out = sparkle(layer, t);
      break;
  }
  const k = (layer.brightness ?? 1) * envelopeAt(layer.envelope, t);
  return out.map((c) => scale(c, k));
}

/** A look's frame at `t` ms since it started. `progress` (0 to 1) feeds its arcs. */
export function renderSpec(look: Look, t: number, progress = 0, spots: readonly SpotInput[] = []): Frame {
  let acc = blackFrame();
  for (const layer of look.layers) acc = blend(acc, renderLayer(layer, t, progress, spots), layer.mode ?? "max");
  const k = (look.brightness ?? 1) * envelopeAt(look.envelope ?? "solid", t);
  return acc.map((c) => scale(c, k));
}

/** A named look's frame, or null when the look is not in the table. */
export function renderLook(name: string, t: number, progress = 0, spots: readonly SpotInput[] = []): Frame | null {
  if (!Object.prototype.hasOwnProperty.call(LOOKS, name)) return null;
  return renderSpec(LOOKS[name as LookName], t, progress, spots);
}

/** A rainbow that runs round the ring: the locate look, approximated (the device draws it as frames). */
export function rainbow(t: number): Frame {
  return Array.from({ length: LEDS }, (_, i) => hueOf(i / LEDS - t / 2400, 0.6));
}
