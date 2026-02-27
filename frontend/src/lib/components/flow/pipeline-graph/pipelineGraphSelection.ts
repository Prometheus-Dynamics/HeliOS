import type { PipelineGraphStore } from '$lib/features/pipelines/graphStore';
import type { EdgeSelection } from './types';

export function notifyGraphSelection(options: {
  nodeId: string | null;
  edge: EdgeSelection | null;
  nodes?: string[];
  selectedEdgeId: string | null;
  setSelectedEdgeId: (id: string | null) => void;
  setSelectedEdge: (edge: EdgeSelection | null) => void;
  dispatch: (event: 'select', payload: { nodeId: string | null; nodes: string[]; edge: EdgeSelection | null }) => void;
  graphStore: PipelineGraphStore;
}): void {
  const nextId = options.edge?.id ?? null;
  if (options.selectedEdgeId !== nextId) {
    options.setSelectedEdgeId(nextId);
  }
  options.setSelectedEdge(options.edge);
  const nodeList = Array.isArray(options.nodes)
    ? options.nodes.filter(Boolean)
    : options.nodeId
      ? [options.nodeId]
      : [];
  options.dispatch('select', { nodeId: options.nodeId, nodes: nodeList, edge: options.edge });
  const primaryNode = options.nodeId ?? nodeList[0] ?? null;
  if (options.edge?.id) {
    options.graphStore.selectEdge(options.edge.id);
  } else if (primaryNode) {
    options.graphStore.selectNode(primaryNode);
  } else {
    options.graphStore.clearSelection();
  }
}
