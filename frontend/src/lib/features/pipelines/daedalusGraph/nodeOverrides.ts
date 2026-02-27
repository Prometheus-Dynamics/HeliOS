import type { PipelineGraphPlan, PipelineNodeValue } from '$lib/types/pipeline';
import { normalizePipelinePortName } from '../boundary';
import type { DaedalusGraphPatch, DaedalusGraphPatchNodeSelector, DaedalusValue } from '../daedalusTypes';
import { UI_NODE_ID_KEY } from '../daedalusTypes';
import { decodeDaedalusConstValue, decodeDaedalusValue, resolvePortEntry } from './valueCodec';
import { resolveNodeOrder } from './graphNormalization';

const resolvePatchNodeId = (
  selector: DaedalusGraphPatchNodeSelector | null | undefined,
  plan: PipelineGraphPlan
): string | null => {
  if (!selector) return null;
  const nodes = plan.nodes ?? {};
  const resolveExisting = (candidate: string | null | undefined): string | null => {
    if (!candidate) return null;
    const trimmed = candidate.trim();
    return trimmed && nodes[trimmed] ? trimmed : null;
  };
  const resolveByMetadata = (value: DaedalusValue): string | null => {
    const decoded = decodeDaedalusValue(value);
    if (typeof decoded === 'string' && decoded.trim().length > 0) {
      const direct = resolveExisting(decoded);
      if (direct) return direct;
      const match = Object.entries(nodes).find(([, node]) => {
        const raw = (
          node?.source as unknown as { metadata?: Record<string, DaedalusValue> } | null | undefined
        )?.metadata?.[UI_NODE_ID_KEY];
        if (!raw) return false;
        const rawDecoded = decodeDaedalusValue(raw);
        return typeof rawDecoded === 'string' && rawDecoded.trim() === decoded.trim();
      });
      if (match) return match[0];
    }
    if (typeof decoded === 'number' && Number.isFinite(decoded)) {
      const direct = resolveExisting(String(decoded));
      if (direct) return direct;
    }
    return null;
  };

  const meta = selector.metadata;
  if (meta && meta.key === UI_NODE_ID_KEY) {
    const resolved = resolveByMetadata(meta.value);
    if (resolved) return resolved;
  }

  if (typeof selector.index === 'number' && Number.isFinite(selector.index)) {
    const idx = Math.trunc(selector.index);
    const direct = resolveExisting(String(idx));
    if (direct) return direct;
    const order = resolveNodeOrder(plan);
    if (idx >= 0 && idx < order.length) {
      const candidate = order[idx] ?? null;
      const resolved = resolveExisting(candidate);
      if (resolved) return resolved;
    }
  }

  if (typeof selector.id === 'string' && selector.id.trim().length > 0) {
    const resolved = resolveExisting(selector.id);
    if (resolved) return resolved;
  }
  return null;
};

export function nodeOverridesFromDaedalusPatch(
  patch: DaedalusGraphPatch | null | undefined,
  plan: PipelineGraphPlan
): Record<string, Record<string, PipelineNodeValue>> {
  const overrides: Record<string, Record<string, PipelineNodeValue>> = {};
  if (!patch || !Array.isArray(patch.ops)) return overrides;
  for (const op of patch.ops) {
    if (!op || op.type !== 'set_node_const') continue;
    const nodeId = resolvePatchNodeId(op.node, plan);
    if (!nodeId) continue;
    const node = plan.nodes?.[nodeId];
    if (!node) continue;
    const port = typeof op.port === 'string' ? op.port : '';
    const normalized = normalizePipelinePortName(port);
    if (!normalized || op.value == null) continue;
    const portMeta = resolvePortEntry(node.metadata?.inputPorts ?? null, port);
    const value = decodeDaedalusConstValue(op.value, portMeta);
    overrides[nodeId] = { ...(overrides[nodeId] ?? {}), [normalized]: value };
  }
  return overrides;
}
