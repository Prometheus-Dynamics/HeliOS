// Colours for the LED maths. A colour is three channels, 0 to 1, exactly as the device writes
// them (no gamma), so the preview blends the same numbers the ring does.

export type Rgb = [number, number, number];

export const BLACK: Rgb = [0, 0, 0];
export const WHITE: Rgb = [1, 1, 1];

export function clamp01(value: number): number {
  return value < 0 ? 0 : value > 1 ? 1 : value;
}

/** "rrggbb" or "#rrggbb" to channels 0 to 1. */
export function rgbOf(hex: string): Rgb {
  const digits = hex.replace(/^#/, "");
  if (!/^[0-9a-fA-F]{6}$/.test(digits)) throw new Error(`not a colour: ${hex}`);
  return [0, 2, 4].map((at) => parseInt(digits.slice(at, at + 2), 16) / 255) as Rgb;
}

/** Channels 0 to 1 to "#rrggbb" for the page. */
export function hexOf(colour: Rgb): string {
  return `#${colour.map((c) => Math.round(clamp01(c) * 255).toString(16).padStart(2, "0")).join("")}`;
}

export function scale(colour: Rgb, k: number): Rgb {
  return [colour[0] * k, colour[1] * k, colour[2] * k];
}

/** `a` at 0 and `b` at 1, per channel. */
export function mix(a: Rgb, b: Rgb, t: number): Rgb {
  return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
}

/** A hue (0 to 1, wrapping) at full saturation, scaled by `value`. */
export function hueOf(hue: number, value: number): Rgb {
  const h = (((hue % 1) + 1) % 1) * 6;
  const x = 1 - Math.abs((h % 2) - 1);
  const sector = [
    [1, x, 0],
    [x, 1, 0],
    [0, 1, x],
    [0, x, 1],
    [x, 0, 1],
    [1, 0, x],
  ][Math.floor(h)] as Rgb;
  return scale(sector, value);
}
