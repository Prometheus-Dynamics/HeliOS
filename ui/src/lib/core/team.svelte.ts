// The team this robot belongs to, and where robot code (NetworkTables) is. Kept on the device:
// the team the user sets there wins; without one, the robot controller's team is used, if HeliOS
// found a controller (`/v1/device/nt4` says which, and where each fact came from).

import { api, errorText } from "#lib/api/client.js";
import type { FactSource, Nt4Settings, Nt4State } from "#lib/api/types.js";
import { DEFAULT_NT4 } from "#lib/core/robot-facts.js";
import { onSettingsChange } from "#lib/core/sync.js";
import { toasts } from "#lib/stores/toasts.svelte.js";

class TeamStore {
  /** What the device resolves the robot's NetworkTables settings to (`null` until it answers). */
  robot = $state<Nt4State | null>(null);
  #started = false;

  /** Load the device's robot state and follow its changes (once). */
  start() {
    if (this.#started) return;
    this.#started = true;
    onSettingsChange("nt4", () => void this.load());
    onSettingsChange("robot", () => void this.load());
    void this.load();
  }

  async load() {
    try {
      this.robot = await api.nt4();
    } catch {
      // The device is not answering: what is shown stays until it does.
    }
  }

  get number(): number | null {
    return this.robot?.effective.team.value ?? null;
  }

  /** Where the team comes from: the user's setting, the robot controller, or nothing. */
  get source(): FactSource {
    return this.robot?.effective.team.source ?? "none";
  }

  get controllerName(): string | null {
    return this.robot?.controller?.name ?? null;
  }

  /** The address robot code is at (`—` when nothing is known). */
  get ntServer(): string {
    return this.robot?.effective.nt4_server.value ?? "—";
  }

  /** Set the team on the device (`null` gives the choice back to the robot controller). */
  set number(n: number | null) {
    void this.save({ team: n });
  }

  /** Change NetworkTables settings on the device; the change is shown once the device has it. */
  async save(change: Partial<Nt4Settings>): Promise<Nt4State | undefined> {
    const settings = { ...(this.robot?.settings ?? DEFAULT_NT4), ...change };
    try {
      this.robot = await api.setNt4(settings);
      return this.robot;
    } catch (error) {
      toasts.error(`NetworkTables: ${errorText(error)}`);
    }
  }
}

export const team = new TeamStore();
