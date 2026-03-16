import type { Node } from '@xyflow/svelte';
import type {
  ActiveConnection,
  EnumSelectionHandler,
  BooleanToggleHandler,
  BooleanConstantHandler,
  PixelConstantToggleHandler,
  PixelColorChangeHandler,
  NumericConstantToggleHandler,
  NumericValueChangeHandler,
  PipelineGraphDiagnostics,
  PipelineGraphHeatmap,
  PipelineNodeDiagnostics
} from '../types';
import type { PipelineDataType, PipelineGraphPlan, PipelineRegistryEntry } from '$lib/types/pipeline';
import { PIPELINE_INPUT_BACKEND_ID, PIPELINE_OUTPUT_BACKEND_ID } from '$lib/features/pipelines/boundary';
import type { ApiGraphNode } from '$lib/types/pipeline-api';
import { detectGpuSegments } from '../gpuOverlay';
import { deriveHeatmapStats, resolveNodeHeatmapPayload } from './heatmap';
import { appendDataTypeTokens, matchesSearchTokens } from './search';
import { applyFanInTypes, filterFanInInputs, mergePorts, resolveGenericPorts, resolvePortOrder } from './ports';

type NodeBuilderData = {
  node: PipelineGraphPlan['nodes'][string];
  apiNode: ApiGraphNode | null;
  registryEntry: PipelineRegistryEntry | null;
  inputOrder: string[];
  outputOrder: string[];
  activeConnection: ActiveConnection | null;
  detailLevel: 'minimal' | 'full';
  portEditorsMode: 'selected' | 'always' | 'never';
  gpuOverlay: boolean;
  gpuSegment: number | null;
  gpuPeers: number | null;
  searchActive: boolean;
  searchMatch: boolean;
  searchTokens: string[];
  heatmap: ReturnType<typeof resolveNodeHeatmapPayload>;
  heatmapMode: boolean;
  diagnostics: PipelineNodeDiagnostics | null;
  highlight: { port: string | null; token: number | null } | null;
  syncOverlay: { enabled: boolean; focus: boolean } | null;
  runtimeWarning: { message: string; at: number | null } | null;
  onPortDoubleClick: (payload: { direction: 'input' | 'output'; port: string; event: MouseEvent }) => void;
  onPortContextMenu: (payload: { direction: 'input' | 'output'; port: string; event: MouseEvent }) => void;
  onPortEnumChange: EnumSelectionHandler;
  onPortBooleanConstantToggle: BooleanConstantHandler;
  onPortBooleanToggle: BooleanToggleHandler;
  onPortPixelConstantToggle: PixelConstantToggleHandler;
  onPortPixelColorChange: PixelColorChangeHandler;
  onPortNumericConstantToggle: NumericConstantToggleHandler;
  onPortNumericValueChange: NumericValueChangeHandler;
  onPortClear: (payload: { nodeId: string; port: string }) => void;
  onNodeRuntimeChange: ((payload: { nodeId: string; syncGroups: unknown[] }) => void) | undefined;
};

const stringArrayEquals = (left: string[] | undefined, right: string[] | undefined): boolean => {
  const a = left ?? [];
  const b = right ?? [];
  if (a.length !== b.length) return false;
  for (let index = 0; index < a.length; index += 1) {
    if (a[index] !== b[index]) return false;
  }
  return true;
};

const highlightEquals = (
  left: { port: string | null; token: number | null } | null | undefined,
  right: { port: string | null; token: number | null } | null | undefined
): boolean => {
  if (left === right) return true;
  if (!left || !right) return left === right;
  return left.port === right.port && left.token === right.token;
};

const syncOverlayEquals = (
  left: { enabled: boolean; focus: boolean } | null | undefined,
  right: { enabled: boolean; focus: boolean } | null | undefined
): boolean => {
  if (left === right) return true;
  if (!left || !right) return left === right;
  return left.enabled === right.enabled && left.focus === right.focus;
};

const runtimeWarningEquals = (
  left: { message: string; at: number | null } | null | undefined,
  right: { message: string; at: number | null } | null | undefined
): boolean => {
  if (left === right) return true;
  if (!left || !right) return left === right;
  return left.message === right.message && left.at === right.at;
};

