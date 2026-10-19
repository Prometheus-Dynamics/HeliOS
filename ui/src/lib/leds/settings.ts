// The LED settings' limits, checked before a save. The device checks them too (HTTP 422); this
// only saves a round trip and says which field is wrong.

import type { LedSettings } from "#lib/api/leds.js";

export const LED_LIMITS = {
  brightness: { min: 0, max: 1, step: 0.05 },
  ambient: { min: 0, max: 1, step: 0.05 },
  forward_led: { min: 0, max: 15, step: 1 },
  fov_deg: { min: 20, max: 180, step: 1 },
} as const;

/** The first problem with `settings`, in words, or null when it can be saved. */
export function validateLeds(settings: LedSettings): string | null {
  const inRange = (value: number, { min, max }: { min: number; max: number }) => Number.isFinite(value) && value >= min && value <= max;
  if (!inRange(settings.brightness, LED_LIMITS.brightness)) return "Brightness is between 0 and 1.";
  if (!inRange(settings.ambient.min, LED_LIMITS.ambient) || !inRange(settings.ambient.max, LED_LIMITS.ambient)) return "Ambient limits are between 0 and 1.";
  if (settings.ambient.min > settings.ambient.max) return "Ambient minimum cannot be above the maximum.";
  if (!Number.isInteger(settings.forward_led) || !inRange(settings.forward_led, LED_LIMITS.forward_led)) return "Forward LED is a whole number from 0 to 15.";
  if (settings.direction !== "cw" && settings.direction !== "ccw") return "Direction is clockwise or counter-clockwise.";
  if (!Number.isInteger(settings.fov_deg) || !inRange(settings.fov_deg, LED_LIMITS.fov_deg)) return "Field of view is a whole number of degrees from 20 to 180.";
  return null;
}
