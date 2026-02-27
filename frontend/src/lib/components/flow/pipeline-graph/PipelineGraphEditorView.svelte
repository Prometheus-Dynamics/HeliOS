<script lang="ts">
  import type {
    Connection,
    Edge,
    EdgeTypes,
    Node,
    NodeEventWithPointer,
    NodeTargetEventWithPointer,
    NodeTypes
  } from '@xyflow/svelte';
  import type { OnConnectEnd, OnConnectStart, OnReconnectEnd, OnReconnectStart, Viewport } from '@xyflow/system';
  import GraphStage from './GraphStage.svelte';
  import type { EdgeSelection, PortEditorState } from './types';
  import type { PipelineConnectionRoute, PipelineConnectionStyle } from '$lib/types/pipeline';

  type Props = {
    handleWindowKeydown: (event: KeyboardEvent) => void;
    flowContainer: HTMLDivElement | null;
    className: string;
    containerStyle: string;
    fluid: boolean;
    handleMouseMove: (event: MouseEvent) => void;
    nodes: Node[];
    edges: Edge[];
    viewport: Viewport;
    interactive: boolean;
    resolvedHeight: number;
    minZoom: number;
    maxZoom: number;
    nodeTypes: NodeTypes;
    edgeTypes: EdgeTypes;
    backgroundVariant: string;
    backgroundGap: number;
    backgroundSize: number;
    backgroundColor: string;
    surface: string;
    showControls: boolean;
    controlsPosition: 'top-right' | 'top-left' | 'bottom-right' | 'bottom-left';
    handleConnectStart?: OnConnectStart;
    handleConnectEnd?: OnConnectEnd;
    handleConnect?: (connection: Connection) => void;
    handleReconnectStart?: OnReconnectStart;
    handleReconnectEnd?: OnReconnectEnd;
    handleEdgeClick?: ({ edge, event }: { edge: Edge; event: MouseEvent }) => void;
    handleEdgeContextMenu?: ({ edge, event }: { edge: Edge; event: MouseEvent }) => void;
    handleNodeDragStart?: NodeTargetEventWithPointer<MouseEvent | TouchEvent, Node>;
    handleNodeDrag?: NodeTargetEventWithPointer<MouseEvent | TouchEvent, Node>;
    handleNodeDragStop?: NodeTargetEventWithPointer<MouseEvent | TouchEvent, Node>;
    handleNodeClick?: NodeEventWithPointer<MouseEvent | TouchEvent, Node>;
    handlePaneClick?: ({ event }: { event: MouseEvent }) => void;
    handleSelectionChange?: (event: { nodes: Node[]; edges: Edge[] }) => void;
    handlePaneContextMenu?: ({ event }: { event: MouseEvent }) => void;
    handleNodeContextMenu?: NodeEventWithPointer<MouseEvent, Node>;
    handleFlowApi?: (event: CustomEvent<any>) => void;
    selectedEdge: EdgeSelection | null;
    selectedEdgeId: string | null;
    edgeStyleOptions: Array<{ id: PipelineConnectionRoute; label: string; icon: string }>;
    selectedEdgeStyle: PipelineConnectionStyle;
    applyEdgeStyle: (style: PipelineConnectionStyle) => void;
    portEditor: PortEditorState | null;
    portEditorDraft: string;
    portEditorError: string | null;
    isPixelPortEditor: boolean;
    pixelEditorHex: string;
    pixelEditorAlpha: number;
    applyPortEditor: () => void;
    clearPortEditor: () => void;
    closePortEditor: () => void;
    handlePixelColorInput: (event: Event) => void;
  };

  let {
    handleWindowKeydown,
    flowContainer = $bindable(null),
    className,
    containerStyle,
    fluid,
    handleMouseMove,
    nodes = $bindable([]),
    edges = $bindable([]),
    viewport = $bindable({ x: 0, y: 0, zoom: 1 }),
    interactive,
    resolvedHeight,
    minZoom,
    maxZoom,
    nodeTypes,
    edgeTypes,
    backgroundVariant,
    backgroundGap,
    backgroundSize,
    backgroundColor,
    surface,
    showControls,
    controlsPosition,
    handleConnectStart,
    handleConnectEnd,
    handleConnect,
    handleReconnectStart,
    handleReconnectEnd,
    handleEdgeClick,
    handleEdgeContextMenu,
    handleNodeDragStart,
    handleNodeDrag,
    handleNodeDragStop,
    handleNodeClick,
    handlePaneClick,
    handleSelectionChange,
    handlePaneContextMenu,
    handleNodeContextMenu,
    handleFlowApi,
    selectedEdge,
    selectedEdgeId,
    edgeStyleOptions,
    selectedEdgeStyle,
    applyEdgeStyle,
    portEditor,
    portEditorDraft,
    portEditorError,
    isPixelPortEditor,
    pixelEditorHex,
    pixelEditorAlpha,
    applyPortEditor,
    clearPortEditor,
    closePortEditor,
    handlePixelColorInput
  }: Props = $props();
</script>

<svelte:window on:keydown|capture={handleWindowKeydown} />

<GraphStage
  bind:container={flowContainer}
  className={className}
  style={containerStyle}
  {fluid}
  onMouseMove={handleMouseMove}
  bind:nodes={nodes}
  bind:edges={edges}
  bind:viewport={viewport}
  {interactive}
  {resolvedHeight}
  {minZoom}
  {maxZoom}
  {nodeTypes}
  {edgeTypes}
  {backgroundVariant}
  {backgroundGap}
  {backgroundSize}
  {backgroundColor}
  {surface}
  {showControls}
  {controlsPosition}
  onConnectStart={handleConnectStart}
  onConnectEnd={handleConnectEnd}
  onConnect={handleConnect}
  onReconnectStart={handleReconnectStart}
  onReconnectEnd={handleReconnectEnd}
  onEdgeClick={handleEdgeClick}
  onEdgeContextMenu={handleEdgeContextMenu}
  onNodeDragStart={handleNodeDragStart}
  onNodeDrag={handleNodeDrag}
  onNodeDragStop={handleNodeDragStop}
  onNodeClick={handleNodeClick}
  onPaneClick={handlePaneClick}
  onSelectionChange={handleSelectionChange}
  onPaneContextMenu={handlePaneContextMenu}
  onNodeContextMenu={handleNodeContextMenu}
  onApi={handleFlowApi}
  {selectedEdge}
  {selectedEdgeId}
  {edgeStyleOptions}
  currentStyle={selectedEdgeStyle}
  onApplyStyle={applyEdgeStyle}
  {portEditor}
  draft={portEditorDraft}
  error={portEditorError}
  {isPixelPortEditor}
  {pixelEditorHex}
  {pixelEditorAlpha}
  onApply={applyPortEditor}
  onClear={clearPortEditor}
  onClose={closePortEditor}
  onColorInput={handlePixelColorInput}
/>

<style>
  @import '../PipelineGraphEditor.css';
</style>
