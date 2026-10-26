// Identities: a user-chosen name, icon and colour for any object (camera,
// pipeline, device, layout). The colour follows the object everywhere: list
// badges, graph headers, 3D frustums, overlays, chart lines, log tags.

import { api, errorText } from "#lib/api/client.js";
import type { StoredIdentity } from "#lib/api/types.js";
import { onSettingsChange } from "#lib/core/sync.js";
import type { IconName } from "#lib/ui/icons.js";
import { toasts } from "#lib/stores/toasts.svelte.js";
import { forget, loadRaw } from "./persist";

export interface Identity {
  name?: string;
  icon?: IconName;
  /** 1..12, an index into the theme's --id-N palette. */
  color: number;
}

export const ID_COLORS = Array.from({ length: 12 }, (_, i) => i + 1);
export const colorVar = (n: number) => `var(--id-${((n - 1) % 12) + 1})`;

/** Stable default colour for an id, so unnamed objects still differ. */
function hashColor(id: string): number {
  let h = 0;
  for (const ch of id) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return (h % 12) + 1;
}

const DEFAULTS: Record<string, Identity> = {
  "camera.raze-front.cam0": { color: 8, icon: "camera" },
  "camera.raze-back.cam0": { color: 2, icon: "camera" },
  "camera.raze-left.cam0": { color: 5, icon: "camera" },
  "camera.raze-right.cam0": { color: 10, icon: "camera" },
  "camera.pv-coproc.arducam": { color: 3, icon: "aperture" },
  "camera.driver-laptop.usb0": { color: 12, icon: "device-laptop" },
  "tags-front": { color: 8, icon: "target" },
  "tags-back": { color: 2, icon: "target" },
  "tags-right": { color: 10, icon: "target" },
  "tags-left": { color: 5, icon: "box" },
  "tags-left-exp": { color: 11, icon: "flask" },
};

/** The identities live on the device (`/v1/device/identities`); every open tab follows them. */
class IdentityStore {
  #overrides = $state<Record<string, Identity>>({});
  #started = false;

  /** Load the device's identities and follow their changes (once). */
  start() {
    if (this.#started) return;
    this.#started = true;
    onSettingsChange("identities", () => void this.load());
    void this.#migrate().then(() => this.load());
  }

  async load() {
    try {
      this.#overrides = (await api.identities()) as Record<string, Identity>;
    } catch {
      // The device is not answering: the defaults stand until it does.
    }
  }

  /** Identities kept in this browser before the device kept them move to the device, once. */
  async #migrate() {
    const legacy = loadRaw<Record<string, Identity> | null>("identities", null);
    if (!legacy) return;
    try {
      for (const [id, identity] of Object.entries(legacy)) await api.putIdentity(id, identity as StoredIdentity);
      forget("identities");
    } catch {
      // Kept in the browser until the device answers.
    }
  }

  get(id: string): Identity {
    return this.#overrides[id] ?? DEFAULTS[id] ?? { color: hashColor(id) };
  }

  color(id: string): string {
    return colorVar(this.get(id).color);
  }

  name(id: string, fallback: string): string {
    return this.get(id).name || fallback;
  }

  set(id: string, patch: Partial<Identity>) {
    const next = { ...this.get(id), ...patch };
    this.#overrides[id] = next;
    void this.#send(() => api.putIdentity(id, next as StoredIdentity));
  }

  reset(id: string) {
    delete this.#overrides[id];
    void this.#send(() => api.deleteIdentity(id));
  }

  export(): Record<string, Identity> {
    return JSON.parse(JSON.stringify(this.#overrides));
  }

  /** Replace every identity with `all` (an imported file). */
  import(all: Record<string, Identity>) {
    const removed = Object.keys(this.#overrides).filter((id) => !(id in all));
    this.#overrides = all;
    void this.#send(async () => {
      for (const [id, identity] of Object.entries(all)) await api.putIdentity(id, identity as StoredIdentity);
      for (const id of removed) await api.deleteIdentity(id);
    });
  }

  async #send(run: () => Promise<unknown>) {
    try {
      await run();
    } catch (error) {
      toasts.error(`Identity not saved: ${errorText(error)}`);
    }
  }
}

export const identity = new IdentityStore();
