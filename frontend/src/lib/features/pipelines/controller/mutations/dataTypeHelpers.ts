import type { PipelineDataType, PipelineEndpoint, PipelineGraphPlan } from '$lib/types/pipeline';
import { normalizePipelinePortName } from '../../boundary';

const GENERIC_TYPE_KEYS = new Set(['generic', 'any', 'unknown', 'dynamic']);

const isSpecificDataType = (
  resolveDataTypeKey: (dataType: PipelineDataType | null | undefined) => string | null,
  dataType: PipelineDataType | undefined | null
): dataType is PipelineDataType => {
  if (!dataType) return false;
  const key = resolveDataTypeKey(dataType)?.toLowerCase();
  if (!key) return false;
  return !GENERIC_TYPE_KEYS.has(key);
};

export const dataTypeForConnection = (
  plan: PipelineGraphPlan,
  endpoint: PipelineEndpoint,
  direction: 'in' | 'out'
): PipelineDataType => {
  const node = plan.nodes?.[endpoint.node];
  if (!node) return 'generic';
  const ports = direction === 'in' ? node.inputs ?? {} : node.outputs ?? {};
  return ports[endpoint.port] ?? 'generic';
};

export const inferPortDataType = (params: {
  plan: PipelineGraphPlan | null | undefined;
  nodeId: string;
  port: string;
  direction: 'input' | 'output';
  cloneDataType: (value: PipelineDataType) => PipelineDataType;
  resolveDataTypeKey: (dataType: PipelineDataType | null | undefined) => string | null;
}): PipelineDataType => {
  const { plan, nodeId, port, direction, cloneDataType, resolveDataTypeKey } = params;
  if (!plan) return 'Generic';
  const normalizedPort = normalizePipelinePortName(port);
  const node = plan.nodes?.[nodeId];
  const record = direction === 'input' ? node?.inputs ?? {} : node?.outputs ?? {};
  const direct = record[normalizedPort];
  if (isSpecificDataType(resolveDataTypeKey, direct)) {
    return cloneDataType(direct ?? 'Generic');
  }
  const connections = plan.connections ?? [];
  const candidates: PipelineDataType[] = [];
  connections.forEach((connection) => {
    const fromPort = normalizePipelinePortName(connection.from.port);
    const toPort = normalizePipelinePortName(connection.to.port);
    if (direction === 'input') {
      if (connection.to.node === nodeId && toPort === normalizedPort) {
        const sourceNode = plan.nodes?.[connection.from.node];
        const sourceType = sourceNode?.outputs?.[fromPort];
        if (sourceType !== undefined) {
          candidates.push(sourceType);
        }
      }
    } else if (connection.from.node === nodeId && fromPort === normalizedPort) {
      const targetNode = plan.nodes?.[connection.to.node];
      const targetType = targetNode?.inputs?.[toPort];
      if (targetType !== undefined) {
        candidates.push(targetType);
      }
    }
  });
  const preferredCandidate = candidates.find((candidate) => isSpecificDataType(resolveDataTypeKey, candidate));
  if (preferredCandidate) {
    return cloneDataType(preferredCandidate ?? 'Generic');
  }
  if (direct !== undefined) {
    return cloneDataType(direct ?? 'Generic');
  }
  if (candidates[0] !== undefined) {
    return cloneDataType(candidates[0] ?? 'Generic');
  }
  return 'Generic';
};
