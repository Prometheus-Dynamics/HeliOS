// One menu host for the whole app: dropdowns and right-click menus both call
// `menu.at(...)`. Items can nest one level (submenus) and carry a check mark.

import type { IconName } from "#lib/ui/icons.js";

export type MenuItem =
  | {
      label: string;
      icon?: IconName;
      /** Identity colour dot (CSS colour). */
      color?: string;
      shortcut?: string;
      checked?: boolean;
      danger?: boolean;
      disabled?: boolean;
      hint?: string;
      run?: () => void;
      items?: MenuItem[];
    }
  | { separator: true }
  | { heading: string };

class MenuStore {
  current = $state<{ x: number; y: number; items: MenuItem[]; minWidth: number } | null>(null);

  /** Opens at a point (right-click). */
  at(x: number, y: number, items: MenuItem[], minWidth = 180) {
    this.current = { x, y, items, minWidth };
  }

  /** Opens under an element (dropdown button). */
  below(el: Element, items: MenuItem[], align: "start" | "end" = "start") {
    const r = el.getBoundingClientRect();
    this.current = { x: align === "start" ? r.left : r.right, y: r.bottom + 3, items, minWidth: Math.max(180, r.width) };
    if (align === "end") this.current.x = -r.right; // negative: anchor right edge
  }

  /** Right-click handler factory. */
  context(items: () => MenuItem[]) {
    return (event: MouseEvent) => {
      event.preventDefault();
      event.stopPropagation();
      this.at(event.clientX, event.clientY, items());
    };
  }

  close() {
    this.current = null;
  }
}

export const menu = new MenuStore();
