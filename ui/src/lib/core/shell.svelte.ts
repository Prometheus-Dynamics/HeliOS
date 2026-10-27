// Which workspace is on screen. Kept in the URL hash too, so a link or a
// reload lands on the same screen.

import { loadRaw, save } from "./persist";
import { pane, workspaces } from "./workspace.svelte";

class ShellStore {
  /** The top bar's name field is open. */
  renaming = $state(false);
  #active = $state<string>(loadRaw("active-workspace", "run"));

  get active(): string {
    return workspaces.get(this.#active) ? this.#active : (workspaces.order[0] ?? "run");
  }

  go(id: string) {
    if (!workspaces.get(id)) return;
    this.#active = id;
    workspaces.maximized = null;
    workspaces.focusedStack = null;
    save("active-workspace", id);
    if (typeof location !== "undefined" && location.hash !== `#${id}`) history.replaceState(null, "", `#${id}`);
  }

  /** Shows a screen with one of its panes in front (opened there if the layout lost it). */
  open(id: string, type: string, props?: Record<string, unknown>) {
    this.go(id);
    workspaces.open(id, pane(type, props));
  }

  fromHash() {
    const id = location.hash.slice(1);
    if (id && workspaces.get(id)) this.go(id);
  }
}

export const shell = new ShellStore();
