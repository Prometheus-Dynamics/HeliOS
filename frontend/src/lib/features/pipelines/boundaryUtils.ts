import type {
  PipelineDataType,
  PipelineGraphNode,
  PipelineGraphPlan,
  PipelineInputQueueConfig,
  PipelineOutputSinkConfig
} from '$lib/types/pipeline';
import { blake3 } from '@noble/hashes/blake3.js';

export const PIPELINE_INPUT_BACKEND_ID = 'pipeline:input';
export const PIPELINE_OUTPUT_BACKEND_ID = 'pipeline:output';
const PIPELINE_BOUNDARY_PROVIDER = 'pipeline.boundary';

const DEFAULT_INPUT_LOCATION = { x: -320, y: 0 };
const DEFAULT_OUTPUT_LOCATION = { x: 320, y: 0 };

export type PipelineBoundaryInfo = {
  name: string;
  nodeId: string;
  node: PipelineGraphNode | null;
  dataType: PipelineDataType;
};

const INPUT_NAMESPACE = 'helios:pipeline:input:';
const OUTPUT_NAMESPACE = 'helios:pipeline:output:';

const cloneUnknown = <T>(input: T): T => {
  if (input === null || typeof input !== 'object') return input;
  if (Array.isArray(input)) {
    return input.map((item) => cloneUnknown(item)) as T;
  }
  const cloned = Object.fromEntries(
    Object.entries(input as Record<string, unknown>).map(([key, item]) => [key, cloneUnknown(item)])
  ) as T;
  const descriptor = Object.getOwnPropertyDescriptor(input as object, 'descriptor');
  if (descriptor?.value !== undefined) {
    Object.defineProperty(cloned as object, 'descriptor', {
      value: descriptor.value,
      enumerable: false,
      configurable: true,
      writable: true
    });
  }
  return cloned;
};

const cloneDataType = (value: PipelineDataType): PipelineDataType => {
  if (value === undefined || typeof value === 'string') return value;
  return cloneUnknown(value);
};

export const normalizePipelinePortName = (name: string): string => name.trim().toLowerCase();

export const isPipelineInputNode = (node: PipelineGraphNode | undefined | null): boolean =>
  Boolean(node && node.backendId?.toLowerCase() === PIPELINE_INPUT_BACKEND_ID);

export const isPipelineOutputNode = (node: PipelineGraphNode | undefined | null): boolean =>
  Boolean(node && node.backendId?.toLowerCase() === PIPELINE_OUTPUT_BACKEND_ID);

const isHostBridgeNode = (node: PipelineGraphNode | undefined | null): boolean => {
  const backendId = node?.backendId?.toLowerCase() ?? '';
  return backendId === 'io.host_bridge' || backendId.endsWith(':io.host_bridge');
};

const isHostOutputNode = (node: PipelineGraphNode | undefined | null): boolean => {
  const backendId = node?.backendId?.toLowerCase() ?? '';
  return backendId === 'io.host_output' || backendId.endsWith(':io.host_output');
};

const hostIoDirection = (node: PipelineGraphNode | undefined | null): 'input' | 'output' | null => {
  if (!node) return null;
  if (isPipelineInputNode(node) || isHostBridgeNode(node)) return 'input';
  if (isPipelineOutputNode(node) || isHostOutputNode(node)) return 'output';
  return null;
};

const boundaryPort = (node: PipelineGraphNode, direction: 'input' | 'output'): [string, PipelineDataType] | null => {
  const record = direction === 'input' ? node.inputs ?? {} : node.outputs ?? {};
  const entries = Object.entries(record);
  if (entries.length === 0) return null;
  // Boundary nodes should only expose a single port; prefer deterministic order otherwise.
  return entries[0];
};

const defaultLocation = (direction: 'input' | 'output') =>
  direction === 'input' ? { ...DEFAULT_INPUT_LOCATION } : { ...DEFAULT_OUTPUT_LOCATION };

const boundaryNamespace = (direction: 'input' | 'output'): string =>
  direction === 'input' ? INPUT_NAMESPACE : OUTPUT_NAMESPACE;

const bytesToUuid = (bytes: Uint8Array): string => {
  const hex = Array.from(bytes, (value) => value.toString(16).padStart(2, '0')).join('');
  return (
    hex.slice(0, 8) +
    '-' +
    hex.slice(8, 12) +
    '-' +
    hex.slice(12, 16) +
    '-' +
    hex.slice(16, 20) +
    '-' +
    hex.slice(20, 32)
  );
};

