import { orderedPortNames, cloneDataType } from '../utils';
import { getDataTypeVariants, normalizeImageFlavorType, resolveDataTypeKey } from '$lib/features/pipelines/valueFormatting';
import type { PipelineDataType, PipelineGraphPlan, PipelineRegistryEntry } from '$lib/types/pipeline';
import type { ApiPortDescriptor } from '$lib/types/pipeline-api';
import { normalizePipelinePortName } from '$lib/features/pipelines/boundary';

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

const baseTypeKey = (value: string | null | undefined): string | null => {
  if (!value) return null;
  const trimmed = value.trim().toLowerCase();
  if (!trimmed) return null;
  const colonIndex = trimmed.indexOf(':');
  return colonIndex >= 0 ? trimmed.slice(0, colonIndex) : trimmed;
};

const normalizeTypeKeyForMatch = (value: string | null | undefined): string | null => {
  if (!value) return null;
  const trimmed = value.trim().toLowerCase();
  if (!trimmed) return null;
  const colonIndex = trimmed.lastIndexOf(':');
  const withoutNamespace = colonIndex >= 0 ? trimmed.slice(colonIndex + 1) : trimmed;
  const segments = withoutNamespace.split('::').filter(Boolean);
  return segments.length > 0 ? segments[segments.length - 1] ?? null : withoutNamespace;
};

