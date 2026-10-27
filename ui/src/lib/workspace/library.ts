// Menus for adding panes, shared by stack menus, the top bar and the palette.

import { menu, type MenuItem } from "#lib/core/menu.svelte.js";
import { pane, workspaces, type DropZone } from "#lib/core/workspace.svelte.js";
import { allPanes, JOB_NAMES, type Job, type PaneDef } from "./panes";

export const PANE_MIME = "application/x-helios-pane";
export const NEW_PANE_MIME = "application/x-helios-new-pane";

const JOBS: Job[] = ["setup", "run", "tune", "device"];

export function paneMenu(pick: (def: PaneDef) => void): MenuItem[] {
  const panes = allPanes().filter((p) => p.type !== "welcome");
  return JOBS.map((g) => ({
    label: JOB_NAMES[g],
    items: panes.filter((p) => p.group === g).map((p) => ({ label: p.title, icon: p.icon, hint: p.summary, run: () => pick(p) })),
  }));
}

export function addPaneTo(ws: string, stackId: string | null, zone: DropZone = "center") {
  return (def: PaneDef) => {
    if (!stackId || zone === "center") {
      if (stackId) workspaces.focusedStack = stackId;
      workspaces.open(ws, pane(def.type));
    } else workspaces.split(ws, stackId, pane(def.type), zone);
  };
}

export function showAddPane(el: Element, ws: string, stackId: string | null) {
  menu.below(el, paneMenu(addPaneTo(ws, stackId)));
}
