// Pipeline drafts: the edited, not-yet-deployed graph per workload. The canvas,
// inspector, catalog and profiler panes all work on the same draft, so a
// change in one shows in the others. Deploy turns the draft into a revision.
//
// A draft follows the device: when the deployed graph changes under an unedited
// draft (a default pipeline applied, a tag family or search change, another
// browser's deploy), the draft reloads from it. Edits are kept until deployed
// or discarded.

import { CATALOG_BY_ID, type NodeType } from "#lib/api/catalog.js";
import type { GraphDocument } from "#lib/api/model.js";
import { cluster } from "#lib/stores/cluster.svelte.js";
import { toasts } from "#lib/stores/toasts.svelte.js";
import { fingerprint, newNode, toFlow, toGraphDocument, type FlowEdge, type FlowNode, type ParamValue } from "./graph";
import { autoLayout } from "./layout";

export class Draft {
  nodes = $state.raw<FlowNode[]>([]);
  edges = $state.raw<FlowEdge[]>([]);
  #undo: { nodes: FlowNode[]; edges: FlowEdge[] }[] = [];
  #redo: { nodes: FlowNode[]; edges: FlowEdge[] }[] = [];
  history = $state(0);
  /** Bumped when the canvas should fit the view (load, layout, template). */
  fitSignal = $state(0);
  /** A node the canvas should centre on (set by the find box). A new object each time, so repeats still move. */
  focus = $state<{ id: string } | null>(null);
  /** The document the draft was loaded from: its metadata and pinned versions carry into deploys. */
  #base = $state.raw<GraphDocument | null>(null);
  /** The fingerprint of the deployed graph the draft was loaded from (what "edited" is measured against). */
  #loaded = $state("");
  #stop: (() => void) | null = null;

  constructor(public workloadId: string) {
    const w = cluster.workload(workloadId);
    if (w) this.#load(w.graph);
    // Follow the device: a new deployed graph replaces an unedited draft.
    this.#stop = $effect.root(() => {
      $effect(() => {
        const w = this.workload;
        if (!w) return;
        const now = fingerprint(w.graph);
        // Never loaded (the workload arrived after the draft was made), or loaded and unedited.
        if (now !== this.#loaded && (!this.#loaded || !this.edited)) this.#load(w.graph);
      });
    });
  }

  #load(graph: GraphDocument) {
    const f = toFlow(graph);
    this.nodes = f.nodes;
    this.edges = f.edges;
    this.#base = graph;
    this.#loaded = fingerprint(graph);
  }

  get workload() {
    return cluster.workload(this.workloadId);
  }
  get document(): GraphDocument {
    return toGraphDocument(this.nodes, this.edges, this.#base ?? undefined);
  }
  /** The draft differs from the graph it was loaded from. */
  get edited(): boolean {
    return fingerprint(this.document) !== this.#loaded;
  }
  /** The draft differs from what the device runs now: a deploy would change it. */
  get dirty(): boolean {
    const w = this.workload;
    return Boolean(w && fingerprint(this.document) !== fingerprint(w.graph));
  }
  get selected(): FlowNode | undefined {
    return this.nodes.find((n) => n.selected);
  }
  get unknown(): FlowNode[] {
    return this.nodes.filter((n) => !CATALOG_BY_ID[n.data.type]);
  }
  get canUndo() {
    void this.history;
    return this.#undo.length > 0;
  }
  get canRedo() {
    void this.history;
    return this.#redo.length > 0;
  }

  /** Records the current state before a change, for undo. */
  snapshot() {
    this.#undo.push({ nodes: this.nodes, edges: this.edges });
    if (this.#undo.length > 100) this.#undo.shift();
    this.#redo = [];
    this.history++;
  }
  undo() {
    const s = this.#undo.pop();
    if (!s) return;
    this.#redo.push({ nodes: this.nodes, edges: this.edges });
    this.nodes = s.nodes;
    this.edges = s.edges;
    this.history++;
  }
  redo() {
    const s = this.#redo.pop();
    if (!s) return;
    this.#undo.push({ nodes: this.nodes, edges: this.edges });
    this.nodes = s.nodes;
    this.edges = s.edges;
    this.history++;
  }

  update(id: string, change: (n: FlowNode) => FlowNode) {
    this.nodes = this.nodes.map((n) => (n.id === id ? change(n) : n));
  }
  setParam(id: string, name: string, value: ParamValue) {
    this.snapshot();
    this.update(id, (n) => ({ ...n, data: { ...n.data, params: { ...n.data.params, [name]: value } } }));
  }
  setLabel(id: string, label: string) {
    this.snapshot();
    this.update(id, (n) => ({ ...n, data: { ...n.data, label } }));
  }
  select(id: string | null) {
    this.nodes = this.nodes.map((n) => (n.selected === (n.id === id) ? n : { ...n, selected: n.id === id }));
  }
  add(type: NodeType, at: { x: number; y: number }) {
    this.snapshot();
    this.nodes = [...this.nodes.map((n) => (n.selected ? { ...n, selected: false } : n)), newNode(type, this.nodes, at)];
  }
  remove(ids: string[]) {
    this.snapshot();
    this.nodes = this.nodes.filter((n) => !ids.includes(n.id));
    this.edges = this.edges.filter((e) => !ids.includes(e.source) && !ids.includes(e.target));
  }
  layout() {
    this.snapshot();
    this.nodes = autoLayout(this.nodes, this.edges);
    this.fitSignal++;
  }
  fit() {
    this.fitSignal++;
  }
  focusNode(id: string) {
    this.focus = { id };
  }
  /** Another graph in place of the draft (a template, an import): the loaded document's
   * metadata is kept, the deployed graph stays what "edited" is measured against. */
  replace(graph: GraphDocument) {
    this.snapshot();
    const f = toFlow(graph);
    this.nodes = f.nodes;
    this.edges = f.edges;
    this.fitSignal++;
  }
  discard() {
    const w = this.workload;
    if (!w) return;
    this.snapshot();
    this.#load(w.graph);
    toasts.info("Changes discarded");
  }
  /** Picks up a new deployed graph (deploy, rollback), dropping any edits. */
  reload() {
    const w = this.workload;
    if (w) this.#load(w.graph);
  }
  /** Stops following the device (tests). */
  destroy() {
    this.#stop?.();
    this.#stop = null;
  }

  async deploy() {
    if (this.unknown.length) {
      toasts.error(`Remove ${this.unknown.map((n) => n.id).join(", ")}: not in the device catalog`);
      return;
    }
    if (await cluster.saveGraph(this.workloadId, this.document)) this.reload();
  }
  async rollBack() {
    await cluster.rollBackWorkload(this.workloadId);
    this.reload();
  }
}

class DraftStore {
  #map = new Map<string, Draft>();

  get(workloadId: string): Draft {
    let d = this.#map.get(workloadId);
    if (!d) this.#map.set(workloadId, (d = new Draft(workloadId)));
    return d;
  }
}

export const drafts = new DraftStore();