const hasPaletteInfo = (value: PipelineDataType | undefined): boolean => {
  if (!value || typeof value === 'string') return false;
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

const shouldUpgradeType = (
  current: PipelineDataType | undefined,
  candidate: PipelineDataType | undefined
): boolean => {
  if (!current || !candidate || typeof candidate === 'string') return false;
  if (isGenericType(candidate) || !hasPaletteInfo(candidate)) return false;
  if (!(typeof current === 'string' || !hasPaletteInfo(current))) return false;

  const currentKeyRaw = resolveDataTypeKey(current)?.toLowerCase() ?? null;
  const candidateKeyRaw = resolveDataTypeKey(candidate)?.toLowerCase() ?? null;
  const currentKey = normalizeTypeKeyForMatch(currentKeyRaw);
  const candidateKey = normalizeTypeKeyForMatch(candidateKeyRaw);

  if (currentKey && candidateKey) {
    if (currentKey === candidateKey) return true;
    if (currentKeyRaw && candidateKeyRaw) {
      if (currentKeyRaw.includes(candidateKey) || candidateKeyRaw.includes(currentKey)) return true;
    }
  }

  if (candidateKey && currentKeyRaw && ['image', 'enum', 'json'].includes(candidateKey)) {
    return currentKeyRaw.includes(candidateKey);
  }

  return Boolean(currentKeyRaw && candidateKeyRaw && currentKeyRaw === candidateKeyRaw);
};

export const mergePorts = (
  existing: Record<string, PipelineDataType> | undefined,
  fallback: Record<string, PipelineDataType> | undefined
) => {
  if (!fallback || Object.keys(fallback).length === 0) {
    return existing ?? {};
  }
  let changed = !existing;
  const base: Record<string, PipelineDataType> = existing ? { ...existing } : {};
  Object.entries(fallback).forEach(([port, dataType]) => {
    const shouldRefresh =
      !Object.prototype.hasOwnProperty.call(base, port) ||
      (isGenericType(base[port]) && !isGenericType(dataType)) ||
      needsEnumRefresh(base[port], dataType) ||
      shouldUpgradeType(base[port], dataType);
    if (shouldRefresh) {
      base[port] = cloneDataType(dataType) ?? 'Generic';
      changed = true;
    }
  });
  return changed ? base : existing ?? {};
};

const resolveFanInPortType = (
  faninInputs: PipelineRegistryEntry['faninInputs'] | undefined,
  port: string
): PipelineDataType | null => {
  if (!faninInputs || faninInputs.length === 0) return null;
  const normalizedPort = normalizePipelinePortName(port);
  for (const fanin of faninInputs) {
    const normalizedPrefix = normalizePipelinePortName(fanin.prefix);
    if (!normalizedPort.startsWith(normalizedPrefix)) continue;
    const suffix = normalizedPort.slice(normalizedPrefix.length);
    if (!/^\d+$/u.test(suffix)) continue;
    return fanin.dataType ?? null;
  }
  return null;
};

export const applyFanInTypes = (
  ports: Record<string, PipelineDataType> | undefined,
  faninInputs: PipelineRegistryEntry['faninInputs'] | undefined
): Record<string, PipelineDataType> | undefined => {
  if (!ports || !faninInputs || faninInputs.length === 0) {
    return ports;
  }
  let changed = false;
  const next = { ...ports };
  for (const [port, dataType] of Object.entries(ports)) {
    if (!isGenericType(dataType)) continue;
    const resolved = resolveFanInPortType(faninInputs, port);
    if (!resolved) continue;
    next[port] = cloneDataType(resolved) ?? resolved;
    changed = true;
  }
  return changed ? next : ports;
};

export const filterFanInInputs = (
  ports: Record<string, PipelineDataType> | undefined,
  faninInputs: PipelineRegistryEntry['faninInputs'] | undefined,
  nodeId: string,
  connections: PipelineGraphPlan['connections'],
  nodes: PipelineGraphPlan['nodes']
): Record<string, PipelineDataType> | undefined => {
  if (!faninInputs || faninInputs.length === 0) {
    return ports;
  }
  const basePorts: Record<string, PipelineDataType> = ports ?? {};

  const faninPrefixes = faninInputs.map((fanin) => ({
    prefix: fanin.prefix,
    normalizedPrefix: normalizePipelinePortName(fanin.prefix),
    start: typeof fanin.start === 'number' ? fanin.start : 0,
    dataType: fanin.dataType
  }));

  const isFanInPort = (normalizedPort: string, normalizedPrefix: string): number | null => {
    if (!normalizedPort.startsWith(normalizedPrefix)) return null;

    const suffix = normalizedPort
      .slice(normalizedPrefix.length)
      .replace(/^[_\-\s\.]+/u, '');

    if (!/^\d+$/u.test(suffix)) return null;
    return Number.parseInt(suffix, 10);
  };

  const byPrefix = new Map<string, { maxIndex: number | null; connectedIndices: Set<number> }>();
  for (const entry of faninPrefixes) {
    byPrefix.set(entry.normalizedPrefix, { maxIndex: null, connectedIndices: new Set() });
  }

  const connectedTypes = new Map<string, PipelineDataType>();
  for (const connection of connections) {
    if (connection.to.node !== nodeId) continue;

    const normalizedToPort = normalizePipelinePortName(connection.to.port);

    for (const entry of faninPrefixes) {
      const index = isFanInPort(normalizedToPort, entry.normalizedPrefix);
      if (index == null) continue;

      const state = byPrefix.get(entry.normalizedPrefix);
      if (!state) continue;

      state.connectedIndices.add(index);

      if (state.maxIndex == null || index > state.maxIndex) state.maxIndex = index;
    }

    const fromNode = nodes?.[connection.from.node];
    const fromType = applyDescriptorOverrides(
      resolvePortValue(fromNode?.outputs, connection.from.port),
      resolvePortDescriptor(fromNode, connection.from.port, 'output')
    );
    if (fromType) {
      connectedTypes.set(connection.to.port, cloneDataType(fromType) ?? fromType);
    }
  }

  const nextPorts: Record<string, PipelineDataType> = { ...basePorts };
  const allowedFanIn = new Map<string, Set<number>>();

  for (const entry of faninPrefixes) {
    const state = byPrefix.get(entry.normalizedPrefix);
    if (!state) continue;

    const indices = new Set<number>();
    if (state.connectedIndices.size === 0) {
      indices.add(entry.start);
    } else {
      for (const index of state.connectedIndices) {
        indices.add(index);
      }
      const nextIndex = Math.max((state.maxIndex ?? entry.start) + 1, entry.start);
      indices.add(nextIndex);
    }
    allowedFanIn.set(entry.normalizedPrefix, indices);
    for (const index of indices) {
      const name = `${entry.prefix}${index}`;
      if (nextPorts[name] == null) {
        nextPorts[name] = entry.dataType ?? 'Generic';
      }
    }
  }

  for (const [port, dataType] of connectedTypes.entries()) {
    nextPorts[port] = dataType;
  }

  let changed = false;
  const filtered: Record<string, PipelineDataType> = {};

  for (const [port, dataType] of Object.entries(nextPorts)) {
    const normalizedPort = normalizePipelinePortName(port);

    let included = true;

    for (const entry of faninPrefixes) {
      const index = isFanInPort(normalizedPort, entry.normalizedPrefix);
      if (index == null) continue;
      const allowed = allowedFanIn.get(entry.normalizedPrefix);
      included = Boolean(allowed?.has(index));
      break;
    }

    if (included) {
      filtered[port] = dataType;

      if (!Object.prototype.hasOwnProperty.call(basePorts, port)) changed = true;
    } else {
      if (Object.prototype.hasOwnProperty.call(basePorts, port)) changed = true;
    }
  }

  return changed ? filtered : basePorts;
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
  direction: 'input' | 'output'
): ApiPortDescriptor | undefined => {
  const record = (direction === 'input' ? node?.source?.inputs : node?.source?.outputs) as
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
  const kindOverride = typeof descriptor.kind === 'string' ? descriptor.kind.trim() : '';

  if (!label && !color && typeof settableOverride !== 'boolean' && !kindOverride) {
    return type;
  }

  let base: PipelineDataType | undefined = type;
  if (!base && kindOverride) {
    base = kindOverride;
  }
  if (!base) return type;

  let next: Exclude<PipelineDataType, string>;
  if (typeof base === 'string') {
    next = { kind: base.trim() || base };
  } else {
    const cloned = cloneDataType(base);
    next = (cloned && typeof cloned === 'object' ? cloned : { ...base }) as Exclude<PipelineDataType, string>;
  }

  if (kindOverride && (isGenericType(next) || !next.kind)) {
    next.kind = kindOverride;
  }
  if (label && (typeof next.label !== 'string' || !next.label.trim())) {
    next.label = label;
  }
  if (color && (typeof (next as { color?: string }).color !== 'string' || !(next as { color?: string }).color?.trim())) {
    (next as { color?: string }).color = color;
  }
  if (typeof settableOverride === 'boolean' && typeof (next as { settable?: boolean }).settable !== 'boolean') {
    (next as { settable?: boolean }).settable = settableOverride;
  }
  return next;
};

const inferTypeFromConnections = (
  graph: PipelineGraphPlan,
  nodeId: string,
  port: string,
  direction: 'input' | 'output'
): { type: PipelineDataType | undefined; hasConnection: boolean } => {
  const normalizedPort = normalizePipelinePortName(port);
  const candidates: PipelineDataType[] = [];
  let hasConnection = false;
  for (const connection of graph.connections ?? []) {
    if (direction === 'input') {
      if (connection.to.node !== nodeId) continue;
      if (normalizePipelinePortName(connection.to.port) !== normalizedPort) continue;
      hasConnection = true;
      const sourceNode = graph.nodes?.[connection.from.node];
      const sourceType = applyDescriptorOverrides(
        resolvePortValue(sourceNode?.outputs, connection.from.port),
        resolvePortDescriptor(sourceNode, connection.from.port, 'output')
      );
      if (sourceType !== undefined) {
        candidates.push(sourceType);
      }
    } else {
      if (connection.from.node !== nodeId) continue;
      if (normalizePipelinePortName(connection.from.port) !== normalizedPort) continue;
      hasConnection = true;
      const targetNode = graph.nodes?.[connection.to.node];
      const targetType = applyDescriptorOverrides(
        resolvePortValue(targetNode?.inputs, connection.to.port),
        resolvePortDescriptor(targetNode, connection.to.port, 'input')
      );
      if (targetType !== undefined) {
        candidates.push(targetType);
      }
    }
  }
  const preferred = candidates.find((candidate) => !isGenericType(candidate));
  return { type: preferred ?? candidates[0], hasConnection };
};

export const resolveGenericPorts = (
  ports: Record<string, PipelineDataType> | undefined,
  graph: PipelineGraphPlan,
  nodeId: string,
  direction: 'input' | 'output',
  options: { resetOnDisconnect?: boolean } = {}
): Record<string, PipelineDataType> | undefined => {
  if (!ports || Object.keys(ports).length === 0) return ports;
  let changed = false;
  const next = { ...ports };
  for (const [port, dataType] of Object.entries(ports)) {
    const { type: inferred, hasConnection } = inferTypeFromConnections(graph, nodeId, port, direction);
    if (!hasConnection && options.resetOnDisconnect) {
      if (!isGenericType(dataType)) {
        next[port] = 'Generic';
        changed = true;
      }
      continue;
    }
    if (!inferred || isGenericType(inferred)) continue;

    if (!isGenericType(dataType)) {
      if (shouldUpgradeType(dataType, inferred)) {
        const normalized = normalizeImageFlavorType(cloneDataType(inferred) ?? inferred) ?? inferred;
        next[port] = normalized;
        changed = true;
      } else if (typeof inferred === 'string' && typeof dataType === 'object') {
        const inferredTrimmed = inferred.trim();
        const inferredBase = baseTypeKey(inferredTrimmed);
        const currentKey = resolveDataTypeKey(dataType);
        if (inferredBase && currentKey && currentKey.toLowerCase() === inferredBase && inferredTrimmed.includes(':')) {
          next[port] = normalizeImageFlavorType(inferredTrimmed) ?? inferredTrimmed;
          changed = true;
        }
      }
      continue;
    }

    const normalized = normalizeImageFlavorType(cloneDataType(inferred) ?? inferred) ?? inferred;
    next[port] = normalized;
    changed = true;
  }
  return changed ? next : ports;
};

const needsEnumRefresh = (current?: PipelineDataType, candidate?: PipelineDataType): boolean => {
  if (!candidate) {
    return false;
  }
  if (!current) {
    return true;
  }
  const currentVariants = getDataTypeVariants(current);
  const candidateVariants = getDataTypeVariants(candidate);
  if (candidateVariants.length === 0) {
    return false;
  }
  if (currentVariants.length !== candidateVariants.length) {
    return true;
  }
  for (let i = 0; i < currentVariants.length; i += 1) {
    if (currentVariants[i] !== candidateVariants[i]) {
      return true;
    }
  }
  return false;
};

export const resolvePortOrder = (
  candidate: string[] | undefined,
  ports: Record<string, PipelineDataType | undefined> | undefined
): string[] => {
  const existing = orderedPortNames(ports);
  if (!candidate || candidate.length === 0) {
    return existing;
  }
  const filtered = candidate.filter((port) => Boolean(ports?.[port]));
  const extras = existing.filter((port) => !filtered.includes(port));
  const combined = [...filtered, ...extras];
  return combined.length > 0 ? combined : existing;
};
