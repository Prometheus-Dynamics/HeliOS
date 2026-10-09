// Identities: a user-chosen name, icon and colour for any object (camera,
// pipeline, device, layout). The colour follows the object everywhere: list
// badges, graph headers, 3D frustums, overlays, chart lines, log tags.

import type { IconName } from "#lib/ui/icons.js";
import { loadRaw, save } from "./persist";

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

class IdentityStore {
  #overrides = $state<Record<string, Identity>>(loadRaw("identities", {}));

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
    this.#overrides[id] = { ...this.get(id), ...patch };
    save("identities", this.#overrides);
  }

  reset(id: string) {
    delete this.#overrides[id];
    save("identities", this.#overrides);
  }

  export(): Record<string, Identity> {
    return JSON.parse(JSON.stringify(this.#overrides));
  }

  import(all: Record<string, Identity>) {
    this.#overrides = all;
    save("identities", this.#overrides);
  }
}

export const identity = new IdentityStore();