export const pipelineBoundaryNodeId = (direction: 'input' | 'output', name: string): string => {
  const canonical = normalizePipelinePortName(name);
  const encoder = new TextEncoder();
  const input = encoder.encode(`${boundaryNamespace(direction)}${canonical}`);
  const hash = blake3(input);
  const bytes = hash.slice(0, 16);
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  return bytesToUuid(bytes);
};

export const buildBoundaryNode = (
  name: string,
  dataType: PipelineDataType,
  direction: 'input' | 'output',
  options?: { location?: { x: number; y: number }; nodeId?: string }
): PipelineGraphNode => {
  const normalized = normalizePipelinePortName(name);
  const nodeId = options?.nodeId ?? pipelineBoundaryNodeId(direction, normalized);
  const displayName = name.trim() || normalized;
  const portRecord = direction === 'input' ? {} : {};
  if (direction === 'input') {
    (portRecord as Record<string, PipelineDataType>)[normalized] = cloneDataType(dataType) ?? 'Generic';
    return {
      id: nodeId,
      backendId: PIPELINE_INPUT_BACKEND_ID,
      metadata: {
        name: displayName,
        provider: PIPELINE_BOUNDARY_PROVIDER
      },
      inputs: {},
      outputs: portRecord as Record<string, PipelineDataType>,
      info: {
        id: `${PIPELINE_INPUT_BACKEND_ID}:${normalized}`,
        location: options?.location ? { ...options.location } : defaultLocation('input')
      },
      embedded: null,
      source: null
    };
  }
  (portRecord as Record<string, PipelineDataType>)[normalized] = cloneDataType(dataType) ?? 'Generic';
  return {
    id: nodeId,
    backendId: PIPELINE_OUTPUT_BACKEND_ID,
    metadata: {
      name: displayName,
      provider: PIPELINE_BOUNDARY_PROVIDER
    },
    inputs: portRecord as Record<string, PipelineDataType>,
    outputs: {},
    info: {
      id: `${PIPELINE_OUTPUT_BACKEND_ID}:${normalized}`,
      location: options?.location ? { ...options.location } : defaultLocation('output')
    },
    embedded: null,
    source: null
  };
};

export const collectBoundaryNodes = (
  plan: PipelineGraphPlan,
  direction: 'input' | 'output'
): PipelineBoundaryInfo[] => {
  const entries: PipelineBoundaryInfo[] = [];
  Object.entries(plan.nodes ?? {}).forEach(([nodeId, node]) => {
    const isInputNode = direction === 'input' && (isPipelineInputNode(node) || isHostBridgeNode(node));
    const isOutputNode = direction === 'output' && (isPipelineOutputNode(node) || isHostOutputNode(node));
    if (!isInputNode && !isOutputNode) {
      return;
    }
    const portRecord = isInputNode ? node.outputs ?? {} : node.inputs ?? {};
    const portEntries = Object.entries(portRecord);
    if (portEntries.length === 0) return;
    portEntries.forEach(([name, dataType]) => {
      const normalized = normalizePipelinePortName(name);
      if (!normalized) return;
      entries.push({
        nodeId,
        node,
        name: normalized,
        dataType
      });
    });
  });
  return entries;
};

const findHostIoNodeForPort = (
  plan: PipelineGraphPlan,
  direction: 'input' | 'output',
  normalized: string
): PipelineBoundaryInfo | null => {
  const nodes = plan.nodes ?? {};
  for (const [nodeId, node] of Object.entries(nodes)) {
    const isInputNode = direction === 'input' && (isPipelineInputNode(node) || isHostBridgeNode(node));
    const isOutputNode = direction === 'output' && (isPipelineOutputNode(node) || isHostOutputNode(node));
    if (!isInputNode && !isOutputNode) continue;
    const record = isInputNode ? node.outputs ?? {} : node.inputs ?? {};
    for (const [name, dataType] of Object.entries(record)) {
      if (normalizePipelinePortName(name) === normalized) {
        return { nodeId, node, name: normalized, dataType };
      }
    }
  }
  return null;
};

export const collectBoundaryPortsFromMetadata = (
  plan: PipelineGraphPlan,
  direction: 'input' | 'output'
): PipelineBoundaryInfo[] => {
  const record = direction === 'input' ? plan.pipelineInputs ?? {} : plan.pipelineOutputs ?? {};
  const entries = Object.entries(record);
  if (entries.length === 0) {
    return collectBoundaryNodes(plan, direction);
  }
  return entries.map(([name, dataType]) => {
    const normalized = normalizePipelinePortName(name);
    const match = findHostIoNodeForPort(plan, direction, normalized);
    if (match) {
      return {
        nodeId: match.nodeId,
        node: match.node,
        name: normalized,
        dataType: match.dataType ?? dataType
      };
    }
    const nodeId = pipelineBoundaryNodeId(direction, normalized);
    return {
      nodeId,
      node: plan.nodes?.[nodeId] ?? null,
      name: normalized,
      dataType
    };
  });
};

