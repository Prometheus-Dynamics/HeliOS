import type { PipelineGraphNode, PipelineGraphPlan, PipelineNodeValue } from '$lib/types/pipeline';
import { normalizePipelinePortName } from '../boundary';
import type { DaedalusGraphPatch, DaedalusGraphPatchOp, DaedalusValue } from '../daedalusTypes';
import { UI_NODE_ID_KEY } from '../daedalusTypes';
import { encodeConstOverrideValue, encodeDaedalusValue } from './valueCodec';
import { resolveNodeOrder } from './graphNormalization';

export function buildDaedalusGraphPatch(
  plan: PipelineGraphPlan,
  nodeOverrides: Record<string, Record<string, PipelineNodeValue>>,
  previousOverrides?: Record<string, Record<string, PipelineNodeValue>> | null
): DaedalusGraphPatch {
  const current = nodeOverrides ?? {};
  const previous = previousOverrides ?? {};
  const ops: DaedalusGraphPatchOp[] = [];
  const nodeIds = new Set<string>([...Object.keys(current), ...Object.keys(previous)]);

  const resolveBaseValue = (
    node: PipelineGraphNode,
    port: string
  ): PipelineNodeValue | null => {
    const values = node.info?.values ?? {};
    const normalized = normalizePipelinePortName(port);
    const direct = values[port];
    if (direct) return direct;
    const matchKey = Object.keys(values).find((key) => normalizePipelinePortName(key) === normalized);
    return matchKey ? values[matchKey] ?? null : null;
  };

  Array.from(nodeIds)
    .sort((a, b) => a.localeCompare(b))
    .forEach((nodeId) => {
      const node = plan.nodes?.[nodeId];
      if (!node) return;
      const rawMetadataValue = (
        node.source as unknown as { metadata?: Record<string, DaedalusValue> } | null | undefined
      )?.metadata?.[UI_NODE_ID_KEY];
      const metadataValue =
        rawMetadataValue && typeof rawMetadataValue === 'object' ? rawMetadataValue : encodeDaedalusValue(nodeId);
      const shouldIncludeIndex =
        node?.source && typeof node.source === 'object' && typeof nodeId === 'string' && nodeId.trim().length > 0;
      const parsedIndex = shouldIncludeIndex && /^\d+$/.test(nodeId.trim()) ? Number.parseInt(nodeId.trim(), 10) : null;
      const currentPorts = current[nodeId] ?? {};
      const previousPorts = previous[nodeId] ?? {};
      const portKeys = new Set<string>([...Object.keys(currentPorts), ...Object.keys(previousPorts)]);
      Array.from(portKeys)
        .sort((a, b) => a.localeCompare(b))
        .forEach((portKey) => {
          const normalized = normalizePipelinePortName(portKey);
          if (!normalized) return;
          const currentValue = currentPorts[normalized] ?? currentPorts[portKey];
          const previousValue = previousPorts[normalized] ?? previousPorts[portKey];
          if (!currentValue && !previousValue) return;
          const encoded = (() => {
            if (currentValue) {
              return encodeConstOverrideValue(normalized, currentValue, node.inputs, node.metadata?.inputPorts);
            }
            if (previousValue) {
              const baseValue = resolveBaseValue(node, normalized);
              if (baseValue) {
                return encodeConstOverrideValue(normalized, baseValue, node.inputs, node.metadata?.inputPorts);
              }
            }
            return null;
          })();
          ops.push({
            type: 'set_node_const',
            node: {
              ...(parsedIndex != null ? { index: parsedIndex } : {}),
              metadata: { key: UI_NODE_ID_KEY, value: metadataValue }
            },
            port: normalized,
            value: encoded
          });
        });
    });

  return { version: 1, ops };
}

export function resolveGraphNodeOrder(plan: PipelineGraphPlan): string[] {
  return resolveNodeOrder(plan);
}
