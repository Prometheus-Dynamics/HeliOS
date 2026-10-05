// One file with everything you set up in this UI: look, identities, layouts,
// camera mounts, field, calibrations and camera settings.

import { cluster } from "$lib/stores/cluster.svelte";
import { toasts } from "$lib/stores/toasts.svelte";
import { loadRaw, save } from "./persist";

const KEYS = ["prefs", "identities", "workspaces", "workspaces-modified", "placements", "field", "calibrations", "cal-board"];

export function exportAll() {
  return {
    format: "helios.config",
    schema_version: 1,
    exported_at: new Date().toISOString(),
    ui: Object.fromEntries(KEYS.map((k) => [k, loadRaw(k, null)])),
    cameras: cluster.cameras.map((c) => ({ id: c.resourceId, settings: c.settings, extra: c.extra ?? {} })),
  };
}

export function importAll(doc: { format?: string; ui?: Record<string, unknown>; cameras?: { id: string; settings: never; extra?: never }[] } | null) {
  if (doc?.format !== "helios.config" || !doc.ui) {
    toasts.error("That file is not a HeliOS config backup");
    return;
  }
  for (const k of KEYS) if (doc.ui[k] !== null && doc.ui[k] !== undefined) save(k, doc.ui[k]);
  for (const c of doc.cameras ?? []) cluster.setCamera(c.id, c.settings, c.extra);
  toasts.success("Config restored; reloading");
  setTimeout(() => location.reload(), 600);
}
