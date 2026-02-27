import type { Edge, Node } from '@xyflow/svelte';
import type { PipelineConnectionStyle, PipelineGraphPlan, PipelineRegistryEntry } from '$lib/types/pipeline';
import type { ActiveConnection, EdgeSelection } from './types';
import {
  buildUpdatedEdges,
  buildUpdatedNodes,
  type GraphNodeUpdateOptions
} from './pipelineGraphUpdates';
import { applyConnectionStyleToGraph } from './pipelineGraphConnectionStyle';

type GraphUpdateControllerOptions = {
  getPlan: () => PipelineGraphPlan;
  setPlan: (plan: PipelineGraphPlan) => void;
  getEdges: () => Edge[];
  setEdges: (edges: Edge[]) => void;
  getPreviousNodes: () => Node[];
  setNodes: (nodes: Node[]) => void;
  getSelectedEdgeId: () => string | null;
  getActiveConnection: () => ActiveConnection | null;
  buildNodeOptions: () => Omit<
    GraphNodeUpdateOptions,
    'graph' | 'activeConnection' | 'previousNodes'
  >;
  resolveRegistryEntryForNode: (node: PipelineGraphPlan['nodes'][string]) => PipelineRegistryEntry | null;
  edgeInteractionsEnabled: boolean;
  defaultStyle: PipelineConnectionStyle;
  normalizeConnectionStyle: (style: PipelineConnectionStyle) => PipelineConnectionStyle;
  commitPlan: () => void;
};

export const createGraphUpdateController = ({
  getPlan,
  setPlan,
  getEdges,
  setEdges,
  getPreviousNodes,
  setNodes,
  getSelectedEdgeId,
  getActiveConnection,
  buildNodeOptions,
  resolveRegistryEntryForNode,
  edgeInteractionsEnabled,
  defaultStyle,
  normalizeConnectionStyle,
  commitPlan
}: GraphUpdateControllerOptions) => {
  const updateNodes = () => {
    const options = buildNodeOptions();
    setNodes(
      buildUpdatedNodes({
        graph: getPlan(),
        activeConnection: getActiveConnection(),
        previousNodes: getPreviousNodes(),
        ...options
      })
    );
  };

  const updateEdges = () => {
    if (!edgeInteractionsEnabled) {
      setEdges([]);
      return;
    }
    setEdges(
      buildUpdatedEdges({
        graph: getPlan(),
        selectedEdgeId: getSelectedEdgeId(),
        activeConnection: getActiveConnection(),
        resolveRegistryEntryForNode
      })
    );
  };

  const applyConnectionStyle = (
    selection: EdgeSelection,
    nextStyle: PipelineConnectionStyle | null,
    { commit = true }: { commit?: boolean } = {}
  ) => {
    const result = applyConnectionStyleToGraph({
      selection,
      nextStyle,
      commit,
      plan: getPlan(),
      edges: getEdges(),
      defaultStyle,
      normalize: normalizeConnectionStyle
    });
    setPlan(result.plan);
    setEdges(result.edges);
    if (commit) {
      commitPlan();
    }
  };

  return { updateNodes, updateEdges, applyConnectionStyle };
};
