// First-time setup, as the device keeps it: done once the user finishes or skips the wizard, so
// it is not shown again on any tab or after a reboot.

import { api, errorText } from "#lib/api/client.js";
import { toasts } from "#lib/stores/toasts.svelte.js";
import { forget, loadRaw } from "./persist";

class SetupStore {
  /** Done. Starts as done, so the wizard does not flash before the device answers. */
  onboarded = $state(true);

  /** Read the device's answer; returns whether setup is done. */
  async load(): Promise<boolean> {
    try {
      const record = await api.setup();
      // Setup finished in this browser before the device kept it: the device learns it once.
      if (!record.onboarded && loadRaw("onboarded", false)) {
        await api.setSetup({ onboarded: true });
        forget("onboarded");
        record.onboarded = true;
      }
      this.onboarded = record.onboarded;
    } catch {
      // The device is not answering: the wizard stays closed until it does.
    }
    return this.onboarded;
  }

  async complete() {
    this.onboarded = true;
    try {
      await api.setSetup({ onboarded: true });
    } catch (error) {
      toasts.error(`Setup not saved: ${errorText(error)}`);
    }
  }
}

export const setup = new SetupStore();
