<script lang="ts">
  import type { Connection, Edge, EdgeTypes, Node, NodeEventWithPointer, NodeTargetEventWithPointer, NodeTypes } from '@xyflow/svelte';
  import type { OnConnectEnd, OnConnectStart, OnReconnectEnd, OnReconnectStart, Viewport } from '@xyflow/system';
  import type { PipelineConnectionRoute, PipelineConnectionStyle } from '$lib/types/pipeline';
  import type { EdgeSelection, PortEditorState } from './types';
  import GraphCanvas from './GraphCanvas.svelte';
  import GraphToolbar from './GraphToolbar.svelte';
  import GraphContextMenu from './GraphContextMenu.svelte';

  type Props = {
    container: HTMLDivElement | null;
    className: string;
    style: string;
    fluid: boolean;
    onMouseMove: (event: MouseEvent) => void;
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
    controlsPosition: 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right';
    onConnectStart?: OnConnectStart;
    onConnectEnd?: OnConnectEnd;
    onConnect?: (connection: Connection) => void;
    onReconnectStart?: OnReconnectStart;
    onReconnectEnd?: OnReconnectEnd;
    onEdgeClick?: ({ edge, event }: { edge: Edge; event: MouseEvent }) => void;
    onEdgeContextMenu?: ({ edge, event }: { edge: Edge; event: MouseEvent }) => void;
    onNodeDragStart?: NodeTargetEventWithPointer<MouseEvent | TouchEvent, Node>;
    onNodeDrag?: NodeTargetEventWithPointer<MouseEvent | TouchEvent, Node>;
    onNodeDragStop?: NodeTargetEventWithPointer<MouseEvent | TouchEvent, Node>;
    onNodeClick?: NodeEventWithPointer<MouseEvent | TouchEvent, Node>;
    onPaneClick?: ({ event }: { event: MouseEvent }) => void;
    onSelectionChange?: (event: { nodes: Node[]; edges: Edge[] }) => void;
    onPaneContextMenu?: ({ event }: { event: MouseEvent }) => void;
    onNodeContextMenu?: NodeEventWithPointer<MouseEvent, Node>;
    onApi?: (event: CustomEvent<any>) => void;
    selectedEdge: EdgeSelection | null;
    selectedEdgeId: string | null;
    edgeStyleOptions: Array<{ id: PipelineConnectionRoute; label: string; icon: string }>;
    currentStyle: PipelineConnectionStyle;
    onApplyStyle: (style: PipelineConnectionStyle) => void;
    portEditor: PortEditorState | null;
    draft: string;
    error: string | null;
    isPixelPortEditor: boolean;
    pixelEditorHex: string;
    pixelEditorAlpha: number | null | undefined;
    onApply: () => void;
    onClear: () => void;
    onClose: () => void;
    onColorInput: (event: Event) => void;
  };

  let {
    container = $bindable(null),
    className,
    style,
    fluid,
    onMouseMove,
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
    onConnectStart,
    onConnectEnd,
    onConnect,
    onReconnectStart,
    onReconnectEnd,
    onEdgeClick,
    onEdgeContextMenu,
    onNodeDragStart,
    onNodeDrag,
    onNodeDragStop,
    onNodeClick,
    onPaneClick,
    onSelectionChange,
    onPaneContextMenu,
    onNodeContextMenu,
    onApi,
    selectedEdge,
    selectedEdgeId,
    edgeStyleOptions,
    currentStyle,
    onApplyStyle,
    portEditor,
    draft,
    error,
    isPixelPortEditor,
    pixelEditorHex,
    pixelEditorAlpha,
    onApply,
    onClear,
    onClose,
    onColorInput
  }: Props = $props();
</script>

<div
  class={`pipeline-flow relative rounded border border-surface-800 bg-surface-900/80 shadow-lg shadow-black/20 ${fluid ? 'h-full min-h-80' : ''} ${className}`.trim()}
  style={style}
  bind:this={container}
  role="presentation"
  onmousemove={onMouseMove}
>
  <GraphCanvas
    bind:nodes={nodes}
    bind:edges={edges}
    bind:viewport={viewport}
    {interactive}
    {resolvedHeight}
    {fluid}
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
    showMinimap={false}
    {onConnectStart}
    {onConnectEnd}
    {onConnect}
    {onReconnectStart}
    {onReconnectEnd}
    {onEdgeClick}
    {onEdgeContextMenu}
    {onNodeDragStart}
    {onNodeDrag}
    {onNodeDragStop}
    {onNodeClick}
    {onPaneClick}
    {onSelectionChange}
    {onPaneContextMenu}
    {onNodeContextMenu}
    {onApi}
  />
  <GraphToolbar
    {selectedEdge}
    {selectedEdgeId}
    {edgeStyleOptions}
    currentStyle={currentStyle}
    onApplyStyle={onApplyStyle}
  />
  <GraphContextMenu
    {portEditor}
    bind:draft={draft}
    {error}
    {isPixelPortEditor}
    {pixelEditorHex}
    {pixelEditorAlpha}
    onApply={onApply}
    onClear={onClear}
    onClose={onClose}
    onColorInput={onColorInput}
  />
</div>
