import type {
  PipelineDataType,
  PipelineGraphPlan,
  PipelineGraphNode,
  PipelineOverviewPipeline,
  PipelineRegistryEntry,
  PipelineNodeStyle,
  PipelinePortMetadata
} from '$lib/types/pipeline';
import { PIPELINE_INPUT_BACKEND_ID, PIPELINE_OUTPUT_BACKEND_ID, normalizePipelinePortName } from './boundary';
import { normalizeNodeStyle } from './model';
import { getDataTypeVariants, normalizeImageFlavorType } from './valueFormatting';

type StyleSource = PipelineNodeStyle | null | undefined;

export type PipelineRegistryLookup = {
  direct: Map<string, PipelineRegistryEntry>;
  normalized: Map<string, PipelineRegistryEntry>;
};

const normalizeId = (value: string | null | undefined): string | null => {
  if (!value) return null;
  const lower = value.toLowerCase();
  const delimiter = lower.indexOf('@');
  return delimiter >= 0 ? lower.slice(0, delimiter) : lower;
};

export const buildRegistryLookup = (
  entries: PipelineRegistryEntry[]
): PipelineRegistryLookup => {
  const direct = new Map<string, PipelineRegistryEntry>();
  const normalized = new Map<string, PipelineRegistryEntry>();
  for (const entry of entries) {
    const idLower = entry.id.toLowerCase();
    direct.set(idLower, entry);
    const normalizedId = normalizeId(entry.id);
    if (normalizedId && !normalized.has(normalizedId)) {
      normalized.set(normalizedId, entry);
    }
  }
  return { direct, normalized };
};

const resolveRegistryEntry = (
  backendId: string | undefined,
  lookup: PipelineRegistryLookup
): PipelineRegistryEntry | null => {
  if (!backendId) return null;
  const idLower = backendId.toLowerCase();
  const direct = lookup.direct.get(idLower);
  if (direct) return direct;
  const normalizedId = normalizeId(backendId);
  if (normalizedId) {
    const normalized = lookup.normalized.get(normalizedId);
    if (normalized) return normalized;
  }
  return null;
};

const mergePortTypes = (
  existing: Record<string, PipelineDataType> | null | undefined,
  incoming: Record<string, PipelineDataType> | null | undefined
): Record<string, PipelineDataType> | null => {
  if (!existing && !incoming) return null;
  if (!existing) return incoming ? { ...incoming } : null;
  if (!incoming) return { ...existing };
  const merged: Record<string, PipelineDataType> = { ...existing };
  for (const [port, dataType] of Object.entries(incoming)) {
    merged[port] = dataType;
  }
  return merged;
};

const mergePortMetadata = (
  existing: Record<string, PipelinePortMetadata> | null | undefined,
  incoming: Record<string, PipelinePortMetadata> | null | undefined
): Record<string, PipelinePortMetadata> | null => {
  if (!existing && !incoming) return null;
  if (!existing) return incoming ? { ...incoming } : null;
  if (!incoming) return { ...existing };
  const merged: Record<string, PipelinePortMetadata> = { ...existing };
  for (const [port, metadata] of Object.entries(incoming)) {
    if (!metadata) continue;
    merged[port] = { ...(merged[port] ?? {}), ...metadata };
  }
  return merged;
};

const getPortValue = <T>(
  record: Record<string, T> | null | undefined,
  port: string
): T | undefined => {
  if (!record) return undefined;
  if (Object.prototype.hasOwnProperty.call(record, port)) return record[port];
  const lower = port.toLowerCase();
  if (Object.prototype.hasOwnProperty.call(record, lower)) return record[lower];
  return undefined;
};

const resolveEnumVariants = (
  node: PipelineGraphNode,
  entry: PipelineRegistryEntry | null | undefined,
  port: string
): string[] => {
  const nodeMeta = getPortValue(node.metadata?.inputPorts ?? null, port)?.allowedValues ?? null;
  const registryMeta = getPortValue(entry?.metadata?.inputPorts ?? null, port)?.allowedValues ?? null;
  const allowed = (nodeMeta ?? registryMeta ?? []).filter((value) => typeof value === 'string' && value.trim().length > 0);
  if (allowed.length > 0) return allowed;
  const dataType =
    getPortValue(node.inputs as Record<string, PipelineDataType> | null | undefined, port) ??
    getPortValue(entry?.inputs ?? null, port);
  return getDataTypeVariants(dataType ?? undefined);
};

