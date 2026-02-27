<script lang="ts">
  import {
    Background,
    Controls,
    SvelteFlow,
    type Edge,
    type Node
  } from '@xyflow/svelte';
  import type {
    OnConnectEnd,
    OnConnectStart,
    OnReconnectEnd,
    OnReconnectStart,
    Viewport
  } from '@xyflow/system';
  import FlowViewportHelper from '../FlowViewportHelper.svelte';
  import GraphMinimap from './GraphMinimap.svelte';
  import type { EdgeTypes, NodeTypes, Connection, NodeEventWithPointer, NodeTargetEventWithPointer } from '@xyflow/svelte';

  type Props = {
    nodes: Node[];
    edges: Edge[];
    viewport: Viewport;
    interactive: boolean;
    resolvedHeight: number;
    fluid: boolean;
    minZoom: number;
    maxZoom: number;
    nodeTypes: NodeTypes;
    edgeTypes: EdgeTypes;
    backgroundVariant: any;
    backgroundGap: number;
    backgroundSize: number;
    backgroundColor: string;
    surface: string;
    showControls: boolean;
    controlsPosition: 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right';
    showMinimap?: boolean;
    minimapPosition?: 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right';
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
  };

  let {
    nodes = $bindable([]),
    edges = $bindable([]),
    viewport = $bindable({ x: 0, y: 0, zoom: 1 }),
    interactive,
    resolvedHeight,
    fluid,
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
    showMinimap = false,
    minimapPosition = 'bottom-left',
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
    onApi
  }: Props = $props();
</script>

<SvelteFlow
  bind:nodes={nodes}
  bind:edges={edges}
  bind:viewport={viewport}
  class="h-full"
  colorMode="dark"
  fitView
  height={fluid ? undefined : resolvedHeight}
  minZoom={minZoom}
  maxZoom={maxZoom}
  nodesDraggable={interactive}
  nodesConnectable={interactive}
  elementsSelectable
  panOnDrag
  {nodeTypes}
  {edgeTypes}
  onconnectstart={onConnectStart}
  onconnectend={onConnectEnd}
  onconnect={onConnect}
  onreconnectstart={onReconnectStart}
  onreconnectend={onReconnectEnd}
  onedgeclick={onEdgeClick}
  onedgecontextmenu={onEdgeContextMenu}
  onnodedragstart={onNodeDragStart}
  onnodedrag={onNodeDrag}
  onnodedragstop={onNodeDragStop}
  onnodeclick={onNodeClick}
  onpaneclick={onPaneClick}
  onselectionchange={onSelectionChange}
  onpanecontextmenu={onPaneContextMenu}
  onnodecontextmenu={onNodeContextMenu}
>
  <Background
    variant={backgroundVariant}
    gap={backgroundGap}
    size={backgroundSize}
    bgColor={surface}
    patternColor={backgroundColor}
  />
  <FlowViewportHelper on:api={onApi} />
  {#if showControls}
    <Controls position={controlsPosition} />
  {/if}
  <GraphMinimap show={showMinimap} position={minimapPosition} />
</SvelteFlow>
