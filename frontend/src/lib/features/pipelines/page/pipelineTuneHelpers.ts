import type { PipelineGraphPlan, PipelineNodeMetadata, PipelineNodeValue, PipelinePortMetadata } from '$lib/types/pipeline';
import { buildOverrideSignature, nodeValueSignature, normalizePortKey } from '$lib/features/pipelines/overrides/overrideUtils';

export { normalizePortKey, nodeValueSignature };

export const isRecord = (value: unknown): value is Record<string, unknown> =>
  Boolean(value && typeof value === 'object' && !Array.isArray(value));

export const isPipelineNodeValue = (value: unknown): value is PipelineNodeValue => {
  if (!isRecord(value)) return false;
  return 'dataType' in value && 'value' in value;
};

export function numberFromMetadata(value: unknown): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (value && typeof value === 'object') {
    const raw = (value as { value?: unknown }).value;
    if (typeof raw === 'number' && Number.isFinite(raw)) return raw;
  }
  return null;
}

export function portMetadataFromFlatKeys(
  metadata: Record<string, unknown> | PipelineNodeMetadata | null | undefined,
  portKey: string,
  direction: 'inputs' | 'outputs'
): PipelinePortMetadata | null {
  if (!metadata) return null;
  const normalized = normalizePortKey(portKey);
  const out: PipelinePortMetadata = {};
  const pattern = new RegExp(
    `^${direction}\\.([^.]*)\\.(description|allowed_values|allowedValues|min|max|step|ui_min|ui_max|ui_step|ui_control|ui)$`
  );
  for (const [key, value] of Object.entries(metadata as Record<string, unknown>)) {
    const match = key.match(pattern);
    if (!match?.[1] || !match[2]) continue;
    if (normalizePortKey(match[1]) !== normalized) continue;
    switch (match[2]) {
      case 'description': {
        if (typeof value === 'string' && value.trim()) out.description = value.trim();
        break;
      }
      case 'allowed_values':
      case 'allowedValues': {
        if (Array.isArray(value)) {
          const list = value
            .map((entry) => (typeof entry === 'string' ? entry.trim() : ''))
            .filter((entry) => entry.length > 0);
          if (list.length) out.allowedValues = list;
        }
        break;
      }
      case 'min': {
        const parsed = numberFromMetadata(value);
        if (parsed != null) out.min = parsed;
        break;
      }
      case 'max': {
        const parsed = numberFromMetadata(value);
        if (parsed != null) out.max = parsed;
        break;
      }
      case 'step': {
        const parsed = numberFromMetadata(value);
        if (parsed != null) out.step = parsed;
        break;
      }
      case 'ui_min': {
        const parsed = numberFromMetadata(value);
        if (parsed != null) out.uiMin = parsed;
        break;
      }
      case 'ui_max': {
        const parsed = numberFromMetadata(value);
        if (parsed != null) out.uiMax = parsed;
        break;
      }
      case 'ui_step': {
        const parsed = numberFromMetadata(value);
        if (parsed != null) out.uiStep = parsed;
        break;
      }
      case 'ui_control':
      case 'ui': {
        if (typeof value === 'string' && value.trim()) out.uiControl = value.trim();
        break;
      }
    }
  }
  return Object.keys(out).length > 0 ? out : null;
}

export function extractNodeOverridesFromGraph(graph: unknown): Record<string, Record<string, PipelineNodeValue>> {
  if (!isRecord(graph)) return {};
  const raw = (graph.nodeValueOverrides ?? graph.node_value_overrides ?? graph.overrides) as unknown;
  if (!isRecord(raw)) return {};
  const normalized: Record<string, Record<string, PipelineNodeValue>> = {};
  for (const [nodeId, overrides] of Object.entries(raw)) {
    if (!isRecord(overrides)) continue;
    const entry: Record<string, PipelineNodeValue> = {};
    for (const [portKey, value] of Object.entries(overrides)) {
      const normalizedPort = normalizePortKey(portKey);
      if (!normalizedPort) continue;
      if (!isPipelineNodeValue(value)) continue;
      entry[normalizedPort] = value;
    }
    if (Object.keys(entry).length > 0) {
      normalized[nodeId] = entry;
    }
  }
  return normalized;
}

export function mergeNodeOverrides(
  base: Record<string, Record<string, PipelineNodeValue>>,
  extra: Record<string, Record<string, PipelineNodeValue>>
): Record<string, Record<string, PipelineNodeValue>> {
  const merged: Record<string, Record<string, PipelineNodeValue>> = { ...(base ?? {}) };
  for (const [nodeId, overrides] of Object.entries(extra ?? {})) {
    merged[nodeId] = { ...(merged[nodeId] ?? {}), ...(overrides ?? {}) };
  }
  return merged;
}

export function diffDaedalusNodeValues(
  basePlan: PipelineGraphPlan,
  streamPlan: PipelineGraphPlan
): Record<string, Record<string, PipelineNodeValue>> {
  const overrides: Record<string, Record<string, PipelineNodeValue>> = {};
  for (const [nodeId, baseNode] of Object.entries(basePlan.nodes ?? {})) {
    const streamNode = streamPlan.nodes?.[nodeId];
    if (!streamNode) continue;
    const baseValues = baseNode.info?.values ?? {};
    const streamValues = streamNode.info?.values ?? {};

    const baseByNormalized = new Map<string, PipelineNodeValue>();
    for (const [key, value] of Object.entries(baseValues)) {
      const normalized = normalizePortKey(key);
      if (!normalized) continue;
      baseByNormalized.set(normalized, value);
    }

    for (const [key, value] of Object.entries(streamValues)) {
      const normalized = normalizePortKey(key);
      if (!normalized) continue;
      const baseValue = baseByNormalized.get(normalized);
      if (nodeValueSignature(value) === nodeValueSignature(baseValue)) continue;
      overrides[nodeId] = { ...(overrides[nodeId] ?? {}), [normalized]: value };
    }
  }
  return overrides;
}

export function streamOverrideSignature(
  pipelineId: string | null | undefined,
  streamId: string,
  inputOverrides: Record<string, PipelineNodeValue>,
  nodeOverrides: Record<string, Record<string, PipelineNodeValue>>
): string {
  return buildOverrideSignature({
    streamId,
    pipelineId,
    inputOverrides,
    nodeOverrides,
    normalizeKey: normalizePortKey
  });
}
