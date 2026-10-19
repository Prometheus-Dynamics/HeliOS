// The LED ring's API (helios-api `/v1/device/leds`): its settings, what the ring is showing,
// and the event that mirrors it (`leds` on the event stream).

/** The states the ring can show, in the order the picker lists them. */
export const LED_STATES = [
  "locate",
  "error",
  "calibrating",
  "robot-wait",
  "robot-wait-tags",
  "tags",
  "tags-locked",
  "searching",
  "robot-enabled",
  "starting",
  "recording",
  "idle",
] as const;

export type LedState = (typeof LED_STATES)[number];
export type LedDirection = "cw" | "ccw";

export interface LedSettings {
  enabled: boolean;
  /** Ring-wide brightness, 0 to 1. */
  brightness: number;
  ambient: { enabled: boolean; min: number; max: number };
  /** The logical LED that faces the camera's centre; 0 is the ring's top. */
  forward_led: number;
  direction: LedDirection;
  /** Horizontal field of view used when the camera has no calibration, in degrees. */
  fov_deg: number;
  tally: boolean;
  boost: boolean;
}

export interface LedLive {
  state: LedState;
  look: string | null;
  brightness: number;
  tags: number;
  locked: boolean;
  recording: boolean;
  ambient_lux: number | null;
  ring_leds: number;
}

export interface LedStateInfo {
  name: LedState;
  label: string;
  look: string | null;
}

export interface LedsResponse {
  settings: LedSettings;
  live: LedLive;
  device: { reachable: boolean; error: string | null };
  preview: { state: LedState; until_ms: number } | null;
  locate_until_ms: number | null;
  states: LedStateInfo[];
}

/** The `data` of a `leds` event, throttled to about 10 Hz. */
export interface LedEvent {
  state: LedState;
  look: string | null;
  brightness: number;
  tags: number;
  locked: boolean;
  recording: boolean;
  /** Sixteen "rrggbb" colours the device is showing, or null when it runs a named look. */
  leds: string[] | null;
  spots: { at: number; level: number }[];
  /** Position of the sheen (0 to 16), or null. */
  sheen: number | null;
}
