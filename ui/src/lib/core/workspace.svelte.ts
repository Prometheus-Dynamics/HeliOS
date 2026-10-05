// Workspaces: every screen is a layout of panes (splits and tab stacks). The
// built-in screens are presets; rearranging one saves your version of it, and
// "Save as" keeps it as a new workspace. Screens and workspaces are the same
// thing.

import type { IconName } from "$lib/ui/icons";
import { loadRaw, save } from "./persist";

export interface PaneRef {
  id: string;
  type: string;
  props?: Record<string, unknown>;
}

export type LayoutNode =
  | { kind: "split"; id: string; dir: "row" | "col"; sizes: number[]; children: LayoutNode[] }
  | { kind: "stack"; id: string; tabs: PaneRef[]; active: number };

export interface Workspace {
  id: string;
  name: string;
  icon: IconName;
  color: number;
  root: LayoutNode;
  builtin?: boolean;
  /** One line on what this screen is for (gallery, tooltips, tour). */
  summary?: string;
}

export type DropZone = "center" | "left" | "right" | "top" | "bottom";

let seq = 0;
export const uid = (prefix: string) => `${prefix}-${Date.now().toString(36)}-${(seq++).toString(36)}`;

export const pane = (type: string, props?: Record<string, unknown>): PaneRef => ({ id: uid("p"), type, props });
export const stack = (...tabs: PaneRef[]): LayoutNode => ({ kind: "stack", id: uid("s"), tabs, active: 0 });
export const row = (sizes: number[], ...children: LayoutNode[]): LayoutNode => ({ kind: "split", id: uid("r"), dir: "row", sizes, children });
export const col = (sizes: number[], ...children: LayoutNode[]): LayoutNode => ({ kind: "split", id: uid("c"), dir: "col", sizes, children });

function clone<T>(v: T): T {
  return JSON.parse(JSON.stringify(v));
}

/** Finds a stack and its parent split by id. */
function findStack(node: LayoutNode, id: string): Extract<LayoutNode, { kind: "stack" }> | null {
  if (node.kind === "stack") return node.id === id ? node : null;
  for (const child of node.children) {
    const hit = findStack(child, id);
    if (hit) return hit;
  }
  return null;
}

function firstStack(node: LayoutNode): Extract<LayoutNode, { kind: "stack" }> {
  return node.kind === "stack" ? node : firstStack(node.children[0]);
}

/** Removes empty stacks and single-child splits, keeping sizes summing to 1. */
function normalize(node: LayoutNode): LayoutNode | null {
  if (node.kind === "stack") {
    if (!node.tabs.length) return null;
    node.active = Math.min(node.active, node.tabs.length - 1);
    return node;
  }
  const kept: LayoutNode[] = [];
  const sizes: number[] = [];
  node.children.forEach((child, i) => {
    const n = normalize(child);
    if (n) {
      // Flatten a nested split in the same direction.
      if (n.kind === "split" && n.dir === node.dir) {
        const total = node.sizes[i];
        n.children.forEach((c, j) => {
          kept.push(c);
          sizes.push(total * n.sizes[j]);
        });
      } else {
        kept.push(n);
        sizes.push(node.sizes[i]);
      }
    }
  });
  if (!kept.length) return null;
  if (kept.length === 1) return kept[0];
  const sum = sizes.reduce((a, b) => a + b, 0) || 1;
  node.children = kept;
  node.sizes = sizes.map((s) => s / sum);
  return node;
}

/** Replaces the node with `id` by the result of `f` (which may wrap it). */
function replace(node: LayoutNode, id: string, f: (n: LayoutNode) => LayoutNode): LayoutNode {
  if (node.id === id) return f(node);
  if (node.kind === "split") node.children = node.children.map((c) => replace(c, id, f));
  return node;
}

class WorkspaceStore {
  /** User copies of built-in screens and user-made workspaces, by id. */
  #saved = $state<Record<string, Workspace>>(loadRaw("workspaces", {}));
  #presets: Record<string, () => Workspace> = {};
  /** Which stack receives "open pane" commands. */
  focusedStack = $state<string | null>(null);
  dragging = $state<{ from: string; index: number } | null>(null);

  /** Screen order in the rail: presets first (registration order), then yours. */
  order = $state<string[]>([]);
  /** A stack shown alone, filling the workspace (transient). */
  maximized = $state<string | null>(null);
  #modified = $state<string[]>(loadRaw<string[]>("workspaces-modified", []));

