import type {
  PipelineDataType,
  PipelineGraphPlan
} from '$lib/types/pipeline';
import {
  normalizePipelinePortName,
  refreshPipelineIoCaches,
  PIPELINE_INPUT_BACKEND_ID,
  PIPELINE_OUTPUT_BACKEND_ID
} from '$lib/features/pipelines/boundary';
import { resolveDataTypeKey } from '$lib/features/pipelines/valueFormatting';
import type { ApiPortDescriptor } from '$lib/types/pipeline-api';
import type { Edge } from '@xyflow/svelte';
import type { HandleType, XYPosition } from '@xyflow/system';
import { resolveTypeLabel } from '../pipeline-node/portUtils';

export const HANDLE_SEPARATOR = '__';
export const PORT_KEY_SEPARATOR = ':';

export type PortOrders = {
  inputs: Record<string, string[]>;
  outputs: Record<string, string[]>;
};

export type GraphBounds = {
  minX: number;
  maxX: number;
  minY: number;
  maxY: number;
};

export const buildPortKey = (direction: 'input' | 'output', name: string): string =>
  `${direction}${PORT_KEY_SEPARATOR}${name}`;

export const cloneUnknown = <T>(value: T): T => {
  if (value === null || typeof value !== 'object') {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map((item) => cloneUnknown(item)) as unknown as T;
  }
  const cloned = Object.fromEntries(
    Object.entries(value as Record<string, unknown>).map(([key, item]) => [key, cloneUnknown(item)])
  ) as T;
  const descriptor = Object.getOwnPropertyDescriptor(value as object, 'descriptor');
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

const mergeChildPorts = (
  existing: Record<string, PipelineDataType> | undefined,
  fallback: Record<string, PipelineDataType> | undefined | null
): Record<string, PipelineDataType> | undefined => {
  if (!fallback || Object.keys(fallback).length === 0) {
    return existing;
  }
  const normalizedFallback = Object.entries(fallback).reduce<Record<string, PipelineDataType>>((acc, [name, dataType]) => {
    const normalized = normalizePipelinePortName(name);
    if (normalized) {
      acc[normalized] = cloneDataType(dataType) ?? 'Generic';
    }
    return acc;
  }, {});
  if (Object.keys(normalizedFallback).length === 0) {
    return existing;
  }
  const base = { ...(existing ?? {}) };
  let changed = false;
  Object.entries(normalizedFallback).forEach(([name, dataType]) => {
    if (!Object.prototype.hasOwnProperty.call(base, name)) {
      base[name] = dataType;
      changed = true;
    }
  });
  return changed ? base : existing;
};

export const cloneDataType = (value: PipelineDataType | undefined): PipelineDataType | undefined => {
  if (value === undefined) return undefined;
  if (typeof value === 'string') return value;
  const descriptor = Object.getOwnPropertyDescriptor(value, 'descriptor');
  const cloned = cloneUnknown(value);
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

export const clonePortRecord = (
  ports: Record<string, PipelineDataType> | undefined
): Record<string, PipelineDataType> => {
  if (!ports) return {};
  return Object.fromEntries(
    Object.entries(ports).map(([key, value]) => [key, cloneDataType(value) ?? 'Generic'])
  );
};

export const clonePlan = (value: PipelineGraphPlan): PipelineGraphPlan => cloneUnknown(value);

export const orderedPortNames = (
  ports: Record<string, PipelineDataType | undefined> | undefined
): string[] => {
  if (!ports) return [];
  return Object.keys(ports).sort((a, b) => a.localeCompare(b));
};

export const buildPortOrders = (graph: PipelineGraphPlan): PortOrders => {
  const ensureSet = (map: Map<string, Set<string>>, key: string) => {
    let set = map.get(key);
    if (!set) {
      set = new Set<string>();
      map.set(key, set);
    }
    return set;
  };

  const inputSets = new Map<string, Set<string>>();
  const outputSets = new Map<string, Set<string>>();

  graph.connections.forEach((connection) => {
    const fromNode = connection.from?.node;
    const fromPort = connection.from?.port;
    if (fromNode && fromPort) {
      ensureSet(outputSets, fromNode).add(fromPort);
    }
    const toNode = connection.to?.node;
    const toPort = connection.to?.port;
    if (toNode && toPort) {
      ensureSet(inputSets, toNode).add(toPort);
    }
  });

  Object.values(graph.nodes).forEach((node) => {
    const inputSet = ensureSet(inputSets, node.id);
    Object.keys(node.inputs ?? {}).forEach((port) => inputSet.add(port));
    const outputSet = ensureSet(outputSets, node.id);
    Object.keys(node.outputs ?? {}).forEach((port) => outputSet.add(port));
  });

  const toRecord = (sets: Map<string, Set<string>>): Record<string, string[]> => {
    const record: Record<string, string[]> = {};
    sets.forEach((set, key) => {
      record[key] = Array.from(set).sort((a, b) => a.localeCompare(b));
    });
    return record;
  };

  return {
    inputs: toRecord(inputSets),
    outputs: toRecord(outputSets)
  };
};

const normalizeTypeKey = (value: string | null | undefined): string | null => {
  if (!value) return null;
  const trimmed = value.trim();
  return trimmed ? trimmed.toLowerCase() : null;
};

export const typeKey = (type: PipelineDataType | undefined): string | null => {
  if (type === undefined || type === null) return null;
  const label = resolveTypeLabel(type);
  const key = label ?? resolveDataTypeKey(type) ?? (typeof type === 'string' ? type : null);
  return normalizeTypeKey(key);
};

const GENERIC_TYPE_KEYS = new Set(['generic', 'any', 'unknown', 'dynamic']);

const isGenericType = (value: PipelineDataType | undefined): boolean => {
  if (!value) return true;
  if (typeof value === 'string') {
    const trimmed = value.trim().toLowerCase();
    return trimmed.length === 0 || GENERIC_TYPE_KEYS.has(trimmed);
  }
  const kind = typeof value.kind === 'string' ? value.kind.trim().toLowerCase() : '';
  return kind.length === 0 || GENERIC_TYPE_KEYS.has(kind);
};

const resolvePortValue = (
  record: Record<string, PipelineDataType> | undefined,
  port: string
): PipelineDataType | undefined => {
  if (!record) return undefined;
  if (Object.prototype.hasOwnProperty.call(record, port)) {
    return record[port];
  }
  const normalized = normalizePipelinePortName(port);
  if (Object.prototype.hasOwnProperty.call(record, normalized)) {
    return record[normalized];
  }
  const fallback = Object.keys(record).find((key) => normalizePipelinePortName(key) === normalized);
  return fallback ? record[fallback] : undefined;
};

const resolvePortDescriptor = (
  node: PipelineGraphPlan['nodes'][string] | undefined,
  port: string,
  handleType: HandleType
): ApiPortDescriptor | undefined => {
  const record = (handleType === 'source' ? node?.source?.outputs : node?.source?.inputs) as
    | Record<string, ApiPortDescriptor>
    | undefined;
  if (!record) return undefined;
  if (Object.prototype.hasOwnProperty.call(record, port)) {
    return record[port];
  }
  const normalized = normalizePipelinePortName(port);
  if (Object.prototype.hasOwnProperty.call(record, normalized)) {
    return record[normalized];
  }
  const fallback = Object.keys(record).find((key) => normalizePipelinePortName(key) === normalized);
  return fallback ? record[fallback] : undefined;
};

const applyDescriptorOverrides = (
  type: PipelineDataType | undefined,
  descriptor: ApiPortDescriptor | undefined
): PipelineDataType | undefined => {
  if (!descriptor) return type;
  const overrides = descriptor.overrides;
  const label = typeof overrides?.label === 'string' ? overrides.label.trim() : '';
  const color = typeof overrides?.color === 'string' ? overrides.color.trim() : '';
  const settableOverride = typeof overrides?.settable === 'boolean' ? overrides.settable : undefined;
  const descriptorKind = typeof descriptor.kind === 'string' ? descriptor.kind.trim() : '';

  if (!label && !color && typeof settableOverride !== 'boolean' && !descriptorKind) {
    return type;
  }

  let base: PipelineDataType | undefined = type;
  if (!base && descriptorKind) {
    base = descriptorKind;
  }
  if (!base) return type;

  if (typeof base === 'string') {
    let nextKind = base.trim() || base;
    if (descriptorKind) {
      const baseLower = nextKind.toLowerCase();
      const descriptorLower = descriptorKind.toLowerCase();
      if (baseLower === 'generic' || descriptorLower.startsWith(`${baseLower}:`)) {
        nextKind = descriptorKind;
      }
    }
    if (!label && !color && typeof settableOverride !== 'boolean') {
      return nextKind;
    }
    const next: Exclude<PipelineDataType, string> = { kind: nextKind };
    if (label) next.label = label;
    if (color) next.color = color;
    if (typeof settableOverride === 'boolean') next.settable = settableOverride;
    return next;
  }

  const cloned = cloneDataType(base);
  const next = (cloned && typeof cloned === 'object' ? cloned : { ...base }) as Exclude<PipelineDataType, string>;
  if (descriptorKind) {
    const baseKind = (next.kind ?? '').trim();
    if (!baseKind || baseKind.toLowerCase() === 'generic' || descriptorKind.toLowerCase().startsWith(`${baseKind.toLowerCase()}:`)) {
      next.kind = descriptorKind;
    }
  }
  if (label && (!next.label || !next.label.trim())) {
    next.label = label;
  }
  if (color && (!(next as { color?: string }).color || !(next as { color?: string }).color?.trim())) {
    (next as { color?: string }).color = color;
  }
  if (typeof settableOverride === 'boolean' && typeof (next as { settable?: boolean }).settable !== 'boolean') {
    (next as { settable?: boolean }).settable = settableOverride;
  }
  return next;
};

export const getPortType = (
  graph: PipelineGraphPlan,
  nodeId: string | null | undefined,
  port: string | null | undefined,
  handleType: HandleType | null
): PipelineDataType | undefined => {
  if (!nodeId || !port || !handleType) return undefined;
  const node = graph.nodes[nodeId];
  if (!node) return undefined;
  const backendId = node.backendId?.toLowerCase() ?? '';
  const resetOnDisconnect =
    backendId === PIPELINE_INPUT_BACKEND_ID ||
    backendId === PIPELINE_OUTPUT_BACKEND_ID ||
    backendId === 'io.host_bridge' ||
    backendId === 'io.host_output' ||
    backendId.endsWith(':io.host_bridge') ||
    backendId.endsWith(':io.host_output') ||
    backendId.endsWith(`:${PIPELINE_INPUT_BACKEND_ID}`) ||
    backendId.endsWith(`:${PIPELINE_OUTPUT_BACKEND_ID}`);
  const direct =
    handleType === 'source'
      ? resolvePortValue(node.outputs, port)
      : resolvePortValue(node.inputs, port);
  const directWithOverrides = applyDescriptorOverrides(direct, resolvePortDescriptor(node, port, handleType));
  if (directWithOverrides && !isGenericType(directWithOverrides)) {
    if (!resetOnDisconnect) {
      return directWithOverrides;
    }
  }
  const normalizedPort = normalizePipelinePortName(port);
  const candidates: PipelineDataType[] = [];
  let hasConnection = false;
  for (const connection of graph.connections ?? []) {
    if (handleType === 'source') {
      if (connection.from.node !== nodeId) continue;
      if (normalizePipelinePortName(connection.from.port) !== normalizedPort) continue;
      hasConnection = true;
      const targetNode = graph.nodes?.[connection.to.node];
      const targetType = applyDescriptorOverrides(
        resolvePortValue(targetNode?.inputs, connection.to.port),
        resolvePortDescriptor(targetNode, connection.to.port, 'target')
      );
      if (targetType !== undefined) {
        candidates.push(targetType);
      }
    } else {
      if (connection.to.node !== nodeId) continue;
      if (normalizePipelinePortName(connection.to.port) !== normalizedPort) continue;
      hasConnection = true;
      const sourceNode = graph.nodes?.[connection.from.node];
      const sourceType = applyDescriptorOverrides(
        resolvePortValue(sourceNode?.outputs, connection.from.port),
        resolvePortDescriptor(sourceNode, connection.from.port, 'source')
      );
      if (sourceType !== undefined) {
        candidates.push(sourceType);
      }
    }
  }
  const preferred = candidates.find((candidate) => !isGenericType(candidate));
  if (preferred) return preferred;
  if (resetOnDisconnect && !hasConnection) {
    return isGenericType(directWithOverrides) ? directWithOverrides : 'Generic';
  }
  return directWithOverrides ?? candidates[0];
};

export const parseHandleId = (handleId: string | null) => {
  if (!handleId) return null;
  const separatorIndex = handleId.lastIndexOf(HANDLE_SEPARATOR);
  if (separatorIndex === -1) return null;
  return {
    node: handleId.slice(0, separatorIndex),
    port: handleId.slice(separatorIndex + HANDLE_SEPARATOR.length)
  };
};

export const ensurePlanPortMetadata = (plan: PipelineGraphPlan): PipelineGraphPlan => {
  const DEFAULT_TYPE: PipelineDataType = 'Generic';

  plan.nodes ??= {};
  plan.connections ??= [];
  refreshPipelineIoCaches(plan);

  Object.values(plan.nodes ?? {}).forEach((node) => {
    if (!node || node.backendId?.toLowerCase() !== 'pipeline:child') {
      return;
    }
    const embeddedInputs = node.embedded?.pipelineInputs;
    const embeddedOutputs = node.embedded?.pipelineOutputs;
    const signatureInputs = node.external?.signature?.inputs;
    const signatureOutputs = node.external?.signature?.outputs;
    node.inputs = mergeChildPorts(node.inputs, embeddedInputs) ?? mergeChildPorts(node.inputs, signatureInputs) ?? node.inputs;
    node.outputs = mergeChildPorts(node.outputs, embeddedOutputs) ?? mergeChildPorts(node.outputs, signatureOutputs) ?? node.outputs;
  });

  const ensureIoNode = (direction: 'input' | 'output') => {
    const record = direction === 'input' ? plan.pipelineInputs ?? {} : plan.pipelineOutputs ?? {};
    const entries = Object.entries(record);
    if (entries.length === 0) return;
    const normalizedPorts: Record<string, PipelineDataType> = {};
    entries.forEach(([name, dataType]) => {
      const normalized = normalizePipelinePortName(name);
      if (!normalized) return;
      normalizedPorts[normalized] = dataType ?? DEFAULT_TYPE;
    });

    const targetBackend = direction === 'input' ? 'pipeline:input' : 'pipeline:output';
    const existingEntry = Object.entries(plan.nodes ?? {}).find(
      ([, node]) => (node?.backendId ?? '').toLowerCase() === targetBackend
    );
    if (existingEntry) {
      const [, node] = existingEntry;
      if (direction === 'input') {
        node.outputs = { ...normalizedPorts };
        node.inputs = node.inputs ?? {};
      } else {
        node.inputs = { ...normalizedPorts };
        node.outputs = node.outputs ?? {};
      }
      if (!node.metadata?.name?.trim()) {
        node.metadata = { ...(node.metadata ?? {}), name: direction === 'input' ? 'Pipeline Input' : 'Pipeline Output' };
      }
      node.info = { ...(node.info ?? { id: targetBackend, location: { x: 0, y: 0 } }), id: targetBackend };
      return;
    }

    const baseId = targetBackend;
    let nodeId = baseId;
    let index = 1;
    while (plan.nodes?.[nodeId]) {
      nodeId = `${baseId}:${index}`;
      index += 1;
    }
    const location = direction === 'input' ? { x: -320, y: 0 } : { x: 320, y: 0 };
    const node = {
      id: nodeId,
      backendId: targetBackend,
      metadata: {
        name: direction === 'input' ? 'Pipeline Input' : 'Pipeline Output',
        provider: 'pipeline.io'
      },
      inputs: direction === 'output' ? { ...normalizedPorts } : {},
      outputs: direction === 'input' ? { ...normalizedPorts } : {},
      info: { id: targetBackend, location },
      embedded: null,
      source: null
    };
    plan.nodes[nodeId] = node;
  };

  ensureIoNode('input');
  ensureIoNode('output');

  const ensureNodePort = (
    node: PipelineGraphPlan['nodes'][string],
    direction: 'input' | 'output',
    port: string,
    fallback: PipelineDataType
  ) => {
    const record = direction === 'input' ? (node.inputs ??= {}) : (node.outputs ??= {});
    const existing = record[port];
    if (existing === undefined || existing === 'Generic') {
      record[port] = fallback ?? DEFAULT_TYPE;
    }
  };

  plan.connections.forEach((connection) => {
    const normalizedFromPort = normalizePipelinePortName(connection.from.port);
    connection.from.port = normalizedFromPort;
    const fromNode = plan.nodes[connection.from.node];
    if (fromNode) {
      const fallback = fromNode.outputs?.[normalizedFromPort] ?? plan.pipelineInputs?.[normalizedFromPort] ?? DEFAULT_TYPE;
      ensureNodePort(fromNode, 'output', normalizedFromPort, fallback);
    }

    const normalizedToPort = normalizePipelinePortName(connection.to.port);
    connection.to.port = normalizedToPort;
    const toNode = plan.nodes[connection.to.node];
    if (toNode) {
      const fallback = toNode.inputs?.[normalizedToPort] ?? plan.pipelineOutputs?.[normalizedToPort] ?? DEFAULT_TYPE;
      ensureNodePort(toNode, 'input', normalizedToPort, fallback);
    }
  });

  refreshPipelineIoCaches(plan);
  return plan;
};

export const computeGraphBounds = (graph: PipelineGraphPlan): GraphBounds => {
  const nodes = Object.values(graph.nodes);
  if (nodes.length === 0) {
    return { minX: 0, minY: 0, maxX: 0, maxY: 0 };
  }
  return nodes.reduce<GraphBounds>(
    (bounds, node) => {
      const x = node.info?.location?.x ?? 0;
      const y = node.info?.location?.y ?? 0;
      return {
        minX: Math.min(bounds.minX, x),
        minY: Math.min(bounds.minY, y),
        maxX: Math.max(bounds.maxX, x),
        maxY: Math.max(bounds.maxY, y)
      };
    },
    { minX: Number.POSITIVE_INFINITY, minY: Number.POSITIVE_INFINITY, maxX: Number.NEGATIVE_INFINITY, maxY: Number.NEGATIVE_INFINITY }
  );
};

export const ensureOrderWithPort = (
  orders: Record<string, string[]>,
  nodeId: string,
  port: string,
  fallback: string[]
): string[] => {
  const existing = orders[nodeId] ?? fallback;
  if (existing.includes(port)) {
    if (!orders[nodeId]) {
      orders[nodeId] = existing;
    }
    return existing;
  }
  const next = [...existing, port].sort((a, b) => a.localeCompare(b));
  orders[nodeId] = next;
  return next;
};

export const portIndexForHandle = (
  graph: PipelineGraphPlan,
  orders: PortOrders,
  endpoint: PipelineGraphPlan['connections'][number]['from'],
  handleType: HandleType
): number | null => {
  const node = graph.nodes[endpoint.node];
  if (node) {
    const portOrderSource = handleType === 'source' ? orders.outputs : orders.inputs;
    const fallback = orderedPortNames(handleType === 'source' ? node.outputs : node.inputs);
    const order = ensureOrderWithPort(portOrderSource, endpoint.node, endpoint.port, fallback);
    const index = order.indexOf(endpoint.port);
    if (index === -1) {
      console.warn('React Flow: unable to resolve port order for node', {
        node: endpoint.node,
        port: endpoint.port,
        handleType
      });
      return null;
    }
    if (!(handleType === 'source' ? node.outputs : node.inputs)?.[endpoint.port]) {
      console.warn('React Flow: synthesizing missing port metadata', {
        node: endpoint.node,
        port: endpoint.port,
        handleType
      });
    }
    return index;
  }
  console.warn('Skipping edge: missing node for endpoint', {
    node: endpoint.node,
    port: endpoint.port,
    handleType
  });
  return null;
};

export const makeHandleId = (
  graph: PipelineGraphPlan,
  orders: PortOrders,
  endpoint: PipelineGraphPlan['connections'][number]['from'],
  handleType: HandleType
): string | null => {
  const index = portIndexForHandle(graph, orders, endpoint, handleType);
  if (index == null) {
    return null;
  }
  const baseId = `${endpoint.node}${HANDLE_SEPARATOR}${endpoint.port}`;
  const order =
    handleType === 'source'
      ? orders.outputs[endpoint.node] ?? orderedPortNames(graph.nodes[endpoint.node]?.outputs)
      : orders.inputs[endpoint.node] ?? orderedPortNames(graph.nodes[endpoint.node]?.inputs);
  const duplicates = order.filter((candidate) => candidate === endpoint.port).length;
  if (duplicates <= 1) {
    return baseId;
  }
  return `${baseId}-${index}`;
};

export const extractPortFromHandle = (handleId: string | null): string | null => {
  if (!handleId) return null;
  const parsed = parseHandleId(handleId);
  if (!parsed) return null;
  const portFromSuffix = parsed.port.split('-')[0];
  return portFromSuffix ?? parsed.port;
};

export const connectionSignature = (connection: PipelineGraphPlan['connections'][number]) =>
  `${connection.from.node}:${connection.from.port}->${connection.to.node}:${connection.to.port}`;

export const edgeSignature = (edge: Edge): string | null => {
  if (!edge.source || !edge.target) return null;
  const sourcePort = extractPortFromHandle(edge.sourceHandle ?? null) ?? edge.data?.fromPort;
  const targetPort = extractPortFromHandle(edge.targetHandle ?? null) ?? edge.data?.toPort;
  if (!sourcePort || !targetPort) return null;
  return `${edge.source}:${sourcePort}->${edge.target}:${targetPort}`;
};

export const clampEditorPosition = (
  position: XYPosition,
  size: { width: number; height: number } = { width: 300, height: 200 },
  padding = 16
): XYPosition => {
  if (typeof window === 'undefined') {
    return position;
  }
  const maxX = Math.max(padding, window.innerWidth - size.width - padding);
  const maxY = Math.max(padding, window.innerHeight - size.height - padding);
  return {
    x: Math.min(Math.max(padding, position.x), maxX),
    y: Math.min(Math.max(padding, position.y), maxY)
  };
};
