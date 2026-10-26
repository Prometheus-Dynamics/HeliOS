// The simulated devices' capabilities (`GET /v1/device`) for `?mock=1` and `?mock=systemcore`, as
// the real devices report them. The Raze image is an owner with its board and all its hardware; a
// SystemCore is a guest with none of it (helios-api `capabilities.rs`).

import type { Capabilities, FeatureName, PeerList } from "./types";
import type { MockDevice } from "./mode";

const ALL: FeatureName[] = ["updates", "slots", "reboot", "safe_mode", "recovery_ladder", "leds", "fan", "imu", "power", "usb_power", "security", "console"];

/** The node id of the SystemCore peer on the simulated robot. */
export const MOCK_SYSTEMCORE_PEER = "systemcore-peer";

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

const RAZE: Capabilities = {
  profile: "owner",
  host: { kind: "raze", name: "Helios v2026.4.0", release: "2026.4.0" },
  ...features(ALL),
  reasons: {},
};

const SYSTEMCORE: Capabilities = {
  profile: "guest",
  host: { kind: "systemcore", name: "limelightosr-2027.0.0-alpha18", release: "aef8f2eb" },
  ...features([]),
};

/** This device's capabilities, by simulated device. The robot is this device the Raze image. */
export const MOCK_CAPABILITIES: Record<MockDevice, Capabilities> = {
  raze: RAZE,
  systemcore: SYSTEMCORE,
  robot: RAZE,
};

/** The SystemCore peer's capabilities on the simulated robot, by its node id (`MOCK_PEERS`). */
export const MOCK_PEER_CAPABILITIES: Record<string, Capabilities> = { [MOCK_SYSTEMCORE_PEER]: SYSTEMCORE };

/** What `GET /v1/peers` lists from the Raze's side of the simulated robot: the SystemCore, enrolled and routed. */
export const MOCK_PEERS: PeerList = {
  cluster: "robot-1234",
  peers: [{ node_id: MOCK_SYSTEMCORE_PEER, state: "enrolled", address: "172.27.0.1", api_port: 5800, routed: true }],
};
