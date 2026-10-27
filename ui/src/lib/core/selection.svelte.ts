// One selection for the whole app: click a camera, node, process, tag or
// resource anywhere and every pane following the selection shows it.

export type SelectionKind = "camera" | "workload" | "graph-node" | "device" | "resource" | "stream" | "process" | "tag";

export interface Selection {
  kind: SelectionKind;
  id: string;
  /** Owning object, e.g. the workload a graph node belongs to. */
  parent?: string;
  /** The order of the selection (the newest is the largest), set by `select`. */
  at?: number;
}

class SelectionStore {
  current = $state<Selection | null>(null);
  /** Last selected object of each kind, so panes can follow "the camera" etc. */
  last = $state<Partial<Record<SelectionKind, Selection>>>({});
  private count = 0;

  select(selection: Selection | null) {
    this.current = selection;
    if (selection) this.last[selection.kind] = { ...selection, at: ++this.count };
  }

  is(kind: SelectionKind, id: string) {
    return this.current?.kind === kind && this.current.id === id;
  }
}

export const selection = new SelectionStore();