const mapEnumIndexToVariant = (raw: unknown, variants: string[]): string | null => {
  if (typeof raw !== 'number' || !Number.isFinite(raw)) return null;
  const index = Math.trunc(raw);
  if (index < 0 || index >= variants.length) return null;
  const value = variants[index];
  return typeof value === 'string' && value.trim().length > 0 ? value : null;
};

const isGenericDataType = (value: PipelineDataType | string | null | undefined): boolean => {
  if (!value) return true;
  if (typeof value === 'string') return value.trim().toLowerCase() === 'generic';
  if (typeof value.kind === 'string') return value.kind.trim().toLowerCase() === 'generic';
  return false;
};

const resolveFanInPortType = (
  faninInputs: PipelineRegistryEntry['faninInputs'] | null | undefined,
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

const matchFanInIndex = (port: string, normalizedPrefix: string): number | null => {
  const normalizedPort = normalizePipelinePortName(port);
  if (!normalizedPort.startsWith(normalizedPrefix)) return null;
  const suffix = normalizedPort.slice(normalizedPrefix.length);
  if (!/^\d+$/u.test(suffix)) return null;
  return Number.parseInt(suffix, 10);
};

const pruneFanInPorts = (
  plan: PipelineGraphPlan,
  node: PipelineGraphNode,
  faninInputs: PipelineRegistryEntry['faninInputs']
): { inputs: Record<string, PipelineDataType> | null; removed: Set<string> } => {
  const inputs: Record<string, PipelineDataType> = node.inputs ? { ...node.inputs } : {};

  const prefixes = faninInputs.map((fanin) => ({
    prefix: fanin.prefix,
    normalizedPrefix: normalizePipelinePortName(fanin.prefix),
    start: typeof fanin.start === 'number' ? fanin.start : 0,
    dataType: fanin.dataType ?? null
  }));

  const maxConnected = new Map<string, number | null>();
  for (const entry of prefixes) {
    maxConnected.set(entry.normalizedPrefix, null);
  }

  const include = new Set<string>();
  for (const connection of plan.connections ?? []) {
    if (connection.to.node !== node.id) continue;
    for (const entry of prefixes) {
      const index = matchFanInIndex(connection.to.port, entry.normalizedPrefix);
      if (index == null) continue;
      include.add(connection.to.port);
      const current = maxConnected.get(entry.normalizedPrefix);
      if (current == null || index > current) {
        maxConnected.set(entry.normalizedPrefix, index);
      }
    }
  }

  for (const entry of prefixes) {
    const current = maxConnected.get(entry.normalizedPrefix);
    const nextIndex = current == null ? entry.start : Math.max(current + 1, entry.start);
    const nextName = `${entry.prefix}${nextIndex}`;
    include.add(nextName);
  }

  const removed = new Set<string>();
  for (const port of Object.keys(inputs)) {
    let isFanIn = false;
    for (const entry of prefixes) {
      if (matchFanInIndex(port, entry.normalizedPrefix) == null) continue;
      isFanIn = true;
      if (!include.has(port)) {
        delete inputs[port];
        removed.add(port);
      }
      break;
    }
    if (!isFanIn) continue;
  }

  for (const entry of prefixes) {
    for (const port of include) {
      if (matchFanInIndex(port, entry.normalizedPrefix) == null) continue;
      let resolved: PipelineDataType | null = null;
      for (const connection of plan.connections ?? []) {
        if (connection.to.node !== node.id) continue;
        if (connection.to.port !== port) continue;
        const sourceNode = plan.nodes?.[connection.from.node];
        const sourceType = sourceNode?.outputs?.[connection.from.port];
        if (sourceType && !isGenericDataType(sourceType)) {
          resolved = sourceType;
          break;
        }
      }
      if (!inputs[port] || isGenericDataType(inputs[port])) {
        inputs[port] = resolved ?? entry.dataType ?? 'Generic';
      }
    }
  }

  return { inputs, removed };
};

const isBoundaryBackendId = (backendId: string | null | undefined): boolean => {
  if (!backendId) return false;
  const normalized = backendId.toLowerCase();
  return normalized === PIPELINE_INPUT_BACKEND_ID.toLowerCase() || normalized === PIPELINE_OUTPUT_BACKEND_ID.toLowerCase();
};

const isHostBridgeBackendId = (backendId: string | null | undefined): boolean => {
  if (!backendId) return false;
  const normalized = backendId.toLowerCase();
  return normalized === 'io.host_bridge' || normalized === 'io.host_output';
};

const inferBoundaryPortType = (
  plan: PipelineGraphPlan,
  nodeId: string,
  port: string,
  direction: 'input' | 'output'
): PipelineDataType | null => {
  const normalizedPort = normalizePipelinePortName(port);
  const connections = plan.connections ?? [];
  const candidates: PipelineDataType[] = [];
  for (const connection of connections) {
    const fromPort = normalizePipelinePortName(connection.from.port);
    const toPort = normalizePipelinePortName(connection.to.port);
    if (direction === 'input') {
      if (connection.to.node === nodeId && toPort === normalizedPort) {
        const sourceNode = plan.nodes?.[connection.from.node];
        const sourceType = getPortValue(sourceNode?.outputs as Record<string, PipelineDataType> | null | undefined, fromPort);
        if (sourceType !== undefined) {
          candidates.push(sourceType);
        }
      }
    } else if (connection.from.node === nodeId && fromPort === normalizedPort) {
      const targetNode = plan.nodes?.[connection.to.node];
      const targetType = getPortValue(targetNode?.inputs as Record<string, PipelineDataType> | null | undefined, toPort);
      if (targetType !== undefined) {
        candidates.push(targetType);
      }
    }
  }
  const preferred = candidates.find((candidate) => !isGenericDataType(candidate));
  const selected = preferred ?? candidates[0] ?? null;
  return normalizeImageFlavorType(selected ?? undefined) ?? selected;
};

const hydratePlanWithLookup = (
  plan: PipelineGraphPlan | null | undefined,
  lookup: PipelineRegistryLookup | null
): void => {
  if (!plan || !lookup) return;
  const nodes = plan.nodes ?? {};
  for (const node of Object.values(nodes)) {
    if (!node) continue;
    const currentStyle = resolveExistingStyle(node);
    const entry = resolveRegistryEntry(node.backendId, lookup);

    if (entry && !isBoundaryBackendId(node.backendId)) {
      const mergedInputs = mergePortTypes(node.inputs as Record<string, PipelineDataType> | null | undefined, entry.inputs);
      if (mergedInputs) node.inputs = mergedInputs;
      const mergedOutputs = mergePortTypes(node.outputs as Record<string, PipelineDataType> | null | undefined, entry.outputs);
      if (mergedOutputs) node.outputs = mergedOutputs;

      if (entry.faninInputs && entry.faninInputs.length > 0) {
        if (!node.inputs) {
          node.inputs = {};
        }
        const nextInputs = { ...node.inputs };
        let updatedFanIn = false;
        for (const [port, dataType] of Object.entries(node.inputs)) {
          if (!isGenericDataType(dataType)) continue;
          const resolved = resolveFanInPortType(entry.faninInputs, port);
          if (!resolved) continue;
          nextInputs[port] = resolved;
          updatedFanIn = true;
        }
        if (updatedFanIn) {
          node.inputs = nextInputs;
        }

        const pruned = pruneFanInPorts(plan, node, entry.faninInputs);
        if (pruned.inputs) {
          node.inputs = pruned.inputs;
        }
        if (pruned.removed.size > 0 && node.info?.values) {
          const nextValues = { ...node.info.values };
          let updatedValues = false;
          for (const port of pruned.removed) {
            if (Object.prototype.hasOwnProperty.call(nextValues, port)) {
              delete nextValues[port];
              updatedValues = true;
            }
          }
          if (updatedValues) {
            node.info = { ...(node.info ?? { id: node.id, location: { x: 0, y: 0 } }), values: nextValues };
          }
        }
      }
      const nodeValues = node.info?.values;
      if (nodeValues && node.inputs) {
        let updated = false;
        const nextValues = { ...nodeValues };
        for (const [port, value] of Object.entries(nodeValues)) {
          if (!value) continue;
          const resolvedType =
            getPortValue(node.inputs as Record<string, PipelineDataType> | null | undefined, port) ??
            getPortValue(entry.inputs, port);
          if (!resolvedType) continue;
          let nextValue = value;
          const variants = resolveEnumVariants(node, entry, port);
          if (variants.length > 0) {
            const mapped = mapEnumIndexToVariant(value.value, variants);
            if (mapped !== null) {
              nextValue = { ...value, dataType: resolvedType, value: mapped };
              updated = true;
            }
          }
          if (nextValue === value && isGenericDataType(value.dataType)) {
            nextValue = { ...value, dataType: resolvedType };
            updated = true;
          }
          nextValues[port] = nextValue;
        }
        if (updated) {
          node.info = { ...(node.info ?? { id: node.id, location: { x: 0, y: 0 } }), values: nextValues };
        }
      }

      const defaults = entry.metadata?.inputPorts ?? {};
      if (Object.keys(defaults).length > 0) {
        const nextValues = { ...(node.info?.values ?? {}) };
        let updatedDefaults = false;
        for (const [port, meta] of Object.entries(defaults)) {
          if (!meta || meta.defaultValue === undefined) continue;
          if (Object.prototype.hasOwnProperty.call(nextValues, port)) continue;
          const resolvedType = getPortValue(node.inputs as Record<string, PipelineDataType> | null | undefined, port) ?? entry.inputs?.[port] ?? 'Generic';
          nextValues[port] = { dataType: resolvedType, value: meta.defaultValue };
          updatedDefaults = true;
        }
        if (updatedDefaults) {
          node.info = { ...(node.info ?? { id: node.id, location: { x: 0, y: 0 } }), values: nextValues };
        }
      }

      const baseMetadata =
        node.metadata ?? { name: entry.metadata.name ?? node.backendId ?? 'Pipeline node' };
      let metadataChanged = false;
      if (entry.metadata?.summary && !baseMetadata.summary) {
        baseMetadata.summary = entry.metadata.summary;
        metadataChanged = true;
      }
      if (entry.metadata?.gpu && !baseMetadata.gpu) {
        baseMetadata.gpu = entry.metadata.gpu;
        metadataChanged = true;
      }
      const mergedInputPorts = mergePortMetadata(baseMetadata.inputPorts, entry.metadata?.inputPorts);
      if (mergedInputPorts) {
        baseMetadata.inputPorts = mergedInputPorts;
        metadataChanged = true;
      }
      const mergedOutputPorts = mergePortMetadata(baseMetadata.outputPorts, entry.metadata?.outputPorts);
      if (mergedOutputPorts) {
        baseMetadata.outputPorts = mergedOutputPorts;
        metadataChanged = true;
      }
      if (metadataChanged) {
        node.metadata = baseMetadata;
      }
    }

    if (isBoundaryBackendId(node.backendId)) {
      const direction = node.backendId?.toLowerCase() === PIPELINE_INPUT_BACKEND_ID.toLowerCase() ? 'output' : 'input';
      const portRecord = direction === 'input' ? node.inputs : node.outputs;
      if (portRecord) {
        const nextRecord = { ...portRecord };
        let updated = false;
        for (const [port, dataType] of Object.entries(portRecord)) {
          if (!isGenericDataType(dataType)) continue;
          const inferred = inferBoundaryPortType(plan, node.id, port, direction);
          if (!inferred) continue;
          nextRecord[port] = inferred;
          updated = true;
        }
        if (updated) {
          if (direction === 'input') {
            node.inputs = nextRecord;
          } else {
            node.outputs = nextRecord;
          }
        }
        const normalized = Object.entries(direction === 'input' ? node.inputs ?? {} : node.outputs ?? {});
        if (direction === 'input') {
          const outputs = { ...(plan.pipelineOutputs ?? {}) };
          for (const [port, dataType] of normalized) {
            if (!isGenericDataType(dataType)) {
              outputs[normalizePipelinePortName(port)] = dataType;
            }
          }
          plan.pipelineOutputs = outputs;
        } else {
          const inputs = { ...(plan.pipelineInputs ?? {}) };
          for (const [port, dataType] of normalized) {
            if (!isGenericDataType(dataType)) {
              inputs[normalizePipelinePortName(port)] = dataType;
            }
          }
          plan.pipelineInputs = inputs;
        }
      }
    }

    if (isHostBridgeBackendId(node.backendId)) {
      const direction = node.backendId?.toLowerCase() === 'io.host_bridge' ? 'output' : 'input';
      const portRecord = direction === 'input' ? node.inputs : node.outputs;
      if (portRecord) {
        const nextRecord = { ...portRecord };
        let updated = false;
        for (const [port, dataType] of Object.entries(portRecord)) {
          if (!isGenericDataType(dataType)) continue;
          const inferred = inferBoundaryPortType(plan, node.id, port, direction);
          if (!inferred) continue;
          nextRecord[port] = inferred;
          updated = true;
        }
        if (updated) {
          if (direction === 'input') {
            node.inputs = nextRecord;
          } else {
            node.outputs = nextRecord;
          }
        }
      }
    }

    if (currentStyle) {
      if (!node.metadata?.style) {
        node.metadata = {
          ...(node.metadata ?? {
            name: entry?.metadata?.name ?? node.backendId ?? 'Pipeline node'
          }),
          style: cloneStyle(currentStyle)
        } as typeof node.metadata;
      }
    } else {
      const entryStyle = normalizeStyle(entry?.metadata?.style);
      if (entryStyle) {
        node.metadata = {
          ...(node.metadata ?? {
            name: entry?.metadata?.name ?? node.backendId ?? 'Pipeline node'
          }),
          style: cloneStyle(entryStyle)
        } as typeof node.metadata;
      }
    }
    if (node.embedded) {
      hydratePlanWithLookup(node.embedded, lookup);
    }
  }
};

function resolveExistingStyle(node: PipelineGraphPlan['nodes'][string]): PipelineNodeStyle | null {
  const direct = normalizeStyle(node.metadata?.style);
  if (direct) {
    return direct;
  }
  const sourceStyle =
    ((node.source ?? null) as { metadata?: { style?: StyleSource } } | null)?.metadata?.style ?? null;
  return normalizeStyle(sourceStyle);
}

const normalizeStyle = (style: StyleSource): PipelineNodeStyle | null => {
  if (!style) return null;
  return normalizeNodeStyle(style) ?? null;
};

const cloneStyle = (style: PipelineNodeStyle): PipelineNodeStyle => ({
  bg_color: style.bg_color,
  border_color: style.border_color
});

export const hydrateGraphWithLookup = (
  plan: PipelineGraphPlan | null | undefined,
  lookup: PipelineRegistryLookup | null
): void => {
  hydratePlanWithLookup(plan, lookup);
};

export const hydrateGraphWithRegistry = (
  plan: PipelineGraphPlan | null | undefined,
  registryEntries: PipelineRegistryEntry[] | null | undefined
): void => {
  if (!plan || !registryEntries || registryEntries.length === 0) {
    return;
  }
  const lookup = buildRegistryLookup(registryEntries);
  hydratePlanWithLookup(plan, lookup);
};

export const hydratePipelinesWithRegistry = (
  pipelines: PipelineOverviewPipeline[] | null | undefined,
  registryEntries: PipelineRegistryEntry[] | null | undefined
): void => {
  if (!pipelines || pipelines.length === 0 || !registryEntries || registryEntries.length === 0) {
    return;
  }
  const lookup = buildRegistryLookup(registryEntries);
  for (const pipeline of pipelines) {
    hydratePlanWithLookup(pipeline.graph, lookup);
  }
};
