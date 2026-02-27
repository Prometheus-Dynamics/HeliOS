import type { PipelineGraphPlan, PipelineNodeValue, PipelinePortMetadata } from '$lib/types/pipeline';
import type { PipelineTuningConstantEntry } from '$lib/components/pipelines/types';
import {
  extractNodeOverridesFromGraph,
  isPipelineNodeValue,
  isRecord,
  normalizePortKey,
  portMetadataFromFlatKeys
} from './pipelineTuneState';

export type ResolveRegistryEntry = (node: PipelineGraphPlan['nodes'][string]) => { metadata?: { inputPorts?: Record<string, PipelinePortMetadata> } } | null;

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
  Boolean(plan && (plan.format === 'daedalus' || plan.daedalus));

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
    for (const [portKey, value] of Object.entries(resolvedValues)) {
      const normalizedPort = normalizePortKey(portKey);
      if (!normalizedPort) continue;
      const overrideValue = overrideRecord?.[normalizedPort] ?? null;
      entries.push({
        nodeId,
        nodeLabel,
        portKey,
        dataType: value?.dataType ?? null,
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