const heatmapEquals = (
  left: ReturnType<typeof resolveNodeHeatmapPayload> | null | undefined,
  right: ReturnType<typeof resolveNodeHeatmapPayload> | null | undefined
): boolean => {
  if (left === right) return true;
  if (!left || !right) return left === right;
  return (
    left.normalized === right.normalized &&
    left.totalTimeMs === right.totalTimeMs &&
    left.peakTimeMs === right.peakTimeMs &&
    left.averageFps === right.averageFps &&
    left.sampleCount === right.sampleCount &&
    left.streamCount === right.streamCount &&
    left.globalAverageTimeMs === right.globalAverageTimeMs &&
    left.deltaFromAverageMs === right.deltaFromAverageMs &&
    left.deltaFromAveragePercent === right.deltaFromAveragePercent
  );
};

const nodeDataEquals = (left: NodeBuilderData, right: NodeBuilderData): boolean =>
  left.node === right.node &&
  left.apiNode === right.apiNode &&
  left.registryEntry === right.registryEntry &&
  left.activeConnection === right.activeConnection &&
  left.detailLevel === right.detailLevel &&
  left.portEditorsMode === right.portEditorsMode &&
  left.gpuOverlay === right.gpuOverlay &&
  left.gpuSegment === right.gpuSegment &&
  left.gpuPeers === right.gpuPeers &&
  left.searchActive === right.searchActive &&
  left.searchMatch === right.searchMatch &&
  stringArrayEquals(left.searchTokens, right.searchTokens) &&
  heatmapEquals(left.heatmap, right.heatmap) &&
  left.heatmapMode === right.heatmapMode &&
  left.diagnostics === right.diagnostics &&
  highlightEquals(left.highlight, right.highlight) &&
  syncOverlayEquals(left.syncOverlay, right.syncOverlay) &&
  runtimeWarningEquals(left.runtimeWarning, right.runtimeWarning) &&
  stringArrayEquals(left.inputOrder, right.inputOrder) &&
  stringArrayEquals(left.outputOrder, right.outputOrder) &&
  left.onPortDoubleClick === right.onPortDoubleClick &&
  left.onPortContextMenu === right.onPortContextMenu &&
  left.onPortEnumChange === right.onPortEnumChange &&
  left.onPortBooleanConstantToggle === right.onPortBooleanConstantToggle &&
  left.onPortBooleanToggle === right.onPortBooleanToggle &&
  left.onPortPixelConstantToggle === right.onPortPixelConstantToggle &&
  left.onPortPixelColorChange === right.onPortPixelColorChange &&
  left.onPortNumericConstantToggle === right.onPortNumericConstantToggle &&
  left.onPortNumericValueChange === right.onPortNumericValueChange &&
  left.onPortClear === right.onPortClear &&
  left.onNodeRuntimeChange === right.onNodeRuntimeChange;

export type BuildFlowNodesOptions = {
  graph: PipelineGraphPlan;
  connection: ActiveConnection | null;
  previousNodes: Node[];
  portOrders: { inputs: Record<string, string[]>; outputs: Record<string, string[]> };
  detailLevel: 'minimal' | 'full';
  portEditorsMode?: 'selected' | 'always' | 'never';
  highlight?: { nodeId: string | null; port: string | null; token: number | null } | null;
  searchTokens?: string[];
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
  onNodePortClear?: (payload: { nodeId: string; port: string }) => void;
  onNodeRuntimeChange?: (payload: { nodeId: string; syncGroups: unknown[] }) => void;
  heatmap?: PipelineGraphHeatmap | null;
  heatmapMode?: boolean;
  diagnostics?: PipelineGraphDiagnostics | null;
  syncInspector?: { enabled: boolean; focusNodeId?: string | null };
  gpuOverlay?: boolean;
  gpuOverlaySegments?: Array<{ id: number; nodes: string[] }> | null;
  runtimeWarnings?: Record<string, { message: string; at: number | null }>;
};

