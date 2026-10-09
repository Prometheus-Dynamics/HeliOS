// Commands: everything the palette (Ctrl+K) can do. Screens and panes register
// their own; the palette searches titles and keywords.

import type { IconName } from "#lib/ui/icons.js";

export interface Command {
  id: string;
  title: string;
  group: string;
  icon?: IconName;
  keywords?: string;
  shortcut?: string;
  run: () => void | Promise<void>;
}

class CommandStore {
  open = $state(false);
  #sources = $state<Record<string, () => Command[]>>({});

  register(source: string, provider: () => Command[]) {
    this.#sources[source] = provider;
    return () => delete this.#sources[source];
  }

  get all(): Command[] {
    return Object.values(this.#sources).flatMap((p) => p());
  }

  search(query: string): Command[] {
    const q = query.trim().toLowerCase();
    const all = this.all;
    if (!q) return all.slice(0, 40);
    const terms = q.split(/\s+/);
    return all
      .map((c) => {
        const hay = `${c.title} ${c.group} ${c.keywords ?? ""}`.toLowerCase();
        const score = terms.every((t) => hay.includes(t)) ? (c.title.toLowerCase().startsWith(terms[0]) ? 2 : 1) : 0;
        return { c, score };
      })
      .filter((x) => x.score > 0)
      .sort((a, b) => b.score - a.score)
      .slice(0, 40)
      .map((x) => x.c);
  }
}

export const commands = new CommandStore();