export const removePipelineBoundaryNode = (
  plan: PipelineGraphPlan,
  direction: 'input' | 'output',
  name: string
) => {
  const normalized = normalizePipelinePortName(name);
  const nodes = plan.nodes ?? {};
  const matchingNodes = Object.entries(nodes).filter(([, node]) => {
    const isInputNode = direction === 'input' && (isPipelineInputNode(node) || isHostBridgeNode(node));
    const isOutputNode = direction === 'output' && (isPipelineOutputNode(node) || isHostOutputNode(node));
    if (!isInputNode && !isOutputNode) return false;
    const record = isInputNode ? node.outputs ?? {} : node.inputs ?? {};
    return Object.keys(record).some((key) => normalizePipelinePortName(key) === normalized);
  });

  matchingNodes.forEach(([nodeId, node]) => {
    const record = direction === 'input' ? node.outputs ?? {} : node.inputs ?? {};
    const next = { ...record };
    Object.keys(next).forEach((key) => {
      if (normalizePipelinePortName(key) === normalized) {
        delete next[key];
      }
    });
    if (direction === 'input') {
      node.outputs = next;
    } else {
      node.inputs = next;
    }
    if (Object.keys(next).length === 0) {
      delete nodes[nodeId];
    }
  });

  const nodeIds = new Set(matchingNodes.map(([nodeId]) => nodeId));
  plan.connections = (plan.connections ?? []).filter((connection) => {
    if (!nodeIds.has(connection.from.node) && !nodeIds.has(connection.to.node)) return true;
    const fromMatch = nodeIds.has(connection.from.node) && normalizePipelinePortName(connection.from.port) === normalized;
    const toMatch = nodeIds.has(connection.to.node) && normalizePipelinePortName(connection.to.port) === normalized;
    return !(fromMatch || toMatch);
  });
  if (direction === 'input' && plan.pipelineInputs) {
    delete plan.pipelineInputs[normalized];
    if (Object.keys(plan.pipelineInputs).length === 0) {
      delete plan.pipelineInputs;
    }
    if (plan.pipelineInputConfigs) {
      delete plan.pipelineInputConfigs[normalized];
      if (Object.keys(plan.pipelineInputConfigs).length === 0) {
        delete plan.pipelineInputConfigs;
      }
    }
    if (plan.pipelineInputValues) {
      delete plan.pipelineInputValues[normalized];
      if (Object.keys(plan.pipelineInputValues).length === 0) {
        delete plan.pipelineInputValues;
      }
    }
  }
  if (direction === 'output' && plan.pipelineOutputs) {
    delete plan.pipelineOutputs[normalized];
    if (Object.keys(plan.pipelineOutputs).length === 0) {
      delete plan.pipelineOutputs;
    }
    if (plan.pipelineOutputConfigs) {
      delete plan.pipelineOutputConfigs[normalized];
      if (Object.keys(plan.pipelineOutputConfigs).length === 0) {
        delete plan.pipelineOutputConfigs;
      }
    }
  }
};

