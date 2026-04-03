import { getDataTypeVariants, resolveDataTypeKey } from '$lib/features/pipelines/valueFormatting';
import type {
  PipelineDataType,
  PipelineGraphPlan,
  PipelineNodeValue,
  PipelinePortMetadata,
  PipelineRegistryEntry
} from '$lib/types/pipeline';
import type { PipelineTuningConstantEntry } from '$lib/components/pipelines/types';
import {
  extractNodeOverridesFromGraph,
  isPipelineNodeValue,
  isRecord,
  normalizePortKey,
  portMetadataFromFlatKeys
} from './pipelineTuneHelpers';

const GENERIC_TYPE_KEYS = new Set(['generic', 'any', 'unknown', 'dynamic']);

export type ResolveRegistryEntry = (
  node: PipelineGraphPlan['nodes'][string]
) => Pick<PipelineRegistryEntry, 'inputs' | 'metadata'> | null;

const portTypeFor = (
  ports: Record<string, PipelineDataType> | null | undefined,
  portKey: string
): PipelineDataType | null => {
  if (!ports) return null;
  const normalized = normalizePortKey(portKey);
  if (!normalized) return null;
  const exact = ports[portKey];
  if (exact) return exact;
  if (ports[normalized]) return ports[normalized] ?? null;
  const resolvedKey = Object.keys(ports).find((key) => normalizePortKey(key) === normalized);
  return resolvedKey ? ports[resolvedKey] ?? null : null;
};

const mergeDataType = (
  base: PipelineTuningConstantEntry['dataType'],
  candidate: PipelineTuningConstantEntry['dataType']
): PipelineTuningConstantEntry['dataType'] => {
  if (!base) return candidate ?? null;
  if (!candidate) return base ?? null;

  const baseVariants = getDataTypeVariants(base ?? undefined);
  const candidateVariants = getDataTypeVariants(candidate ?? undefined);
  if (candidateVariants.length > 0) {
    if (baseVariants.length === 0) return candidate;
    if (baseVariants.join('|') !== candidateVariants.join('|')) return candidate;
  }

  const baseKey = (resolveDataTypeKey(base ?? undefined) ?? '').toLowerCase();
  const candidateKey = (resolveDataTypeKey(candidate ?? undefined) ?? '').toLowerCase();
  if (GENERIC_TYPE_KEYS.has(baseKey) && candidateKey && !GENERIC_TYPE_KEYS.has(candidateKey)) {
    return candidate;
  }

  return base;
};

export function portMetadataForConstant(
  node: PipelineGraphPlan['nodes'][string] | undefined,
  portKey: string,
  resolveRegistryEntryForNode: ResolveRegistryEntry
): PipelinePortMetadata | null {
  if (!node) return null;
  const normalized = portKey.trim();
  const inputPorts = node.metadata?.inputPorts ?? {};
  const nodeMeta =
    inputPorts[normalized] ??
    inputPorts[normalized.toLowerCase()] ??
    (node.metadata ? portMetadataFromFlatKeys(node.metadata, normalized, 'inputs') : null);
  const registryEntry = resolveRegistryEntryForNode(node);
  const registryPorts = registryEntry?.metadata?.inputPorts ?? {};
  const registryMeta =
    registryPorts[normalized] ??
    registryPorts[normalized.toLowerCase()] ??
    (registryEntry?.metadata
      ? portMetadataFromFlatKeys(registryEntry.metadata as Record<string, unknown>, normalized, 'inputs')
      : null);
  if (!registryMeta) return nodeMeta ?? null;
  if (!nodeMeta) return registryMeta ?? null;
  return { ...registryMeta, ...nodeMeta };
}

export function extractInputValues(graph: unknown): Record<string, PipelineNodeValue> {
  if (!isRecord(graph)) return {};
  const raw = (graph.pipelineInputValues ?? graph.pipeline_input_values) as unknown;
  if (!isRecord(raw)) return {};
  const normalized: Record<string, PipelineNodeValue> = {};
  for (const [key, value] of Object.entries(raw)) {
    const normalizedKey = normalizePortKey(key);
    if (!normalizedKey) continue;
    if (!isPipelineNodeValue(value)) continue;
    normalized[normalizedKey] = value;
  }
  return normalized;
}

export function safeClonePlan(plan: PipelineGraphPlan): PipelineGraphPlan {
  if (typeof structuredClone === 'function') {
    try {
      return structuredClone(plan);
    } catch (error) {
      if (error instanceof DOMException && error.name === 'DataCloneError') {
        // fall through to JSON clone
      } else {
        throw error;
      }
    }
  }
  return JSON.parse(JSON.stringify(plan)) as PipelineGraphPlan;
}

export const isDaedalusPlan = (plan: PipelineGraphPlan | null | undefined): boolean =>
  Boolean(plan?.format === 'daedalus');

export function extractTuneConstantEntries(
  plan: PipelineGraphPlan,
  resolveRegistryEntryForNode: ResolveRegistryEntry
): PipelineTuningConstantEntry[] {
  const entries: PipelineTuningConstantEntry[] = [];
  const nodeOverrides = extractNodeOverridesFromGraph(plan);

  for (const [nodeId, node] of Object.entries(plan.nodes ?? {})) {
    const resolvedValues = node.info?.values ?? null;
    if (!resolvedValues || typeof resolvedValues !== 'object') continue;
    const nodeLabel = node.metadata?.name ?? nodeId;
    const overrideRecord = nodeOverrides?.[nodeId] ?? {};
    const registryEntry = resolveRegistryEntryForNode(node);
    for (const [portKey, value] of Object.entries(resolvedValues)) {
      const normalizedPort = normalizePortKey(portKey);
      if (!normalizedPort) continue;
      const overrideValue = overrideRecord?.[normalizedPort] ?? null;
      const nodeInputType = portTypeFor(node.inputs ?? null, portKey);
      const registryInputType = portTypeFor(registryEntry?.inputs ?? null, portKey);
      entries.push({
        nodeId,
        nodeLabel,
        portKey,
        dataType: mergeDataType(
          mergeDataType(value?.dataType ?? null, nodeInputType),
          registryInputType
        ),
        baseValue: value ?? null,
        overrideValue,
        metadata: portMetadataForConstant(node, portKey, resolveRegistryEntryForNode)
      });
    }
  }
  return entries.sort((a, b) => {
    const nodeOrder = a.nodeLabel.localeCompare(b.nodeLabel);
    return nodeOrder !== 0 ? nodeOrder : a.portKey.localeCompare(b.portKey);
  });
}
