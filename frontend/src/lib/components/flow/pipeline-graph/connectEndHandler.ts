import type { Connection } from '@xyflow/svelte';
import type { OnConnectEnd, XYPosition } from '@xyflow/system';
import type { PipelineGraphPlan } from '$lib/types/pipeline';
import { normalizeImageFlavorType, resolveDataTypeKey } from '$lib/features/pipelines/valueFormatting';
import {
  buildPortOrders,
  cloneDataType,
  extractPortFromHandle,
  getPortType,
  makeHandleId
} from './utils';
import { deriveAutoPortName, resolveBoundaryDirection } from './editorHelpers';
import type { PipelineRegistryEntry } from '$lib/types/pipeline';

export function createConnectEndHandler(options: {
  baseHandleConnectEnd: OnConnectEnd;
  edgeInteractionsEnabled: boolean;
  interactive: () => boolean;
  getPlan: () => PipelineGraphPlan;
  setPlan: (plan: PipelineGraphPlan) => void;
  updateNodes: () => void;
  handleConnect: (connection: Connection) => void;
  toFlowPosition: (pos: XYPosition) => XYPosition;
  findNodeAtFlowPosition: (pos: XYPosition) => { id: string } | null;
  resolveClientPosition: (event: MouseEvent | TouchEvent) => XYPosition | null;
  resolveRegistryEntryForNode?: (node: PipelineGraphPlan['nodes'][string]) => PipelineRegistryEntry | null;
}): OnConnectEnd {
  return (event, connectionState) => {
    const {
      baseHandleConnectEnd,
      edgeInteractionsEnabled,
      interactive,
      getPlan,
      setPlan,
      updateNodes,
      handleConnect,
      toFlowPosition,
      findNodeAtFlowPosition,
      resolveClientPosition,
      resolveRegistryEntryForNode
    } = options;

    baseHandleConnectEnd(event, connectionState);
    if (!edgeInteractionsEnabled || !interactive()) return;
    if (!connectionState?.fromHandle) return;
    if (connectionState.toHandle) return;

    const fromHandle = connectionState.fromHandle;
    const fromPort = extractPortFromHandle(fromHandle.id ?? null);
    if (!fromPort) return;

    const fromNodeId = fromHandle.nodeId ?? connectionState.fromNode?.id;
    let toNodeId = connectionState.toNode?.id ?? null;
    if (!toNodeId) {
      const client = resolveClientPosition(event);
      if (client) {
        const flowPosition = toFlowPosition(client);
        const hit = findNodeAtFlowPosition(flowPosition);
        toNodeId = hit?.id ?? null;
      }
    }
    if (!fromNodeId || !toNodeId) return;

    const plan = getPlan();
    const targetNode = plan.nodes?.[toNodeId];
    if (!targetNode) return;

    const direction = resolveBoundaryDirection(targetNode);
    if (!direction) return;
    if (direction === 'input' && fromHandle.type !== 'target') return;
    if (direction === 'output' && fromHandle.type !== 'source') return;

    const pipelineRecord =
      direction === 'input' ? plan.pipelineInputs ?? {} : plan.pipelineOutputs ?? {};
    const nodeRecord = direction === 'input' ? targetNode.outputs ?? {} : targetNode.inputs ?? {};
    const nameSource = { ...pipelineRecord, ...nodeRecord };
    const nextPortName = deriveAutoPortName(fromPort, direction, nameSource);

    const resolveRegistryPortType = (): PipelineGraphPlan['pipelineInputs'][string] | undefined => {
      if (!resolveRegistryEntryForNode) return undefined;
      const node = plan.nodes?.[fromNodeId];
      if (!node) return undefined;
      const entry = resolveRegistryEntryForNode(node);
      if (!entry) return undefined;
      const record = fromHandle.type === 'source' ? entry.outputs : entry.inputs;
      if (!record) return undefined;
      if (Object.prototype.hasOwnProperty.call(record, fromPort)) {
        return record[fromPort];
      }
      const normalized = fromPort.trim().toLowerCase();
      if (Object.prototype.hasOwnProperty.call(record, normalized)) {
        return record[normalized];
      }
      const fallback = Object.keys(record).find((key) => key.trim().toLowerCase() === normalized);
      return fallback ? record[fallback] : undefined;
    };

    const resolvedType = getPortType(plan, fromNodeId, fromPort, fromHandle.type);
    const registryType = resolveRegistryPortType();
    const resolvedKey = resolveDataTypeKey(resolvedType)?.toLowerCase() ?? null;
    const registryKey = resolveDataTypeKey(registryType)?.toLowerCase() ?? null;
    const hasPaletteInfo = (value: unknown): boolean => {
      if (!value || typeof value !== 'object') return false;
      const record = value as {
        label?: string | null;
        color?: string | null;
        descriptor?: { label?: string | null; color?: string | null };
      };
      return Boolean(
        (record.label && record.label.trim()) ||
          (record.color && record.color.trim()) ||
          (record.descriptor?.label && record.descriptor.label.trim()) ||
          (record.descriptor?.color && record.descriptor.color.trim())
      );
    };
    const shouldUpgrade =
      registryType &&
      typeof registryType === 'object' &&
      (resolvedType == null ||
        typeof resolvedType === 'string' ||
        !hasPaletteInfo(resolvedType)) &&
      (!resolvedKey || !registryKey || resolvedKey === registryKey);
    const inferredType =
      normalizeImageFlavorType(
        cloneDataType((shouldUpgrade ? registryType : resolvedType) ?? registryType ?? 'Generic') ?? 'Generic'
      ) ?? 'Generic';

    const nextNodes = { ...(plan.nodes ?? {}) };
    const nextTarget = { ...targetNode };
    if (direction === 'input') {
      nextTarget.outputs = { ...(targetNode.outputs ?? {}), [nextPortName]: inferredType };
      nextTarget.inputs = targetNode.inputs ?? {};
    } else {
      nextTarget.inputs = { ...(targetNode.inputs ?? {}), [nextPortName]: inferredType };
      nextTarget.outputs = targetNode.outputs ?? {};
    }
    nextNodes[toNodeId] = nextTarget;

    const nextPlan: PipelineGraphPlan = {
      ...plan,
      nodes: nextNodes,
      pipelineInputs:
        direction === 'input'
          ? { ...(plan.pipelineInputs ?? {}), [nextPortName]: inferredType }
          : plan.pipelineInputs,
      pipelineOutputs:
        direction === 'output'
          ? { ...(plan.pipelineOutputs ?? {}), [nextPortName]: inferredType }
          : plan.pipelineOutputs
    };

    setPlan(nextPlan);
    updateNodes();

    const portOrders = buildPortOrders(nextPlan);
    const boundaryHandleId = makeHandleId(
      nextPlan,
      portOrders,
      { node: toNodeId, port: nextPortName },
      direction === 'input' ? 'source' : 'target'
    );
    if (!boundaryHandleId) return;

    const fromHandleId =
      fromHandle.id ?? makeHandleId(nextPlan, portOrders, { node: fromNodeId, port: fromPort }, fromHandle.type);
    if (!fromHandleId) return;

    const connection: Connection =
      direction === 'input'
        ? {
            source: toNodeId,
            sourceHandle: boundaryHandleId,
            target: fromNodeId,
            targetHandle: fromHandleId
          }
        : {
            source: fromNodeId,
            sourceHandle: fromHandleId,
            target: toNodeId,
            targetHandle: boundaryHandleId
          };

    handleConnect(connection);
  };
}
