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
  /** Each tag's pose axes, from `pose_solutions` and the camera's intrinsics. */
  poseAxes: boolean;
  /** The dense mapper's inverse depth, colour-mapped, blended over the image. */
  depth: boolean;
}

export const DEFAULT_OVERLAYS: Overlays = { outlines: true, corners: true, ids: true, grid: false, crosshair: false, histogram: true, hud: true, features: true, poseAxes: true, depth: false };

const LABELS: [keyof Overlays, string][] = [
  ["outlines", "Tag outlines"],
  ["corners", "Corner dots (by index)"],
  ["ids", "Tag ids"],
  ["poseAxes", "Tag pose axes"],
  ["features", "Feature tracks"],
  ["depth", "Depth preview"],
  ["hud", "Camera info"],
  ["histogram", "Histogram"],
  ["grid", "Rule-of-thirds grid"],
  ["crosshair", "Centre crosshair"],
];

export function overlayMenu(o: Overlays, set: (patch: Partial<Overlays>) => void): MenuItem[] {
  return [{ heading: "Overlays" }, ...LABELS.map(([k, label]) => ({ label, checked: o[k], run: () => set({ [k]: !o[k] }) }))];
}
