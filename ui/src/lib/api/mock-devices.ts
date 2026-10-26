// The simulated devices' capabilities (`GET /v1/device`) for `?mock=1` and `?mock=systemcore`, as
// the real devices report them. The Raze image is an owner with its board and all its hardware; a
// SystemCore is a guest with none of it (helios-api `capabilities.rs`).

import type { Capabilities, FeatureName } from "./types";
import type { MockDevice } from "./mode";

const ALL: FeatureName[] = ["updates", "slots", "reboot", "safe_mode", "recovery_ladder", "leds", "fan", "imu", "power", "usb_power", "security", "console"];

const GUEST_REASON = "the host owns the device on a guest; HeliOS installs as a package";

/** Each feature in `on` is on; the others are off, with the reason a guest gives. */
function features(on: FeatureName[]): { features: Capabilities["features"]; reasons: Capabilities["reasons"] } {
  const out = { features: {} as Capabilities["features"], reasons: {} as Capabilities["reasons"] };
  for (const name of ALL) {
    const enabled = on.includes(name);
    out.features[name] = enabled;
    if (!enabled) out.reasons[name] = GUEST_REASON;
  }
  return out;
}

export const MOCK_CAPABILITIES: Record<MockDevice, Capabilities> = {
  raze: {
    profile: "owner",
    host: { kind: "raze", name: "Helios v2026.4.0", release: "2026.4.0" },
    ...features(ALL),
    reasons: {},
  },
  systemcore: {
    profile: "guest",
    host: { kind: "systemcore", name: "limelightosr-2027.0.0-alpha18", release: "aef8f2eb" },
    ...features([]),
  },
};