const consolidateHostIoNodes = (plan: PipelineGraphPlan) => {
  const nodes = plan.nodes ?? {};
  const byDirection: Record<'input' | 'output', string[]> = { input: [], output: [] };
  Object.entries(nodes).forEach(([nodeId, node]) => {
    const direction = hostIoDirection(node);
    if (!direction) return;
    byDirection[direction].push(nodeId);
  });

  (['input', 'output'] as const).forEach((direction) => {
    const nodeIds = byDirection[direction];
    if (nodeIds.length <= 1) {
      const onlyId = nodeIds[0];
      if (onlyId && nodes[onlyId]) {
        const node = nodes[onlyId];
        if (direction === 'input' && isHostBridgeNode(node)) {
          node.backendId = PIPELINE_INPUT_BACKEND_ID;
          node.info = { ...(node.info ?? { id: node.backendId, location: { x: 0, y: 0 } }), id: node.backendId };
        }
        if (direction === 'output' && isHostOutputNode(node)) {
          node.backendId = PIPELINE_OUTPUT_BACKEND_ID;
          node.info = { ...(node.info ?? { id: node.backendId, location: { x: 0, y: 0 } }), id: node.backendId };
        }
      }
      return;
    }

    const getPortCount = (nodeId: string): number => {
      const node = nodes[nodeId];
      if (!node) return 0;
      const record = direction === 'input' ? node.outputs ?? {} : node.inputs ?? {};
      return Object.keys(record).length;
    };

    const preferredId =
      nodeIds.find((id) => {
        const node = nodes[id];
        if (!node) return false;
        const backendId = node.backendId?.toLowerCase() ?? '';
        return direction === 'input'
          ? backendId === PIPELINE_INPUT_BACKEND_ID
          : backendId === PIPELINE_OUTPUT_BACKEND_ID;
      }) ??
      nodeIds.find((id) => {
        const node = nodes[id];
        if (!node) return false;
        const backendId = node.backendId?.toLowerCase() ?? '';
        return direction === 'input'
          ? backendId === 'io.host_bridge' || backendId.endsWith(':io.host_bridge')
          : backendId === 'io.host_output' || backendId.endsWith(':io.host_output');
      }) ??
      nodeIds[0]!;

    const canonicalId = nodeIds
      .slice()
      .sort((a, b) => getPortCount(b) - getPortCount(a))
      .find((id) => id === preferredId) ?? preferredId;

    const canonical = nodes[canonicalId];
    if (!canonical) return;
    if (direction === 'input') {
      canonical.backendId = PIPELINE_INPUT_BACKEND_ID;
    } else {
      canonical.backendId = PIPELINE_OUTPUT_BACKEND_ID;
    }
    canonical.info = { ...(canonical.info ?? { id: canonical.backendId, location: { x: 0, y: 0 } }), id: canonical.backendId };
    canonical.metadata = {
      ...(canonical.metadata ?? {}),
      name: canonical.metadata?.name?.trim()
        ? canonical.metadata.name
        : direction === 'input'
          ? 'Pipeline Input'
          : 'Pipeline Output'
    };

    const portMap = new Map<string, { name: string; dataType: PipelineDataType }>();
    const pipelineRecord = direction === 'input' ? plan.pipelineInputs ?? {} : plan.pipelineOutputs ?? {};
    Object.entries(pipelineRecord).forEach(([name, dataType]) => {
      const normalized = normalizePipelinePortName(name);
      if (!normalized) return;
      portMap.set(normalized, { name: normalized, dataType: cloneDataType(dataType) ?? 'Generic' });
    });

    nodeIds.forEach((nodeId) => {
      const node = nodes[nodeId];
      if (!node) return;
      const record = direction === 'input' ? node.outputs ?? {} : node.inputs ?? {};
      Object.entries(record).forEach(([name, dataType]) => {
        const normalized = normalizePipelinePortName(name);
        if (!normalized) return;
        if (!portMap.has(normalized)) {
          portMap.set(normalized, { name: normalized, dataType: cloneDataType(dataType) ?? 'Generic' });
        }
      });
    });

    const canonicalPorts: Record<string, PipelineDataType> = {};
    portMap.forEach((entry) => {
      canonicalPorts[entry.name] = entry.dataType;
    });

    if (direction === 'input') {
      canonical.outputs = canonicalPorts;
      canonical.inputs = canonical.inputs ?? {};
    } else {
      canonical.inputs = canonicalPorts;
      canonical.outputs = canonical.outputs ?? {};
    }

    const removedIds = nodeIds.filter((id) => id !== canonicalId);
    if (removedIds.length === 0) return;

    const portNameByNormalized = new Map<string, string>();
    Object.keys(canonicalPorts).forEach((name) => {
      const normalized = normalizePipelinePortName(name);
      if (normalized) {
        portNameByNormalized.set(normalized, name);
      }
    });

    plan.connections = (plan.connections ?? []).map((connection) => {
      const next = { ...connection };
      if (removedIds.includes(connection.from.node)) {
        const normalized = normalizePipelinePortName(connection.from.port);
        next.from = {
          node: canonicalId,
          port: portNameByNormalized.get(normalized) ?? connection.from.port
        };
      }
      if (removedIds.includes(connection.to.node)) {
        const normalized = normalizePipelinePortName(connection.to.port);
        next.to = {
          node: canonicalId,
          port: portNameByNormalized.get(normalized) ?? connection.to.port
        };
      }
      return next;
    });

    removedIds.forEach((id) => {
      delete nodes[id];
    });

    const deduped: typeof plan.connections = [];
    const seen = new Set<string>();
    (plan.connections ?? []).forEach((connection) => {
      const signature = `${connection.from.node}:${connection.from.port}->${connection.to.node}:${connection.to.port}`;
      if (seen.has(signature)) return;
      seen.add(signature);
      deduped.push(connection);
    });
    plan.connections = deduped;
  });
};

