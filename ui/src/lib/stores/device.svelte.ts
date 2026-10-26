// This device's capabilities: what it is (owner or guest, which host) and which features it has.
// Every device-specific control asks here (`device.has("leds")`), and the registries (panes,
// palette commands) take a `requires` feature. Loaded when the UI starts and again when the event
// stream reconnects; the live store applies the same answer when it reads the device. A peer on
// the same robot has its own answer (`hasOn(nodeId, ...)`): a control drawn for a peer asks there.

import { api } from "#lib/api/client.js";
import { MOCK, MOCK_DEVICE } from "#lib/api/mode.js";
import { MOCK_CAPABILITIES, MOCK_PEER_CAPABILITIES } from "#lib/api/mock-devices.js";
import type { Capabilities, FeatureName } from "#lib/api/types.js";

class DeviceStore {
  /** Null until the device has answered: until then no device-specific control shows. */
  capabilities = $state<Capabilities | null>(MOCK ? MOCK_CAPABILITIES[MOCK_DEVICE] : null);
  /** The capabilities of each routed peer on the robot, by node id (empty until they are read). */
  peers = $state<Record<string, Capabilities>>(MOCK && MOCK_DEVICE === "robot" ? MOCK_PEER_CAPABILITIES : {});

  /** Whether this device has the feature (a device that reports none has none). */
  has(name: FeatureName): boolean {
    return this.capabilities?.features?.[name] === true;
  }

  /** Whether the device `nodeId` has the feature: a routed peer's own answer, else this device's. */
  hasOn(nodeId: string | undefined, name: FeatureName): boolean {
    const peer = nodeId === undefined ? undefined : this.peers[nodeId];
    return peer ? peer.features?.[name] === true : this.has(name);
  }

  /** Why the feature is off here, when it is. */
  reason(name: FeatureName): string | null {
    return this.capabilities?.reasons[name] ?? null;
  }

  get owner(): boolean {
    return this.capabilities?.profile === "owner";
  }

  apply(capabilities: Capabilities) {
    this.capabilities = capabilities;
  }

  /** A peer's capabilities as its own device answered them (live mode). */
  applyPeer(nodeId: string, capabilities: Capabilities) {
    this.peers = { ...this.peers, [nodeId]: capabilities };
  }

  /** Forgets the peers that are no longer on the robot. */
  keepPeers(nodeIds: string[]) {
    const keep = new Set(nodeIds);
    const kept = Object.fromEntries(Object.entries(this.peers).filter(([id]) => keep.has(id)));
    if (Object.keys(kept).length !== Object.keys(this.peers).length) this.peers = kept;
  }

  /** Reads the device's capabilities (a no-op on the simulated robot, which starts with its own). */
  async load() {
    if (MOCK) return;
    try {
      this.apply(await api.device());
    } catch {
      // Keep what we have; the next load (or the event stream's reconnect) asks again.
    }
  }
}

export const device = new DeviceStore();
