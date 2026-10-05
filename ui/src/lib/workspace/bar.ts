// Panes put their toolbar in their stack's tab row, so a pane spends no
// height on its own header. The stack provides a target element; <PaneBar>
// moves its children there while its pane is the active tab.

import { getContext, setContext } from "svelte";

export interface BarSlot {
  readonly target: HTMLElement | null;
  readonly active: boolean;
}

const KEY = Symbol("pane-bar");

export function setBarSlot(slot: BarSlot) {
  setContext(KEY, slot);
}

export function getBarSlot(): BarSlot | undefined {
  return getContext<BarSlot | undefined>(KEY);
}

/** Moves an element into `target` (and back out on destroy). */
export function portal(node: HTMLElement, target: HTMLElement | null) {
  const move = (t: HTMLElement | null) => {
    if (t) t.appendChild(node);
    else node.remove();
  };
  move(target);
  return {
    update: move,
    destroy: () => node.remove(),
  };
}
