// This device's capabilities: what it is (owner or guest, which host) and which features it has.
// Every device-specific control asks here (`device.has("leds")`), and the registries (panes,
// palette commands) take a `requires` feature. Loaded when the UI starts and again when the event
// stream reconnects; the live store applies the same answer when it reads the device.

import { api } from "#lib/api/client.js";
import { MOCK, MOCK_DEVICE } from "#lib/api/mode.js";
import { MOCK_CAPABILITIES } from "#lib/api/mock-devices.js";
import type { Capabilities, FeatureName } from "#lib/api/types.js";

class DeviceStore {
  /** Null until the device has answered: until then no device-specific control shows. */
  capabilities = $state<Capabilities | null>(MOCK ? MOCK_CAPABILITIES[MOCK_DEVICE] : null);

  /** Whether the device has the feature (a device that reports none has none). */
  has(name: FeatureName): boolean {
    return this.capabilities?.features?.[name] === true;
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