export const buildFlowNodes = ({
  graph,
  connection,
  previousNodes,
  portOrders,
  detailLevel,
  portEditorsMode = 'selected',
  highlight = null,
  searchTokens = [],
  resolveRegistryEntryForNode,
  onNodePortDoubleClick,
  onNodePortContextMenu,
  onNodePortEnumChange,
  onNodePortBooleanConstantToggle,
  onNodePortBooleanToggle,
  onNodePortPixelConstantToggle,
  onNodePortPixelColorChange,
  onNodePortNumericConstantToggle,
  onNodePortNumericValueChange,
  onNodePortClear,
  onNodeRuntimeChange,
  heatmap = null,
  heatmapMode = false,
  diagnostics = null,
  syncInspector,
  gpuOverlay = false,
  gpuOverlaySegments = null,
  runtimeWarnings = {}
}: BuildFlowNodesOptions): Node[] => {
  const previousMap = new Map(previousNodes.map((node) => [node.id, node]));
  const heatmapStats = deriveHeatmapStats(heatmap);
  const shouldResolveGpu =
    gpuOverlay || (Array.isArray(gpuOverlaySegments) && gpuOverlaySegments.length > 0);
  const detectedGpu = shouldResolveGpu
    ? detectGpuSegments(graph, {
        resolveRegistryEntry: (node) => resolveRegistryEntryForNode(node)
      })
    : {
        nodeToSegment: new Map<string, number>(),
        segments: [] as Array<{ id: number; nodes: string[] }>,
        gpuNodeIds: new Set<string>()
      };
  const runtimeSegments =
    Array.isArray(gpuOverlaySegments) && gpuOverlaySegments.length > 0
      ? gpuOverlaySegments.filter((segment) => Number.isFinite(segment.id) && Array.isArray(segment.nodes))
      : null;
  const runtimeNodeToSegment = new Map<string, number>();
  if (runtimeSegments) {
    runtimeSegments.forEach((segment) => {
      segment.nodes.forEach((nodeId) => runtimeNodeToSegment.set(nodeId, segment.id));
    });
  }
  const nodeToSegment = runtimeSegments ? runtimeNodeToSegment : detectedGpu.nodeToSegment;
  const segments = runtimeSegments ?? detectedGpu.segments;
  const hasGpuSegments = segments.length > 0;
  const gpuOverlayActive = gpuOverlay && hasGpuSegments;
  const segmentSizeById = new Map<number, number>();
  segments.forEach((segment) => segmentSizeById.set(segment.id, segment.nodes.length));
  const activeSearchTokens = Array.isArray(searchTokens) ? searchTokens : [];
  const searchActive = activeSearchTokens.length > 0;

  const used = new Set<string>();
  const result: Node[] = [];

  const applyPipelineNode = (nodeKey: string, planNode: PipelineGraphPlan['nodes'][string]) => {
    const existing = previousMap.get(nodeKey);
    const existingData = (existing?.data ?? null) as NodeBuilderData | null;
    const position = {
      x: planNode.info.location?.x ?? 0,
      y: planNode.info.location?.y ?? 0
    };
    const registryEntry = resolveRegistryEntryForNode(planNode);

    const mergedInputs = applyFanInTypes(
      mergePorts(planNode.inputs, registryEntry?.inputs),
      registryEntry?.faninInputs
    );
    const mergedOutputs = mergePorts(planNode.outputs, registryEntry?.outputs);
    const visibleInputs = filterFanInInputs(
      mergedInputs,
      registryEntry?.faninInputs,
      planNode.id,
      graph.connections,
      graph.nodes
    );
    const backendId = planNode.backendId?.toLowerCase() ?? '';
    const resetOnDisconnect =
      backendId === PIPELINE_INPUT_BACKEND_ID ||
      backendId === PIPELINE_OUTPUT_BACKEND_ID ||
      backendId === 'io.host_bridge' ||
      backendId === 'io.host_output' ||
      backendId.endsWith(':io.host_bridge') ||
      backendId.endsWith(':io.host_output') ||
      backendId.endsWith(`:${PIPELINE_INPUT_BACKEND_ID}`) ||
      backendId.endsWith(`:${PIPELINE_OUTPUT_BACKEND_ID}`);
    const resolvedInputs = resolveGenericPorts(
      visibleInputs ?? mergedInputs,
      graph,
      planNode.id,
      'input',
      { resetOnDisconnect }
    );
    const resolvedOutputs = resolveGenericPorts(
      mergedOutputs,
      graph,
      planNode.id,
      'output',
      { resetOnDisconnect }
    );
    const displayNode =
      resolvedInputs !== planNode.inputs || resolvedOutputs !== planNode.outputs
        ? { ...planNode, inputs: resolvedInputs ?? mergedInputs, outputs: resolvedOutputs ?? mergedOutputs }
        : planNode;

    const inputOrder = resolvePortOrder(portOrders.inputs[planNode.id], displayNode.inputs);
    const outputOrder = resolvePortOrder(portOrders.outputs[planNode.id], displayNode.outputs);

    const handleDoubleClick =
      existingData?.onPortDoubleClick ??
      (({ direction, port, event }: { direction: 'input' | 'output'; port: string; event: MouseEvent }) =>
        onNodePortDoubleClick(planNode.id, direction, port, event));
    const handleContextMenu =
      existingData?.onPortContextMenu ??
      (({ direction, port, event }: { direction: 'input' | 'output'; port: string; event: MouseEvent }) =>
        onNodePortContextMenu(planNode.id, direction, port, event));
    const handleEnumChange: EnumSelectionHandler =
      existingData?.onPortEnumChange ??
      (({ nodeId, port, value }) => {
        if (nodeId === planNode.id) {
          onNodePortEnumChange({ nodeId, port, value });
        }
      });
    const handleBooleanToggle: BooleanToggleHandler =
      existingData?.onPortBooleanToggle ??
      (({ nodeId, port, value }) => {
        if (nodeId === planNode.id) {
          onNodePortBooleanToggle({ nodeId, port, value });
        }
      });
    const handleBooleanConstantToggle: BooleanConstantHandler =
      existingData?.onPortBooleanConstantToggle ??
      (({ nodeId, port, enabled }) => {
        if (nodeId === planNode.id) {
          onNodePortBooleanConstantToggle({ nodeId, port, enabled });
        }
      });
    const handlePixelConstantToggle: PixelConstantToggleHandler =
      existingData?.onPortPixelConstantToggle ??
      (({ nodeId, port, enabled }) => {
        if (nodeId === planNode.id) {
          onNodePortPixelConstantToggle({ nodeId, port, enabled });
        }
      });
    const handlePixelColorChange: PixelColorChangeHandler =
      existingData?.onPortPixelColorChange ??
      (({ nodeId, port, hex }) => {
        if (nodeId === planNode.id) {
          onNodePortPixelColorChange({ nodeId, port, hex });
        }
      });
    const handleNumericConstantToggle: NumericConstantToggleHandler =
      existingData?.onPortNumericConstantToggle ??
      (({ nodeId, port, enabled }) => {
        if (nodeId === planNode.id) {
          onNodePortNumericConstantToggle({ nodeId, port, enabled });
        }
      });
    const handleNumericValueChange: NumericValueChangeHandler =
      existingData?.onPortNumericValueChange ??
      (({ nodeId, port, value }) => {
        if (nodeId === planNode.id) {
          onNodePortNumericValueChange({ nodeId, port, value });
        }
      });
    const handlePortClear =
      existingData?.onPortClear ??
      ((payload: { nodeId: string; port: string }) => {
        if (!onNodePortClear) return;
        if (payload.nodeId !== planNode.id) return;
        onNodePortClear(payload);
      });
    const handleRuntimeChange =
      existingData?.onNodeRuntimeChange ??
      (onNodeRuntimeChange
        ? ({ nodeId, syncGroups }: { nodeId: string; syncGroups: unknown[] }) => {
            if (nodeId !== planNode.id) return;
            onNodeRuntimeChange({ nodeId, syncGroups });
          }
        : undefined);
    const apiNode: ApiGraphNode | null = displayNode.source ?? null;
    const nodeDiagnostics: PipelineNodeDiagnostics | null = diagnostics?.[nodeKey] ?? null;
    const nodeHighlight =
      highlight && highlight.nodeId === nodeKey
        ? {
            port: highlight.port ?? null,
            token: highlight.token ?? null
          }
        : null;
    let nodeSearchMatch = false;
    if (searchActive) {
      const searchTerms: string[] = [
        planNode.id,
        planNode.backendId,
        planNode.metadata?.name,
        planNode.metadata?.summary,
        planNode.info?.id,
        registryEntry?.metadata?.name,
        registryEntry?.metadata?.summary
      ];
      const addPorts = (ports: Record<string, PipelineDataType> | undefined) => {
        if (!ports) return;
        Object.entries(ports).forEach(([name, type]) => {
          searchTerms.push(name);
          appendDataTypeTokens(searchTerms, type);
        });
      };
      addPorts(displayNode.inputs);
      addPorts(displayNode.outputs);
      const nodeValues = planNode.info?.values ?? null;
      if (nodeValues) {
        Object.entries(nodeValues).forEach(([name, value]) => {
          searchTerms.push(name);
          if (typeof value === 'string') {
            searchTerms.push(value);
            return;
          }
          if (typeof value === 'object' && value && 'value' in value) {
            const raw = (value as { value?: unknown }).value;
            if (raw != null) {
              searchTerms.push(String(raw));
            }
          }
        });
      }
      nodeSearchMatch = matchesSearchTokens(activeSearchTokens, searchTerms);
    }
    const syncOverlay =
      syncInspector?.enabled
        ? {
            enabled: true,
            focus: Boolean(syncInspector.focusNodeId && syncInspector.focusNodeId === nodeKey)
          }
        : null;
    const nodeSegmentId = nodeToSegment.get(nodeKey) ?? null;
    const nodeHeatmap = resolveNodeHeatmapPayload(heatmap, planNode, nodeKey, heatmapStats);
    const nodeData: NodeBuilderData = {
      node: displayNode,
      apiNode,
      registryEntry,
      activeConnection: connection,
      detailLevel,
      portEditorsMode,
      gpuOverlay: gpuOverlayActive,
      gpuSegment: nodeSegmentId,
      gpuPeers: nodeSegmentId == null ? null : (segmentSizeById.get(nodeSegmentId) ?? null),
      inputOrder,
      outputOrder,
      onPortDoubleClick: handleDoubleClick,
      onPortContextMenu: handleContextMenu,
      onPortEnumChange: handleEnumChange,
      onPortBooleanConstantToggle: handleBooleanConstantToggle,
      onPortBooleanToggle: handleBooleanToggle,
      onPortPixelConstantToggle: handlePixelConstantToggle,
      onPortPixelColorChange: handlePixelColorChange,
      onPortNumericConstantToggle: handleNumericConstantToggle,
      onPortNumericValueChange: handleNumericValueChange,
      onPortClear: handlePortClear,
      onNodeRuntimeChange: handleRuntimeChange,
      searchActive,
      searchMatch: nodeSearchMatch,
      searchTokens: activeSearchTokens,
      heatmap: nodeHeatmap,
      heatmapMode,
      diagnostics: nodeDiagnostics,
      highlight: nodeHighlight,
      syncOverlay,
      runtimeWarning: runtimeWarnings[nodeKey] ?? null
    };

    if (existing) {
      const hasPositionChanged =
        existing.position?.x !== position.x || existing.position?.y !== position.y;
      const hasDataChanged = !existingData || !nodeDataEquals(existingData, nodeData);
      const reuse =
        !hasPositionChanged &&
        !hasDataChanged &&
        existing.type === 'pipeline' &&
        existing.draggable === true &&
        existing.selectable === true &&
        existing.connectable === true;
      if (reuse) {
        result.push(existing);
      } else {
        result.push({
          ...existing,
          id: nodeKey,
          type: 'pipeline',
          position: { ...position },
          data: nodeData,
          draggable: true,
          selectable: true,
          connectable: true
        });
      }
    } else {
      result.push({
        id: nodeKey,
        type: 'pipeline',
        position: { ...position },
        data: nodeData,
        draggable: true,
        selectable: true,
        connectable: true
      });
    }
    used.add(nodeKey);
  };

  for (const prev of previousNodes) {
    const planNode = graph.nodes?.[prev.id];
    if (planNode) {
      applyPipelineNode(prev.id, planNode);
      continue;
    }
  }

  for (const [nodeKey, planNode] of Object.entries(graph.nodes ?? {})) {
    if (!used.has(nodeKey)) {
      applyPipelineNode(nodeKey, planNode);
    }
  }

  return result;
};
