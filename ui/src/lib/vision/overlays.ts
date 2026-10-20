import type { MenuItem } from "#lib/core/menu.svelte.js";

export interface Overlays {
  outlines: boolean;
  corners: boolean;
  ids: boolean;
  grid: boolean;
  crosshair: boolean;
  histogram: boolean;
  hud: boolean;
  /** Feature tracks of a `tracks` output: dots and trails. */
  features: boolean;
}

export const DEFAULT_OVERLAYS: Overlays = { outlines: true, corners: true, ids: true, grid: false, crosshair: false, histogram: true, hud: true, features: true };

const LABELS: [keyof Overlays, string][] = [
  ["outlines", "Tag outlines"],
  ["corners", "Corner dots (by index)"],
  ["ids", "Tag ids"],
  ["features", "Feature tracks"],
  ["hud", "Camera info"],
  ["histogram", "Histogram"],
  ["grid", "Rule-of-thirds grid"],
  ["crosshair", "Centre crosshair"],
];

export function overlayMenu(o: Overlays, set: (patch: Partial<Overlays>) => void): MenuItem[] {
  return [{ heading: "Overlays" }, ...LABELS.map(([k, label]) => ({ label, checked: o[k], run: () => set({ [k]: !o[k] }) }))];
}