export const refreshPipelineIoCaches = (plan: PipelineGraphPlan) => {
  consolidateHostIoNodes(plan);
  const derivePorts = (
    record: Record<string, PipelineDataType> | undefined,
    direction: 'input' | 'output'
  ): Record<string, PipelineDataType> => {
    const normalizedEntries = Object.entries(record ?? {}).map(([name, dataType]) => {
      const normalized = normalizePipelinePortName(name);
      return [normalized, cloneDataType(dataType) ?? 'Generic'] as const;
    });
    if (normalizedEntries.length > 0) {
      return Object.fromEntries(normalizedEntries);
    }
    const fallback = collectBoundaryNodes(plan, direction).map(({ name, dataType }) => [
      name,
      cloneDataType(dataType) ?? 'Generic'
    ]) as Array<[string, PipelineDataType]>;
    return fallback.length > 0 ? Object.fromEntries(fallback) : {};
  };

  const syncBoundaryNodeTypes = (
    direction: 'input' | 'output',
    record: Record<string, PipelineDataType>
  ) => {
    Object.values(plan.nodes ?? {}).forEach((node) => {
      if (!node) return;
      const isInputNode = direction === 'input' && (isPipelineInputNode(node) || isHostBridgeNode(node));
      const isOutputNode = direction === 'output' && (isPipelineOutputNode(node) || isHostOutputNode(node));
      if (!isInputNode && !isOutputNode) {
        return;
      }
      if (isPipelineInputNode(node) || isPipelineOutputNode(node) || isHostBridgeNode(node) || isHostOutputNode(node)) {
        const current = direction === 'input' ? node.outputs ?? {} : node.inputs ?? {};
        const nextRecord: Record<string, PipelineDataType> = { ...current };
        Object.entries(current).forEach(([port, dataType]) => {
          const normalized = normalizePipelinePortName(port);
          const override = record[normalized];
          if (override) {
            nextRecord[port] = cloneDataType(override) ?? 'Generic';
          } else {
            nextRecord[port] = cloneDataType(dataType) ?? 'Generic';
          }
        });
        if (direction === 'input') {
          node.outputs = nextRecord;
          node.inputs = node.inputs ?? {};
        } else {
          node.inputs = nextRecord;
          node.outputs = node.outputs ?? {};
        }
        return;
      }

      const portEntry = boundaryPort(node, direction === 'input' ? 'output' : 'input');
      if (!portEntry) return;
      const [portName] = portEntry;
      const normalized = normalizePipelinePortName(portName);
      const dataType = record[normalized];
      if (!dataType) return;

      if (direction === 'input') {
        node.outputs = { ...(node.outputs ?? {}), [portName]: cloneDataType(dataType) ?? 'Generic' };
      } else {
        node.inputs = { ...(node.inputs ?? {}), [portName]: cloneDataType(dataType) ?? 'Generic' };
      }
    });
  };

  const inputs = derivePorts(plan.pipelineInputs, 'input');
  const inputNames = new Set(Object.keys(inputs));
  plan.pipelineInputs = inputs;
  syncBoundaryNodeTypes('input', inputs);
  const existingInputConfigs = { ...(plan.pipelineInputConfigs ?? {}) };
  const defaultInputConfig: PipelineInputQueueConfig = { policy: 'NewestWins', capacity: 3 };
  inputNames.forEach((name) => {
    existingInputConfigs[name] = { ...(existingInputConfigs[name] ?? defaultInputConfig) };
  });
  Object.keys(existingInputConfigs).forEach((key) => {
    if (!inputNames.has(key)) {
      delete existingInputConfigs[key];
    }
  });
  plan.pipelineInputConfigs = existingInputConfigs;

  const outputs = derivePorts(plan.pipelineOutputs, 'output');
  const outputNames = new Set(Object.keys(outputs));
  plan.pipelineOutputs = outputs;
  syncBoundaryNodeTypes('output', outputs);
  const existingOutputConfigs = { ...(plan.pipelineOutputConfigs ?? {}) };
  const defaultOutputConfig: PipelineOutputSinkConfig = { capacity: 4 };
  outputNames.forEach((name) => {
    existingOutputConfigs[name] = { ...(existingOutputConfigs[name] ?? defaultOutputConfig) };
  });
  Object.keys(existingOutputConfigs).forEach((key) => {
    if (!outputNames.has(key)) {
      delete existingOutputConfigs[key];
    }
  });
  plan.pipelineOutputConfigs = existingOutputConfigs;
};
