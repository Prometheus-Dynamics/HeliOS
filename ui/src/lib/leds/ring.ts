// Geometry and easing for the 16-LED ring. Logical LED 0 is at the top and the index grows
// clockwise, as the preview draws it.

export const LEDS = 16;

/** `a` modulo `n`, always in [0, n). */
export function mod(a: number, n: number): number {
  return ((a % n) + n) % n;
}

/** The shorter way round the ring between two (fractional) positions: 0 to 8. */
export function ringDistance(a: number, b: number): number {
  const d = mod(a - b, LEDS);
  return Math.min(d, LEDS - d);
}

/** Clockwise angle from the top, in radians, of a (fractional) LED position. */
export function angleOf(position: number): number {
  return (mod(position, LEDS) / LEDS) * Math.PI * 2;
}

/**
 * Move `current` toward `target` for a frame that took `dtMs`: it closes about 63% of the gap
 * every `tauMs`, whatever the frame rate, so changes between device events ease in.
 */
export function approach(current: number, target: number, dtMs: number, tauMs: number): number {
  if (tauMs <= 0) return target;
  return target + (current - target) * Math.exp(-Math.max(dtMs, 0) / tauMs);
}
