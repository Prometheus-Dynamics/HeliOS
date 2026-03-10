import type { Node, Edge } from '@xyflow/svelte';
import type { PipelineGraphPlan, PipelineRegistryEntry } from '$lib/types/pipeline';
import { buildPortOrders } from './utils';
import { buildFlowNodes } from './nodes';
import { buildFlowEdges } from './edges';
import type {
  ActiveConnection,
  BooleanConstantHandler,
  BooleanToggleHandler,
  EnumSelectionHandler,
  NumericConstantToggleHandler,
  NumericValueChangeHandler,
  PixelColorChangeHandler,
  PixelConstantToggleHandler,
  PipelineGraphDiagnostics,
  PipelineGraphHeatmap
} from './types';

export type GraphNodeUpdateOptions = {
  graph: PipelineGraphPlan;
  activeConnection: ActiveConnection | null;
  previousNodes: Node[];
  detailLevel: 'minimal' | 'full';
  focusHighlight: { nodeId: string; port: string | null; token: number } | null;
  resolveRegistryEntryForNode: (node: PipelineGraphPlan['nodes'][string]) => PipelineRegistryEntry | null;
  onNodePortDoubleClick: (nodeId: string, direction: 'input' | 'output', port: string, event: MouseEvent) => void;
  onNodePortContextMenu: (nodeId: string, direction: 'input' | 'output', port: string, event: MouseEvent) => void;
  onNodePortEnumChange: EnumSelectionHandler;
  onNodePortBooleanConstantToggle: BooleanConstantHandler;
  onNodePortBooleanToggle: BooleanToggleHandler;
  onNodePortPixelConstantToggle: PixelConstantToggleHandler;
  onNodePortPixelColorChange: PixelColorChangeHandler;
  onNodePortNumericConstantToggle: NumericConstantToggleHandler;
  onNodePortNumericValueChange: NumericValueChangeHandler;
  onNodePortClear: (payload: { nodeId: string; port: string }) => void;
  onNodeRuntimeChange: (payload: { nodeId: string; syncGroups: unknown[] }) => void;
  heatmap: PipelineGraphHeatmap | null;
  heatmapMode: boolean;
  diagnostics: PipelineGraphDiagnostics;
  syncInspector: { enabled: boolean; focusNodeId?: string | null };
  gpuOverlay: boolean;
  gpuOverlaySegments: Array<{ id: number; nodes: string[] }> | null;
  searchTokens: string[];
  runtimeWarnings: Record<string, { message: string; at: number | null }>;
};

export type GraphEdgeUpdateOptions = {
  graph: PipelineGraphPlan;
  selectedEdgeId: string | null;
  activeConnection: ActiveConnection | null;
  resolveRegistryEntryForNode: (node: PipelineGraphPlan['nodes'][string]) => PipelineRegistryEntry | null;
};

let lastPortOrderGraph: PipelineGraphPlan | null = null;
let lastPortOrders: ReturnType<typeof buildPortOrders> | null = null;

const resolvePortOrders = (graph: PipelineGraphPlan) => {
  if (graph !== lastPortOrderGraph || !lastPortOrders) {
    lastPortOrderGraph = graph;
    lastPortOrders = buildPortOrders(graph);
  }
  return lastPortOrders;
};

export function buildUpdatedNodes(options: GraphNodeUpdateOptions): Node[] {
  return buildFlowNodes({
    graph: options.graph,
    connection: options.activeConnection,
    previousNodes: options.previousNodes,
    portOrders: resolvePortOrders(options.graph),
    detailLevel: options.detailLevel,
    highlight: options.focusHighlight,
    resolveRegistryEntryForNode: options.resolveRegistryEntryForNode,
    onNodePortDoubleClick: options.onNodePortDoubleClick,
    onNodePortContextMenu: options.onNodePortContextMenu,
    onNodePortEnumChange: options.onNodePortEnumChange,
    onNodePortBooleanConstantToggle: options.onNodePortBooleanConstantToggle,
    onNodePortBooleanToggle: options.onNodePortBooleanToggle,
    onNodePortPixelConstantToggle: options.onNodePortPixelConstantToggle,
    onNodePortPixelColorChange: options.onNodePortPixelColorChange,
    onNodePortNumericConstantToggle: options.onNodePortNumericConstantToggle,
    onNodePortNumericValueChange: options.onNodePortNumericValueChange,
    onNodePortClear: options.onNodePortClear,
    onNodeRuntimeChange: options.onNodeRuntimeChange,
    heatmap: options.heatmap,
    heatmapMode: options.heatmapMode,
    diagnostics: options.diagnostics,
    syncInspector: options.syncInspector,
    gpuOverlay: options.gpuOverlay,
    gpuOverlaySegments: options.gpuOverlaySegments,
    searchTokens: options.searchTokens,
    runtimeWarnings: options.runtimeWarnings
  });
}

export function buildUpdatedEdges(options: GraphEdgeUpdateOptions): Edge[] {
  return buildFlowEdges(
    options.graph,
    options.selectedEdgeId,
    options.activeConnection,
    resolvePortOrders(options.graph),
    options.resolveRegistryEntryForNode
  );
}
