// Panes either follow the app selection ("the camera you clicked last") or
// are pinned to one object through their props.

import { selection, type SelectionKind } from "$lib/core/selection.svelte";
import { workspaces, type PaneRef } from "$lib/core/workspace.svelte";

/** `pane` and `ws` are getters so the helper always reads the current props. */
export function follow(paneOf: () => PaneRef, wsOf: () => string, kind: SelectionKind, fallback: () => string | undefined) {
  return {
    get id(): string | undefined {
      return (paneOf().props?.[kind] as string | undefined) ?? selection.last[kind]?.id ?? fallback();
    },
    get pinned(): boolean {
      return Boolean(paneOf().props?.[kind]);
    },
    pin(id: string | undefined) {
      workspaces.setProps(wsOf(), paneOf().id, { [kind]: id });
    },
    toggle() {
      this.pin(this.pinned ? undefined : this.id);
    },
  };
}