  registerPreset(id: string, make: () => Workspace) {
    this.#presets[id] = make;
    // Untouched presets follow the code; your edited copy is kept.
    if (!this.#saved[id] || !this.#modified.includes(id)) this.#saved[id] = { ...make(), builtin: true };
    if (!this.order.includes(id)) this.order.push(id);
  }

  get presets(): string[] {
    return Object.keys(this.#presets);
  }

  get custom(): Workspace[] {
    return Object.values(this.#saved).filter((w) => !this.#presets[w.id]);
  }

  /** The current layout for a screen: your saved version, else the preset. */
  get(id: string): Workspace | null {
    return this.#saved[id] ?? null;
  }

  /** Every screen, presets first. */
  get all(): Workspace[] {
    return [...this.order.map((id) => this.#saved[id]).filter(Boolean), ...this.custom];
  }

  /** Built-in screens shown in the rail; null shows them all. Yours always show. */
  #visible = $state<string[] | null>(loadRaw<string[] | null>("visible-screens", null));

  isVisible(id: string): boolean {
    return !this.#presets[id] || this.#visible === null || this.#visible.includes(id);
  }

  /** The screens in the rail, in order. */
  get shown(): Workspace[] {
    return this.all.filter((w) => this.isVisible(w.id));
  }

  /** Built-in screens not in the rail (offered in the gallery). */
  get hidden(): Workspace[] {
    return this.order.filter((id) => !this.isVisible(id)).map((id) => this.#saved[id]).filter(Boolean);
  }

  setVisible(ids: string[] | null) {
    this.#visible = ids;
    save("visible-screens", ids);
  }

  show(id: string) {
    if (this.#visible && !this.#visible.includes(id)) this.setVisible([...this.#visible, id]);
  }

  hide(id: string) {
    const base = this.#visible ?? [...this.order];
    this.setVisible(base.filter((x) => x !== id));
  }

  /** Puts a whole layout beside the current one (a kit of panes). */
  attach(wsId: string, node: LayoutNode, side: "right" | "bottom" = "right", size = 0.38) {
    const ws = this.get(wsId);
    if (!ws) return;
    const dir = side === "right" ? "row" : "col";
    ws.root = normalize({ kind: "split", id: uid(dir === "row" ? "r" : "c"), dir, sizes: [1 - size, size], children: [ws.root, node] }) ?? node;
    this.#touch(wsId);
  }

  isModified(id: string): boolean {
    return Boolean(this.#presets[id] && this.#modified.includes(id));
  }

  #touch(id: string) {
    if (!this.#modified.includes(id)) this.#modified.push(id);
    save("workspaces", this.#saved);
    save("workspaces-modified", this.#modified);
  }

  reset(id: string) {
    const make = this.#presets[id];
    if (!make) return;
    this.#saved[id] = { ...make(), builtin: true };
    this.#modified = this.#modified.filter((m) => m !== id);
    this.maximized = null;
    save("workspaces", this.#saved);
    save("workspaces-modified", this.#modified);
  }

  saveAs(id: string, name: string, icon: IconName, color: number): string {
    const source = this.get(id);
    const nextId = uid("ws");
    if (source) this.#saved[nextId] = { ...clone(source), id: nextId, name, icon, color, builtin: false };
    save("workspaces", this.#saved);
    return nextId;
  }

  /** A new workspace from scratch, or from a layout (imported, or a preset). */
  create(name: string, icon: IconName, color: number, root?: LayoutNode): string {
    const id = uid("ws");
    this.#saved[id] = { id, name, icon, color, root: root ? clone(root) : stack(pane("welcome")), builtin: false };
    save("workspaces", this.#saved);
    return id;
  }

  rename(id: string, patch: Partial<Pick<Workspace, "name" | "icon" | "color">>) {
    const ws = this.#saved[id];
    if (!ws) return;
    Object.assign(ws, patch);
    if (this.#presets[id]) this.#touch(id);
    else save("workspaces", this.#saved);
  }

  remove(id: string) {
    if (this.#presets[id]) return;
    delete this.#saved[id];
    save("workspaces", this.#saved);
  }

  /** Splits a stack, putting a new pane beside it. */
  split(wsId: string, stackId: string, ref: PaneRef, zone: Exclude<DropZone, "center">) {
    const ws = this.get(wsId);
    if (!ws) return;
    const dir = zone === "left" || zone === "right" ? "row" : "col";
    const before = zone === "left" || zone === "top";
    const fresh = stack(ref);
    ws.root = replace(ws.root, stackId, (n) => ({ kind: "split", id: uid(dir === "row" ? "r" : "c"), dir, sizes: [0.5, 0.5], children: before ? [fresh, n] : [n, fresh] }));
    ws.root = normalize(ws.root) ?? stack(pane("welcome"));
    this.focusedStack = fresh.id;
    this.#touch(wsId);
  }

  /** Persists after a live resize (the drag mutates sizes directly). */
  commitSizes(wsId: string) {
    this.#touch(wsId);
  }

  setSizes(wsId: string, splitId: string, sizes: number[]) {
    const ws = this.get(wsId);
    if (!ws) return;
    const visit = (n: LayoutNode) => {
      if (n.kind !== "split") return;
      if (n.id === splitId) n.sizes = sizes;
      else n.children.forEach(visit);
    };
    visit(ws.root);
    this.#touch(wsId);
  }

  activate(wsId: string, stackId: string, index: number) {
    const s = this.get(wsId) && findStack(this.get(wsId)!.root, stackId);
    if (!s) return;
    s.active = index;
    this.focusedStack = stackId;
    save("workspaces", this.#saved);
  }

  close(wsId: string, stackId: string, index: number) {
    const ws = this.get(wsId);
    const s = ws && findStack(ws.root, stackId);
    if (!ws || !s) return;
    const [gone] = s.tabs.splice(index, 1);
    if (gone && gone.type !== "welcome") {
      const list = (this.closed[wsId] ??= []);
      list.push({ ref: gone, stackId });
      if (list.length > 12) list.shift();
    }
    if (!s.tabs.length && this.maximized === s.id) this.maximized = null;
    ws.root = normalize(ws.root) ?? stack(pane("welcome"));
    this.#touch(wsId);
  }

  /** Panes closed in each workspace this session, newest last, for reopening. */
  closed = $state<Record<string, { ref: PaneRef; stackId: string }[]>>({});

  /** Brings back a closed pane (the newest, or a given one) where it was. */
  reopen(wsId: string, paneId?: string) {
    const list = this.closed[wsId];
    const ws = this.get(wsId);
    if (!list?.length || !ws) return;
    const i = paneId ? list.findIndex((c) => c.ref.id === paneId) : list.length - 1;
    if (i < 0) return;
    const [{ ref, stackId }] = list.splice(i, 1);
    const home = findStack(ws.root, stackId);
    if (home) {
      home.tabs.push(ref);
      home.active = home.tabs.length - 1;
      this.#touch(wsId);
    } else this.open(wsId, ref);
  }

  /** Adds a pane to the focused stack (or the first one). */
  open(wsId: string, ref: PaneRef) {
    const ws = this.get(wsId);
    if (!ws) return;
    const target = (this.focusedStack && findStack(ws.root, this.focusedStack)) || firstStack(ws.root);
    const existing = target.tabs.findIndex((t) => t.type === ref.type && JSON.stringify(t.props ?? {}) === JSON.stringify(ref.props ?? {}));
    if (existing >= 0) target.active = existing;
    else {
      target.tabs.push(ref);
      target.active = target.tabs.length - 1;
    }
    this.#touch(wsId);
  }

  /** Moves a tab onto another stack, or splits beside it. */
  move(wsId: string, fromStack: string, index: number, toStack: string, zone: DropZone) {
    const ws = this.get(wsId);
    if (!ws) return;
    const from = findStack(ws.root, fromStack);
    const to = findStack(ws.root, toStack);
    if (!from || !to) return;
    if (from === to && (zone === "center" || from.tabs.length === 1)) return;
    const [tab] = from.tabs.splice(index, 1);
    if (zone === "center") {
      to.tabs.push(tab);
      to.active = to.tabs.length - 1;
    } else {
      const fresh = stack(tab);
      const dir = zone === "left" || zone === "right" ? "row" : "col";
      const before = zone === "left" || zone === "top";
      ws.root = replace(ws.root, to.id, (n) => ({
        kind: "split",
        id: uid(dir === "row" ? "r" : "c"),
        dir,
        sizes: [0.5, 0.5],
        children: before ? [fresh, n] : [n, fresh],
      }));
    }
    ws.root = normalize(ws.root) ?? stack(pane("welcome"));
    this.#touch(wsId);
  }

  /** Panes embedded in pages (outside any workspace), by a stable id. */
  embeds = $state<Record<string, PaneRef>>(loadRaw("embeds", {}));

  /** The pane ref for an embedded pane, created on first use. */
  embed(id: string, type: string, props: Record<string, unknown> = {}): PaneRef {
    const existing = this.embeds[id];
    if (existing && existing.type === type) {
      // Props passed by the page (e.g. which camera) win over stored ones.
      existing.props = { ...(existing.props ?? {}), ...props };
      return existing;
    }
    this.embeds[id] = { id, type, props };
    return this.embeds[id];
  }

  setProps(wsId: string, paneId: string, props: Record<string, unknown>) {
    const embedded = this.embeds[paneId];
    if (embedded) {
      embedded.props = { ...(embedded.props ?? {}), ...props };
      save("embeds", this.embeds);
      return;
    }
    const ws = this.get(wsId);
    if (!ws) return;
    const visit = (n: LayoutNode) => {
      if (n.kind === "stack") {
        for (const t of n.tabs) if (t.id === paneId) t.props = { ...(t.props ?? {}), ...props };
      } else n.children.forEach(visit);
    };
    visit(ws.root);
    save("workspaces", this.#saved);
  }
}

export const workspaces = new WorkspaceStore();
