// Small flows around workspaces: create, rename, import/export layouts.

import { colorVar, ID_COLORS } from "$lib/core/identity.svelte";
import { menu } from "$lib/core/menu.svelte";
import { shell } from "$lib/core/shell.svelte";
import { workspaces, type Workspace } from "$lib/core/workspace.svelte";
import type { IconName } from "$lib/ui/icons";
import { downloadJson, pickJson } from "$lib/files";
import { toasts } from "$lib/stores/toasts.svelte";

export const WS_ICONS: IconName[] = ["layout-grid", "camera", "schema", "cube", "cpu", "target", "radar-2", "robot", "flask", "tool", "gauge", "terminal-2", "bulb", "world-www"];

export function newWorkspace(anchor: Element) {
  menu.below(anchor, [
    { heading: "New workspace" },
    { label: "Empty", icon: "file-plus", run: () => create("New workspace") },
    { label: `Copy of “${workspaces.get(shell.active)?.name}”`, icon: "copy", run: () => { const w = workspaces.get(shell.active)!; shell.go(workspaces.saveAs(w.id, `${w.name} copy`, w.icon, w.color)); } },
    { label: "Import layout…", icon: "upload", run: importWorkspace },
  ]);
}

function create(name: string) {
  const color = ID_COLORS[(workspaces.custom.length * 5 + 3) % 12];
  shell.go(workspaces.create(name, "layout-grid", color));
}

/** Shows the workspace and opens the inline name editor in the top bar. */
export function renameWorkspace(id: string) {
  shell.go(id);
  shell.renaming = true;
}

export function exportWorkspace(id: string) {
  const w = workspaces.get(id);
  if (!w) return;
  downloadJson(`${w.name.toLowerCase().replace(/\W+/g, "-")}.helios-layout.json`, { format: "helios.layout", schema_version: 1, workspace: w });
}

export async function importWorkspace() {
  const doc = await pickJson<{ format?: string; workspace?: Workspace }>();
  if (!doc) return;
  if (doc.format !== "helios.layout" || !doc.workspace?.root) {
    toasts.error("That file is not a HeliOS layout");
    return;
  }
  const w = doc.workspace;
  shell.go(workspaces.create(w.name, w.icon ?? "layout-grid", w.color ?? 8, w.root));
  toasts.success(`Imported “${w.name}”`);
}

export { colorVar };
