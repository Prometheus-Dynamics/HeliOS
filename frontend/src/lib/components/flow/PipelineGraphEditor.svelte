<script lang="ts">
  import { createEventDispatcher, onDestroy } from 'svelte';
  import { untrack } from 'svelte';
  import '@xyflow/svelte/dist/style.css';
  import {
    type EdgeTypes,
    type Edge,
    type Node,
    type NodeTypes
  } from '@xyflow/svelte';
  import type { useSvelteFlow } from '@xyflow/svelte';
  import type {
    OnConnectEnd,
    Viewport,
    XYPosition
  } from '@xyflow/system';
  import PipelineNode from './PipelineNode.svelte';
  import type {
    PipelineConnectionStyle,
    PipelineGraphPlan,
    PipelineNodeLayout
  } from '$lib/types/pipeline';
  import { clonePlan, ensurePlanPortMetadata } from './pipeline-graph/utils';
  import { createRegistryResolver } from './pipeline-graph/registry';
  import type {
    ActiveConnection,
    EdgeSelection,
    PipelineGraphDiagnostics,
    PipelineGraphHeatmap,
    PortEditorState
  } from './pipeline-graph/types';
  import {
    mergeTheme,
    buildGraphDiagnostics,
    parsePixelDraft,
    parsePixelValue,
    pixelToHex,
    isPixelTypeKey,
    DEFAULT_PIXEL_COLOR
  } from './pipeline-graph/editorUtils';
  import { createHistoryManager } from './pipeline-graph/historyManager';
  import PipelineGraphEditorView from './pipeline-graph/PipelineGraphEditorView.svelte';
  import { rebuildPlan } from './pipeline-graph/planUtils';
  import { createInteractionController } from './pipeline-graph/interactionController';
  import { createConnectEndHandler } from './pipeline-graph/connectEndHandler';
  import { createValueController } from './pipeline-graph/valueHandlers';
  import PipelineConnectionEdge from './pipeline-graph/PipelineConnectionEdge.svelte';
  import { DEFAULT_CONNECTION_STYLE, normalizeConnectionStyle } from './pipeline-graph/edgeStyle';
  import { createPipelineGraphStore } from '$lib/features/pipelines/graphStore';
  import { createHeatmapScheduler } from './pipeline-graph/heatmapScheduler';
  import { deriveSelectionFromId } from './pipeline-graph/selectionHelpers';
  import { findNodeAtFlowPosition } from './pipeline-graph/hitTest';
  import { createHistoryControls } from './pipeline-graph/pipelineGraphHistoryControls';
  import { buildLayoutSnapshot } from './pipeline-graph/pipelineGraphLayout';
  import { commitGraphPlan } from './pipeline-graph/pipelineGraphCommit';
  import {
    applyFocusRequest as applyGraphFocusRequest,
    focusOnGraphCenter as focusGraphCenter,
    type FocusRequest
  } from './pipeline-graph/pipelineGraphFocus';
  import { createFocusHighlighter, type FocusHighlight } from './pipeline-graph/pipelineGraphFocusHighlight';
  import { notifyGraphSelection } from './pipeline-graph/pipelineGraphSelection';
  import { createGraphUpdateController } from './pipeline-graph/pipelineGraphUpdateController';
  import { createWindowKeydownHandler } from './pipeline-graph/pipelineGraphWindowKeydown';
  import {
    EDGE_STYLE_OPTIONS,
    applyEdgeStyleSelection,
    buildSelectedEdgeStyle
  } from './pipeline-graph/pipelineGraphEditorEdgeStyle';
  import type { GraphContextEvent, PipelineGraphEditorProps } from './pipeline-graph/pipelineGraphEditorTypes';
  import {
    clampValue,
    isEditableTarget,
    normalizeSearchTokens,
    resolveClientPosition
  } from './pipeline-graph/editorHelpers';
  import { emptyPipelineGraphPlan } from '$lib/features/pipelines/graph';

  const EDGE_INTERACTIONS_ENABLED = true;
  const VIEWPORT_MIN_ZOOM = 0.03;
  const VIEWPORT_MAX_ZOOM = 2.5;
  const FOCUS_HIGHLIGHT_DURATION_MS = 2400;
  const MIN_FLOW_HEIGHT = 320;
  const FALLBACK_MAX_FLOW_HEIGHT = 1100;
  const VIEWPORT_PADDING = 160;
  const VIEWPORT_EPSILON = 0.0001;
  const FULL_NODE_DETAIL_MIN_ZOOM = 0.55;
  const MINIMAL_NODE_DETAIL_GRAPH_THRESHOLD = 60;

  type FlowApi = ReturnType<typeof useSvelteFlow>;
  let viewportHeight = $state(FALLBACK_MAX_FLOW_HEIGHT);
  if (typeof window !== 'undefined') {
    const syncViewportHeight = () => {
      viewportHeight = Math.max(MIN_FLOW_HEIGHT, window.innerHeight || FALLBACK_MAX_FLOW_HEIGHT);
    };
    syncViewportHeight();
    window.addEventListener('resize', syncViewportHeight);
    onDestroy(() => window.removeEventListener('resize', syncViewportHeight));
  }
  let {
    plan,
    interactive = true,
    height = 520,
    fluid = false,
    className = '',
    selectedEdgeId = null,
    registryEntries = [],
    theme: themeOverrides = {},
    metricsHeatmap = null,
    heatmapMode = false,
    gpuOverlayMode = false,
    gpuOverlaySegments = null,
    diagnostics = null,
    syncInspector = { enabled: false, focusNodeId: null },
    runtimeWarnings = {},
    searchQuery = '',
    graphStore: graphStoreProp = null
  }: PipelineGraphEditorProps = $props();
  const getGraphStoreProp = () => graphStoreProp;
  const resolveInitialPlan = (): PipelineGraphPlan =>
    ensurePlanPortMetadata(clonePlan(plan ?? emptyPipelineGraphPlan()));
  const resolveInteractive = (): boolean => interactive;
  const resolveGpuOverlayMode = (): boolean => gpuOverlayMode;
  const graphStore = getGraphStoreProp() ?? createPipelineGraphStore();
  const ownsGraphStore = getGraphStoreProp() == null;
  const dispatch = createEventDispatcher<{
    change: { plan: PipelineGraphPlan };
    select: { nodeId: string | null; nodes: string[]; edge: EdgeSelection | null };
    edit: { nodeId: string };
    context: GraphContextEvent;
    viewport: { viewport: Viewport | undefined; center: XYPosition | null };
    layout: { nodes: PipelineNodeLayout };
    runtime: { nodeId: string; syncGroups: unknown[] };
  }>();
  let internalPlan = $state<PipelineGraphPlan>(resolveInitialPlan());
  let nodes = $state<Node[]>([]);
  let edges = $state<Edge[]>([]);
  let flowViewport = $state<Viewport>({ x: 0, y: 0, zoom: 1 });
  let flowContainer = $state<HTMLDivElement | null>(null);
  let lastPlanRef: PipelineGraphPlan | null = null;
  let activeConnection = $state<ActiveConnection | null>(null);
  let lastNodeClick: { id: string | null; timestamp: number } = { id: null, timestamp: 0 };
  let selectedEdge = $state<EdgeSelection | null>(null);
  let isDragging = false;
  let pendingPlan: PipelineGraphPlan | null = null;
  let portEditor = $state<PortEditorState | null>(null);
  let portEditorDraft = $state('');
  let portEditorError = $state<string | null>(null);
  let lastLayoutHash: string | null = null;
  const history = createHistoryManager(resolveInitialPlan(), { interactive: resolveInteractive() });
  let flowApi: FlowApi | null = null;
  let pendingFocusRequest: FocusRequest | null = null;
  let focusHighlight = $state<FocusHighlight | null>(null);
  const gpuOverlay = $derived(resolveGpuOverlayMode());
  const HEATMAP_NODE_REFRESH_MS = 520;
  let heatmapForNodes = $state<PipelineGraphHeatmap | null>(null);
  const searchTokens = $derived.by(() => normalizeSearchTokens(searchQuery));
  const resolveRegistryEntryForNode = $derived(createRegistryResolver(registryEntries));
  const resolvedTheme = $derived(mergeTheme(themeOverrides));
  const resolveHeatmapCandidate = $derived.by<PipelineGraphHeatmap | null>(() => {
    if (!metricsHeatmap?.enabled || metricsHeatmap.maxValue <= 0) {
      return null;
    }
    return metricsHeatmap;
  });
  const graphDiagnostics = $derived.by<PipelineGraphDiagnostics>(() =>
    buildGraphDiagnostics(internalPlan, diagnostics)
  );
  let lastPointer: XYPosition | null = null;
  let lastFlowPointer: XYPosition | null = null;
  let toFlowPositionRef: (pos: XYPosition) => XYPosition = (pos) => pos;
  let layoutSnapshotFrame: number | null = null;
  let viewportPublishFrame: number | null = null;
  let initialFitFrame: number | null = null;
  let lastPublishedViewport: Viewport | null = null;
  let initialFitPending = true;
  let nodeDetailLevel = $state<'minimal' | 'full'>('minimal');
  const graphNodeCount = $derived.by(() => Object.keys(internalPlan.nodes ?? {}).length);

  const isPixelPortEditor = $derived(Boolean(portEditor) && isPixelTypeKey(portEditor?.dataTypeKey ?? null));
  const pixelEditorState = $derived(
    portEditor && isPixelPortEditor
      ? parsePixelDraft(portEditorDraft) ?? parsePixelValue(portEditor.existingValue ?? null) ?? DEFAULT_PIXEL_COLOR
      : null
  );
  const pixelEditorHex = $derived(pixelEditorState ? pixelToHex(pixelEditorState) : '#ffffff');
  const pixelEditorAlpha = $derived(pixelEditorState?.a ?? 255);

  const nodeTypes: NodeTypes = {
    pipeline: PipelineNode
  };

  const edgeTypes: EdgeTypes = {
    pipeline: PipelineConnectionEdge
  };
  const edgeStyleOptions = EDGE_STYLE_OPTIONS;
  const selectedEdgeStyle = $derived.by(() => buildSelectedEdgeStyle(selectedEdge, internalPlan));

  const availableMaxHeight = $derived.by(() =>
    Math.max(MIN_FLOW_HEIGHT, Math.min(FALLBACK_MAX_FLOW_HEIGHT, viewportHeight - VIEWPORT_PADDING))
  );
  const resolvedHeight = $derived.by(() =>
    clampValue(height ?? MIN_FLOW_HEIGHT, MIN_FLOW_HEIGHT, availableMaxHeight)
  );
  const containerHeightStyle = $derived.by(() =>
    fluid
      ? `min-height:${MIN_FLOW_HEIGHT}px; max-height:${availableMaxHeight}px; height:100%;`
      : `height:${resolvedHeight}px; max-height:${availableMaxHeight}px;`
  );

  const { closePortEditor, undoPlanChange, redoPlanChange } = createHistoryControls({
    history,
    setPlan: (next) => {
      internalPlan = next;
    },
    dispatchChange: (next) => {
      dispatch('change', { plan: next });
    },
    setPortEditor: (next) => {
      portEditor = next;
    },
    setPortEditorDraft: (next) => {
      portEditorDraft = next;
    },
    setPortEditorError: (next) => {
      portEditorError = next;
    }
  });

  const valueController = createValueController({
    getPlan: () => internalPlan,
    setPlan: (plan) => (internalPlan = plan),
    commitPlan,
    interactive: () => interactive,
    getPortEditor: () => portEditor,
    setPortEditor: (next) => (portEditor = next),
    getDraft: () => portEditorDraft,
    setDraft: (draft) => (portEditorDraft = draft),
    getError: () => portEditorError,
    setError: (err) => (portEditorError = err),
    getIsPixelPortEditor: () => isPixelPortEditor,
    getPixelEditorState: () => pixelEditorState,
    closePortEditor,
    undoPlan: undoPlanChange,
    redoPlan: redoPlanChange
  });

  const {
    openPortEditor,
    handleEnumChange: handleNodePortEnumChange,
    handleBooleanToggle: handleNodePortBooleanToggle,
    handleBooleanConstantToggle,
    handlePixelConstantToggle,
    handlePixelColorChange,
    handlePixelColorInput,
    handleNumericConstantToggle,
    handleNumericValueChange,
    clearNodePortValue,
    applyPortEditor,
    clearPortEditor,
    handleWindowKeydown: rawHandleWindowKeydown
  } = valueController;

  const handleWindowKeydown = createWindowKeydownHandler({
    interactive: () => interactive,
    isEditableTarget,
    getLastPointer: () => lastPointer,
    getLastFlowPointer: () => lastFlowPointer,
    toFlowPosition: (pos) => toFlowPositionRef(pos),
    dispatchContext: (payload) => dispatch('context', payload),
    rawHandleWindowKeydown
  });

  const { applyFocusHighlight, clearFocusHighlightTimer } = createFocusHighlighter({
    durationMs: FOCUS_HIGHLIGHT_DURATION_MS,
    setFocusHighlight: (next) => {
      focusHighlight = next;
    },
    getFocusHighlight: () => focusHighlight
  });

  const notifySelection = (nodeId: string | null, edge: EdgeSelection | null, nodes?: string[]) => {
    notifyGraphSelection({
      nodeId,
      edge,
      nodes,
      selectedEdgeId,
      setSelectedEdgeId: (next) => {
        selectedEdgeId = next;
      },
      setSelectedEdge: (next) => {
        selectedEdge = next;
      },
      dispatch,
      graphStore
    });
  };

  function commitPlan() {
    commitGraphPlan({
      interactive,
      history,
      plan: internalPlan,
      nodes,
      edges,
      edgeInteractionsEnabled: EDGE_INTERACTIONS_ENABLED,
      rebuildPlan,
      ensurePlanPortMetadata,
      setPlan: (next) => {
        internalPlan = next;
      },
      dispatchChange: (next) => {
        dispatch('change', { plan: next });
      }
    });
  }

  const interaction = createInteractionController({
    getPlan: () => internalPlan,
    setPlan: (plan) => (internalPlan = plan),
    getNodes: () => nodes,
    setNodes: (next) => (nodes = next),
    getEdges: () => edges,
    setEdges: (next) => (edges = next),
    getActiveConnection: () => activeConnection,
    setActiveConnection: (next) => (activeConnection = next),
    getSelectedEdgeId: () => selectedEdgeId,
    setSelectedEdgeId: (next) => (selectedEdgeId = next),
    getFlowContainer: () => flowContainer,
    getFlowViewport: () => flowViewport,
    notifySelection,
    dispatchContext: (payload) => dispatch('context', payload),
    commitPlan,
    interactive: () => interactive,
    edgeInteractionsEnabled: EDGE_INTERACTIONS_ENABLED,
    onNodeEdit: (nodeId) => dispatch('edit', { nodeId }),
    getLastNodeClick: () => lastNodeClick,
    setLastNodeClick: (next) => (lastNodeClick = next)
  });

  const {
    handleConnect,
    handleConnectStart,
    handleConnectEnd: baseHandleConnectEnd,
    handleReconnectStart,
    handleReconnectEnd,
    handleNodeDragStop: baseHandleNodeDragStop,
    handleNodeDrag,
    handleNodeClick,
    handlePaneClick,
    handleSelectionChange,
    handleEdgeClick,
    handleEdgeContextMenu,
    handlePaneContextMenu,
    handleNodeContextMenu,
    toFlowPosition
  } = interaction;
  toFlowPositionRef = toFlowPosition;

  const handleNodePortContextMenu = (
    nodeId: string,
    direction: 'input' | 'output',
    port: string,
    event: MouseEvent
  ) => {
    if (!interactive) return;
    event.preventDefault();
    event.stopPropagation();
    const client = { x: event.clientX, y: event.clientY };
    const flowPosition = toFlowPositionRef(client);
    dispatch('context', { type: 'port', nodeId, port, direction, position: client, flowPosition });
  };

  const { updateNodes, updateEdges, applyConnectionStyle } = createGraphUpdateController({
    getPlan: () => internalPlan,
    setPlan: (next) => {
      internalPlan = next;
    },
    getEdges: () => edges,
    setEdges: (next) => {
      edges = next;
    },
    getPreviousNodes: () => untrack(() => nodes),
    setNodes: (next) => {
      nodes = next;
    },
    getSelectedEdgeId: () => selectedEdgeId,
    getActiveConnection: () => activeConnection,
    buildNodeOptions: () => ({
      detailLevel: nodeDetailLevel,
      focusHighlight,
      resolveRegistryEntryForNode: (node) => resolveRegistryEntryForNode(node),
      onNodePortDoubleClick: (nodeId, direction, port, event) =>
        openPortEditor(nodeId, port, event, direction),
      onNodePortContextMenu: handleNodePortContextMenu,
      onNodePortEnumChange: handleNodePortEnumChange,
      onNodePortBooleanConstantToggle: handleBooleanConstantToggle,
      onNodePortBooleanToggle: handleNodePortBooleanToggle,
      onNodePortPixelConstantToggle: handlePixelConstantToggle,
      onNodePortPixelColorChange: handlePixelColorChange,
      onNodePortNumericConstantToggle: handleNumericConstantToggle,
      onNodePortNumericValueChange: handleNumericValueChange,
      onNodePortClear: ({ nodeId, port }) => clearNodePortValue(nodeId, port),
      onNodeRuntimeChange: (payload) => dispatch('runtime', payload),
      heatmap: heatmapForNodes,
      heatmapMode,
      diagnostics: graphDiagnostics,
      syncInspector,
      gpuOverlay,
      gpuOverlaySegments,
      searchTokens,
      runtimeWarnings
    }),
    resolveRegistryEntryForNode: (node) => resolveRegistryEntryForNode(node),
    edgeInteractionsEnabled: EDGE_INTERACTIONS_ENABLED,
    defaultStyle: DEFAULT_CONNECTION_STYLE,
    normalizeConnectionStyle,
    commitPlan
  });

  const applyEdgeStyle = (style: PipelineConnectionStyle) =>
    applyEdgeStyleSelection(selectedEdge, internalPlan, style, applyConnectionStyle);

  const handleConnectEnd: OnConnectEnd = createConnectEndHandler({
    baseHandleConnectEnd,
    edgeInteractionsEnabled: EDGE_INTERACTIONS_ENABLED,
    interactive: () => interactive,
    getPlan: () => internalPlan,
    setPlan: (plan) => (internalPlan = plan),
    updateNodes,
    handleConnect,
    toFlowPosition: (pos) => toFlowPositionRef(pos),
    findNodeAtFlowPosition: (pos) => findNodeAtFlowPosition(nodes, pos),
    resolveClientPosition,
    resolveRegistryEntryForNode: (node) => resolveRegistryEntryForNode(node)
  });

  const handleNodeDragStart = () => {
    isDragging = true;
  };

  const handleNodeDragStop: typeof baseHandleNodeDragStop = (event) => {
    baseHandleNodeDragStop(event);
    isDragging = false;
    if (!pendingPlan) {
      scheduleLayoutSnapshot();
      return;
    }
    const nextPlan = pendingPlan;
    pendingPlan = null;
    internalPlan = ensurePlanPortMetadata(clonePlan(nextPlan));
    history.pushSnapshot(internalPlan);
    scheduleLayoutSnapshot();
  };

  const handleMouseMove = (event: MouseEvent) => {
    const client = { x: event.clientX, y: event.clientY };
    lastPointer = client;
    lastFlowPointer = toFlowPositionRef(client);
  };

  const flushLayoutSnapshot = () => {
    layoutSnapshotFrame = null;
    const { hash, layout } = buildLayoutSnapshot(nodes);
    if (hash === lastLayoutHash) {
      return;
    }
    lastLayoutHash = hash;
    dispatch('layout', { nodes: layout });
  };

  const scheduleLayoutSnapshot = () => {
    if (isDragging) return;
    if (typeof window === 'undefined') {
      flushLayoutSnapshot();
      return;
    }
    if (layoutSnapshotFrame != null) {
      window.cancelAnimationFrame(layoutSnapshotFrame);
    }
    layoutSnapshotFrame = window.requestAnimationFrame(() => {
      flushLayoutSnapshot();
    });
  };

  const publishViewport = () => {
    viewportPublishFrame = null;
    const next = flowViewport;
    const previous = lastPublishedViewport;
    if (
      previous &&
      Math.abs(previous.x - next.x) < VIEWPORT_EPSILON &&
      Math.abs(previous.y - next.y) < VIEWPORT_EPSILON &&
      Math.abs(previous.zoom - next.zoom) < VIEWPORT_EPSILON
    ) {
      return;
    }
    lastPublishedViewport = { x: next.x, y: next.y, zoom: next.zoom };
    graphStore.setViewport(lastPublishedViewport);
  };

  const scheduleViewportPublish = () => {
    if (typeof window === 'undefined') {
      publishViewport();
      return;
    }
    if (viewportPublishFrame != null) {
      return;
    }
    viewportPublishFrame = window.requestAnimationFrame(() => {
      publishViewport();
    });
  };

  const scheduleInitialFit = () => {
    if (!initialFitPending || typeof window === 'undefined' || !flowApi?.fitView || nodes.length === 0) {
      return;
    }
    if (initialFitFrame != null) {
      return;
    }
    initialFitFrame = window.requestAnimationFrame(() => {
      initialFitFrame = window.requestAnimationFrame(() => {
        initialFitFrame = null;
        if (!initialFitPending || !flowApi?.fitView || nodes.length === 0) {
          return;
        }
        initialFitPending = false;
        void flowApi.fitView({ duration: 0, padding: 0.14 });
      });
    });
  };

  $effect(updateNodes);
  $effect(updateEdges);
  const heatmapScheduler = createHeatmapScheduler({
    refreshMs: HEATMAP_NODE_REFRESH_MS,
    apply: (payload) => {
      heatmapForNodes = payload;
    }
  });

  $effect(() => {
    heatmapScheduler.schedule(resolveHeatmapCandidate);
  });

  $effect(() => {
    if (plan === lastPlanRef) return;
    lastPlanRef = plan;
    if (isDragging) {
      pendingPlan = plan;
      return;
    }
    internalPlan = ensurePlanPortMetadata(clonePlan(plan));
    history.pushSnapshot(internalPlan);
    initialFitPending = true;
    if (pendingFocusRequest) {
      applyFocusRequest(pendingFocusRequest);
    }
  });

  $effect(() => {
    // Keep local edge selection in sync with controlled selectedEdgeId prop.
    if (!selectedEdgeId) {
      selectedEdge = null;
      return;
    }
    if (selectedEdge && selectedEdge.id === selectedEdgeId) {
      return;
    }
    const derived = deriveSelectionFromId(selectedEdgeId, edges, internalPlan);
    if (derived) {
      selectedEdge = derived;
    }
  });

  $effect(() => {
    void nodes;
    scheduleLayoutSnapshot();
  });

  const handleFlowApi = (event: CustomEvent<FlowApi>) => {
    flowApi = event.detail;
    scheduleInitialFit();
    if (pendingFocusRequest) {
      applyFocusRequest(pendingFocusRequest);
    }
  };

  function applyFocusRequest(request: FocusRequest) {
    applyGraphFocusRequest(request, {
      plan: internalPlan,
      nodes,
      flowApi,
      flowViewport,
      notifySelection,
      applyFocusHighlight,
      setPendingFocusRequest: (next) => {
        pendingFocusRequest = next;
      }
    });
  }

  export function focusOnNode(nodeId: string, options?: { port?: string | null }) {
    if (!nodeId) return;
    const request: FocusRequest = { nodeId, port: options?.port ?? null };
    applyFocusHighlight(nodeId, request.port ?? null);
    applyFocusRequest(request);
  }

  export function focusOnGraphCenter() {
    focusGraphCenter({ nodes, flowApi, flowViewport });
  }

  onDestroy(() => {
    clearFocusHighlightTimer();
    heatmapScheduler.dispose();
    if (layoutSnapshotFrame != null && typeof window !== 'undefined') {
      window.cancelAnimationFrame(layoutSnapshotFrame);
    }
    if (viewportPublishFrame != null && typeof window !== 'undefined') {
      window.cancelAnimationFrame(viewportPublishFrame);
    }
    if (initialFitFrame != null && typeof window !== 'undefined') {
      window.cancelAnimationFrame(initialFitFrame);
    }
    if (ownsGraphStore) {
      graphStore.destroy();
    }
  });

  $effect(() => {
    if (!flowViewport) return;
    scheduleViewportPublish();
  });

  $effect(() => {
    void nodes.length;
    scheduleInitialFit();
  });

  $effect(() => {
    const shouldKeepMinimalNodes =
      initialFitPending ||
      (graphNodeCount > MINIMAL_NODE_DETAIL_GRAPH_THRESHOLD && flowViewport.zoom < FULL_NODE_DETAIL_MIN_ZOOM);
    const nextDetailLevel = shouldKeepMinimalNodes ? 'minimal' : 'full';
    if (nodeDetailLevel !== nextDetailLevel) {
      nodeDetailLevel = nextDetailLevel;
    }
  });
