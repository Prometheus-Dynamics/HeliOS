import { derived, writable, type Readable } from 'svelte/store';

export type GraphSelection = {
  nodeId: string | null;
  edgeId: string | null;
  portId: string | null;
};

export type GraphHover = {
  nodeId: string | null;
  portId: string | null;
};

export type GraphViewport = {
  x: number;
  y: number;
  zoom: number;
};

export type PipelineGraphStore = {
  selection: Readable<GraphSelection>;
  hover: Readable<GraphHover>;
  viewport: Readable<GraphViewport>;
  hasSelection: Readable<boolean>;
  selectNode: (nodeId: string | null) => void;
  selectEdge: (edgeId: string | null) => void;
  selectPort: (portId: string | null) => void;
  clearSelection: () => void;
  setHoverNode: (nodeId: string | null) => void;
  setHoverPort: (portId: string | null) => void;
  setViewport: (viewport: Partial<GraphViewport>) => void;
  destroy: () => void;
};

export function createPipelineGraphStore(initial: Partial<GraphViewport> = {}): PipelineGraphStore {
  const selection = writable<GraphSelection>({ nodeId: null, edgeId: null, portId: null });
  const hover = writable<GraphHover>({ nodeId: null, portId: null });
  const viewport = writable<GraphViewport>({
    x: initial.x ?? 0,
    y: initial.y ?? 0,
    zoom: initial.zoom ?? 1
  });

  const hasSelection = derived(selection, ($selection) => Boolean($selection.nodeId || $selection.edgeId || $selection.portId));

  function selectNode(nodeId: string | null): void {
    selection.update((current) => ({ ...current, nodeId, edgeId: null }));
  }

  function selectEdge(edgeId: string | null): void {
    selection.update((current) => ({ ...current, edgeId, nodeId: null }));
  }

  function selectPort(portId: string | null): void {
    selection.update((current) => ({ ...current, portId }));
  }

  function clearSelection(): void {
    selection.set({ nodeId: null, edgeId: null, portId: null });
  }

  function setHoverNode(nodeId: string | null): void {
    hover.update((current) => ({ ...current, nodeId }));
  }

  function setHoverPort(portId: string | null): void {
    hover.update((current) => ({ ...current, portId }));
  }

  function setViewport(next: Partial<GraphViewport>): void {
    viewport.update((current) => ({ ...current, ...next }));
  }

  function destroy(): void {
    selection.set({ nodeId: null, edgeId: null, portId: null });
    hover.set({ nodeId: null, portId: null });
  }

  return {
    selection,
    hover,
    viewport,
    hasSelection,
    selectNode,
    selectEdge,
    selectPort,
    clearSelection,
    setHoverNode,
    setHoverPort,
    setViewport,
    destroy
  };
}
