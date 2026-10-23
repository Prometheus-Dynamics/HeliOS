// The status ring's live frames, as the node's stream sends a light: samples of `offset`,
// `clockwise`, `brightness` and then `led.0`..`led.<n>`, each logical LED's colour as
// 0xWWRRGGBB, exactly as lemnosd wrote it. Logical LED 0 is the ring's top, physical LED
// `offset`; "clockwise" (Lemnos `cw`) means logical LEDs run with increasing physical (wire)
// indices: physical = (offset ± logical) mod n, as lemnos-board's gravity.rs maps them.

import type { LiveDevice } from "./feed.ts";

/** One frame in physical order, as 0xRRGGBB (white folded in). */
export interface RingFrame {
  colors: number[];
  /** The newest row's time, so a repaint happens only on change. */
  at: number;
  /** The ring's brightness (0 to 1), when the light reports it. */
  brightness: number | null;
}

/** The newest frame of a light, or null when it has none. */
export function ringFrame(device: LiveDevice, previous: RingFrame | null): RingFrame | null {
  if (!device.count) return null;
  const at = device.index(device.count - 1);
  const t = device.t[at];
  if (previous && previous.at === t) return previous;
  const names = device.channels.map((channel) => channel.name);
  // A light that doesn't report its offset is taken as starting at its top LED.
  const reported = device.v[names.indexOf("offset")]?.[at];
  const offset = reported !== undefined && Number.isFinite(reported) ? Math.round(reported) : 0;
  const reportedDirection = device.v[names.indexOf("clockwise")]?.[at];
  const clockwise = reportedDirection === undefined || Number.isNaN(reportedDirection) ? true : reportedDirection >= 0.5;
  const brightnessChannel = names.indexOf("brightness");
  const brightness = brightnessChannel >= 0 && Number.isFinite(device.v[brightnessChannel][at]) ? device.v[brightnessChannel][at] : null;
  const leds = names.flatMap((name, i) => (name.startsWith("led.") ? [i] : []));
  const n = leds.length;
  if (!n) return null;
  const colors = new Array<number>(n).fill(0);
  leds.forEach((channel, logical) => {
    const value = device.v[channel][at];
    if (!Number.isFinite(value)) return;
    const wrgb = value >>> 0;
    const w = (wrgb >>> 24) & 0xff;
    const add = (c: number) => Math.min(255, c + w);
    const rgb = (add((wrgb >> 16) & 0xff) << 16) | (add((wrgb >> 8) & 0xff) << 8) | add(wrgb & 0xff);
    const physical = (((clockwise ? offset + logical : offset - logical) % n) + n) % n;
    colors[physical] = rgb;
  });
  return { colors, at: t, brightness };
}
