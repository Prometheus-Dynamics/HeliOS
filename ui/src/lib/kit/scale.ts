// Log-scale mapping for sliders over a span such as 9 µs .. 476 ms. Both bounds must be above zero.

const clamp01 = (t: number) => Math.min(1, Math.max(0, t));

/** A value's position on a log scale from min (0) to max (1). */
export function toLogFraction(value: number, min: number, max: number): number {
  if (!(min > 0) || !(max > min)) return 0;
  const v = Math.min(max, Math.max(min, value));
  return clamp01((Math.log(v) - Math.log(min)) / (Math.log(max) - Math.log(min)));
}

/** The value at a position (0 to 1) on a log scale from min to max. */
export function fromLogFraction(t: number, min: number, max: number): number {
  if (!(min > 0) || !(max > min)) return min;
  return Math.exp(Math.log(min) + clamp01(t) * (Math.log(max) - Math.log(min)));
}