</script>

<PipelineGraphEditorView
  {handleWindowKeydown}
  bind:flowContainer
  {className}
  containerStyle={`${containerHeightStyle}; --flow-surface:${resolvedTheme.surface}; --flow-surface-soft:${resolvedTheme.surfaceSoft}; --flow-border:${resolvedTheme.border}; --flow-text:${resolvedTheme.text}; --flow-accent:${resolvedTheme.accent}; --flow-edge-highlight:${resolvedTheme.edgeHighlight};`}
  {fluid}
  {handleMouseMove}
  bind:nodes={nodes}
  bind:edges={edges}
  bind:viewport={flowViewport}
  {interactive}
  {resolvedHeight}
  minZoom={VIEWPORT_MIN_ZOOM}
  maxZoom={VIEWPORT_MAX_ZOOM}
  {nodeTypes}
  {edgeTypes}
  backgroundVariant={resolvedTheme.backgroundVariant}
  backgroundGap={resolvedTheme.backgroundGap}
  backgroundSize={resolvedTheme.backgroundSize}
  backgroundColor={resolvedTheme.backgroundColor}
  surface={resolvedTheme.surface}
  showControls={resolvedTheme.showControls}
  controlsPosition={resolvedTheme.controlsPosition}
  handleConnectStart={EDGE_INTERACTIONS_ENABLED ? handleConnectStart : undefined}
  handleConnectEnd={EDGE_INTERACTIONS_ENABLED ? handleConnectEnd : undefined}
  handleConnect={EDGE_INTERACTIONS_ENABLED ? handleConnect : undefined}
  handleReconnectStart={EDGE_INTERACTIONS_ENABLED ? handleReconnectStart : undefined}
  handleReconnectEnd={EDGE_INTERACTIONS_ENABLED ? handleReconnectEnd : undefined}
  handleEdgeClick={EDGE_INTERACTIONS_ENABLED ? handleEdgeClick : undefined}
  handleEdgeContextMenu={EDGE_INTERACTIONS_ENABLED ? handleEdgeContextMenu : undefined}
  {handleNodeDragStart}
  {handleNodeDrag}
  {handleNodeDragStop}
  {handleNodeClick}
  {handlePaneClick}
  {handleSelectionChange}
  handlePaneContextMenu={handlePaneContextMenu}
  handleNodeContextMenu={handleNodeContextMenu}
  handleFlowApi={handleFlowApi}
  {selectedEdge}
  {selectedEdgeId}
  {edgeStyleOptions}
  selectedEdgeStyle={selectedEdgeStyle}
  applyEdgeStyle={applyEdgeStyle}
  {portEditor}
  portEditorDraft={portEditorDraft}
  portEditorError={portEditorError}
  {isPixelPortEditor}
  {pixelEditorHex}
  {pixelEditorAlpha}
  {applyPortEditor}
  {clearPortEditor}
  {closePortEditor}
  {handlePixelColorInput}
/>
