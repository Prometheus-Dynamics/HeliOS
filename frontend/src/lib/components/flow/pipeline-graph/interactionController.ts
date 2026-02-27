import { edgeClassName, resolveEdgeFromColor } from './edges';
import { extractPortFromHandle, getPortType, parseHandleId, typeKey } from './utils';
import type { PipelineGraphPlan } from '$lib/types/pipeline';
import type {
  ActiveConnection,
  EdgeSelection,
  PipelineEdgeData
} from './types';
import { DEFAULT_CONNECTION_STYLE } from './edgeStyle';
import type {
  Connection,
  Edge,
  Node,
  NodeEventWithPointer,
  NodeTargetEventWithPointer
} from '@xyflow/svelte';
import type {
  HandleType,
  OnConnectEnd,
  OnConnectStart,
  OnReconnectEnd,
  OnReconnectStart,
  Viewport,
  XYPosition
} from '@xyflow/system';

type Getter<T> = () => T;
type Setter<T> = (value: T) => void;

export const createInteractionController = (deps: {
  getPlan: Getter<PipelineGraphPlan>;
  setPlan: Setter<PipelineGraphPlan>;
  getNodes: Getter<Node[]>;
  setNodes: Setter<Node[]>;
  getEdges: Getter<Edge[]>;
  setEdges: Setter<Edge[]>;
  getActiveConnection: Getter<ActiveConnection | null>;
  setActiveConnection: Setter<ActiveConnection | null>;
  getSelectedEdgeId: Getter<string | null>;
  setSelectedEdgeId: Setter<string | null>;
  getFlowContainer: Getter<HTMLDivElement | null>;
  getFlowViewport: Getter<Viewport | undefined>;
  notifySelection: (nodeId: string | null, edge: EdgeSelection | null, nodes?: string[]) => void;
  dispatchContext: (payload: {
    type: 'pane' | 'node';
    position: XYPosition;
    flowPosition: XYPosition;
    nodeId?: string | null;
  }) => void;
  commitPlan: () => void;
  interactive: () => boolean;
  edgeInteractionsEnabled: boolean;
  onNodeEdit: (nodeId: string) => void;
  getLastNodeClick: Getter<{ id: string | null; timestamp: number }>;
  setLastNodeClick: Setter<{ id: string | null; timestamp: number }>;
}) => {
  const setActiveConnectionFromHandle = (handleId: string | null, handleType: HandleType | null) => {
    if (!deps.edgeInteractionsEnabled) {
      deps.setActiveConnection(null);
      return;
    }
    if (!handleId || !handleType) {
      deps.setActiveConnection(null);
      return;
    }
    const parsed = parseHandleId(handleId);
    if (!parsed) {
      deps.setActiveConnection(null);
      return;
    }
    const port = extractPortFromHandle(handleId) ?? parsed.port;
    const portType = getPortType(deps.getPlan(), parsed.node, port, handleType);
    deps.setActiveConnection({
      handleType,
      typeKey: typeKey(portType)
    });
  };

  const flowEdgeToSelection = (edge: Edge, plan: PipelineGraphPlan): EdgeSelection | null => {
    const data = edge.data as PipelineEdgeData | undefined;
    const selectionId = data?.edgeId ?? edge.id ?? null;
    const fromPort = data?.fromPort ?? extractPortFromHandle(edge.sourceHandle ?? null);
    const toPort = data?.toPort ?? extractPortFromHandle(edge.targetHandle ?? null);
    const fromNodeId = edge.source ?? null;
    const toNodeId = edge.target ?? null;
    if (!selectionId || !fromNodeId || !toNodeId || !fromPort || !toPort) {
      return null;
    }
    const signature = `${fromNodeId}:${fromPort}->${toNodeId}:${toPort}`;
    const matched = plan.connections.find(
      (connection) => `${connection.from.node}:${connection.from.port}->${connection.to.node}:${connection.to.port}` === signature
    );
    const fromType = matched
      ? getPortType(plan, matched.from.node, matched.from.port, 'source')
      : data?.fromType;
    const toType = matched ? getPortType(plan, matched.to.node, matched.to.port, 'target') : data?.toType;
    return {
      id: selectionId,
      from: { node: fromNodeId, port: fromPort, dataType: fromType },
      to: { node: toNodeId, port: toPort, dataType: toType }
    };
  };

  const handleConnect = (connection: Connection) => {
    if (!deps.edgeInteractionsEnabled || !deps.interactive()) return;
    const fromPort = extractPortFromHandle(connection.sourceHandle ?? null);
    const toPort = extractPortFromHandle(connection.targetHandle ?? null);
    if (!connection.source || !connection.target || !fromPort || !toPort) {
      return;
    }
    const edges = deps.getEdges();
    const duplicateEdge = edges.some((edge) => {
      const data = edge.data as PipelineEdgeData | undefined;
      return (
        edge.source === connection.source &&
        edge.target === connection.target &&
        data?.fromPort === fromPort &&
        data?.toPort === toPort
      );
    });
    if (duplicateEdge) {
      return;
    }
    const edgeId = `edge-${connection.source}-${connection.target}-${Date.now()}`;
    const fromTypeUpdated = getPortType(deps.getPlan(), connection.source, fromPort, 'source');
    const toTypeUpdated = getPortType(deps.getPlan(), connection.target, toPort, 'target');
    const fromColor = resolveEdgeFromColor(deps.getPlan(), connection.source, fromPort);
    const edgeData: PipelineEdgeData = {
      edgeId,
      fromPort,
      toPort,
      fromType: fromTypeUpdated,
      toType: toTypeUpdated,
      fromColor,
      style: { ...DEFAULT_CONNECTION_STYLE }
    };
    const newEdge: Edge = {
      id: edgeId,
      source: connection.source,
      target: connection.target,
      type: 'pipeline',
      animated: true,
      data: edgeData,
      class: edgeClassName(deps.getActiveConnection(), edgeData, false),
      selected: false,
      selectable: true,
      interactionWidth: 84
    };
    if (connection.sourceHandle) {
      newEdge.sourceHandle = connection.sourceHandle;
    }
    if (connection.targetHandle) {
      newEdge.targetHandle = connection.targetHandle;
    }
    deps.setEdges([...edges, newEdge]);
    deps.commitPlan();
  };

  const handleConnectStart: OnConnectStart = (_event, params) => {
    if (!deps.edgeInteractionsEnabled || !deps.interactive()) return;
    setActiveConnectionFromHandle(params.handleId, params.handleType);
  };

  const handleConnectEnd: OnConnectEnd = () => {
    if (!deps.edgeInteractionsEnabled) return;
    deps.setActiveConnection(null);
  };

  const handleReconnectStart: OnReconnectStart<Edge> = (_event, edge, handleType) => {
    if (!deps.edgeInteractionsEnabled || !deps.interactive()) return;
    const edgeData = edge.data as PipelineEdgeData | undefined;
    const portType = handleType === 'source' ? edgeData?.fromType : edgeData?.toType;
    deps.setActiveConnection({
      handleType,
      typeKey: typeKey(portType)
    });
  };

  const handleReconnectEnd: OnReconnectEnd<Edge> = () => {
    if (!deps.edgeInteractionsEnabled) return;
    deps.setActiveConnection(null);
  };

  const handleNodeDragStop: NodeTargetEventWithPointer<MouseEvent | TouchEvent, Node> = () => {
    // XYFlow can occasionally emit drag-stop for click/tap interactions.
    // Only commit when something actually moved to avoid spurious dirty state.
    const plan = deps.getPlan();
    const flowNodes = deps.getNodes();
    const moved = flowNodes.some((node) => {
      const planNode = plan.nodes?.[node.id];
      if (!planNode) return false;
      const currentX = planNode.info?.location?.x ?? 0;
      const currentY = planNode.info?.location?.y ?? 0;
      return node.position.x !== currentX || node.position.y !== currentY;
    });
    if (moved) {
      deps.commitPlan();
    }
  };

  const handleNodeDrag: NodeTargetEventWithPointer<MouseEvent | TouchEvent, Node> = ({ nodes: dragNodes }) => {
    const currentNodes = deps.getNodes();
    const dragMap = new Map(dragNodes.map((node) => [node.id, node]));
    const updated = currentNodes.map((node) => {
      const dragged = dragMap.get(node.id);
      if (!dragged) {
        return node;
      }
      return {
        ...node,
        position: { ...dragged.position }
      };
    });
    const updatedIds = new Set(updated.map((node) => node.id));
    for (const [id, dragged] of dragMap.entries()) {
      if (updatedIds.has(id)) continue;
      updated.push({ ...dragged });
      updatedIds.add(id);
    }
    deps.setNodes(updated);
  };

  const handleNodeClick: NodeEventWithPointer<MouseEvent | TouchEvent, Node> = ({ node, event }) => {
    const now = typeof performance !== 'undefined' ? performance.now() : Date.now();
    const nodeId = node?.id ?? null;
    const target = (event?.target ?? null) as HTMLElement | null;
    if (target && target.closest('.pipeline-port')) {
      deps.notifySelection(nodeId, null, nodeId ? [nodeId] : []);
      deps.setLastNodeClick({ id: null, timestamp: 0 });
      return;
    }
    const lastClick = deps.getLastNodeClick();
    const isDoubleClick = nodeId && lastClick.id === nodeId && now - lastClick.timestamp < 320;
    deps.notifySelection(nodeId, null, nodeId ? [nodeId] : []);
    if (isDoubleClick && nodeId) {
      deps.onNodeEdit(nodeId);
    }
    deps.setLastNodeClick({ id: nodeId, timestamp: now });
  };

  const handlePaneClick = () => {
    deps.notifySelection(null, null, []);
  };

  const handleSelectionChange = (params: { nodes: Node[]; edges: Edge[] }) => {
    const { nodes: selectedNodes, edges: selectedEdges } = params;
    const selectedIds = selectedNodes.map((entry) => entry.id).filter(Boolean);
    const plan = deps.getPlan();
    if (selectedEdges.length > 0) {
      const lastEdge = selectedEdges[selectedEdges.length - 1];
      const selection = flowEdgeToSelection(lastEdge, plan);
      deps.notifySelection(null, selection, selectedIds);
      return;
    }
    if (selectedNodes.length > 0) {
      const lastNode = selectedNodes[selectedNodes.length - 1];
      deps.notifySelection(lastNode?.id ?? null, null, selectedIds);
      return;
    }
    deps.notifySelection(null, null, []);
  };

  const handleEdgeClick = ({ edge }: { edge: Edge; event: MouseEvent }) => {
    if (!deps.edgeInteractionsEnabled) return;
    const selection = flowEdgeToSelection(edge, deps.getPlan());
    deps.notifySelection(null, selection);
  };

  const handleEdgeContextMenu = ({ edge, event }: { edge: Edge; event: MouseEvent }) => {
    if (!deps.edgeInteractionsEnabled) return;
    event.preventDefault();
    event.stopPropagation();
    const selection = flowEdgeToSelection(edge, deps.getPlan());
    deps.notifySelection(null, selection);
  };

  const toFlowPosition = (client: XYPosition, viewport = deps.getFlowViewport()): XYPosition => {
    const flowContainer = deps.getFlowContainer();
    if (!flowContainer || !viewport) return client;
    const bounds = flowContainer.getBoundingClientRect();
    const corrected = {
      x: client.x - bounds.left,
      y: client.y - bounds.top
    };
    return {
      x: (corrected.x - viewport.x) / viewport.zoom,
      y: (corrected.y - viewport.y) / viewport.zoom
    };
  };

  const handlePaneContextMenu = ({ event }: { event: MouseEvent }) => {
    if (!deps.interactive()) return;
    event.preventDefault();
    deps.notifySelection(null, null);
    const client = { x: event.clientX, y: event.clientY };
    const flowPosition = toFlowPosition(client);
    deps.dispatchContext({
      type: 'pane',
      position: client,
      flowPosition
    });
  };

  const handleNodeContextMenu: NodeEventWithPointer<MouseEvent, Node> = ({ node, event }) => {
    if (!node?.id || !deps.interactive()) return;
    event.preventDefault();
    const client = { x: event.clientX, y: event.clientY };
    const flowPosition = toFlowPosition(client);
    deps.dispatchContext({
      type: 'node',
      position: client,
      flowPosition,
      nodeId: node.id
    });
  };

  return {
    handleConnect,
    handleConnectStart,
    handleConnectEnd,
    handleReconnectStart,
    handleReconnectEnd,
    handleNodeDragStop,
    handleNodeDrag,
    handleNodeClick,
    handlePaneClick,
    handleSelectionChange,
    handleEdgeClick,
    handleEdgeContextMenu,
    handlePaneContextMenu,
    handleNodeContextMenu,
    setActiveConnectionFromHandle,
    toFlowPosition
  };
};
