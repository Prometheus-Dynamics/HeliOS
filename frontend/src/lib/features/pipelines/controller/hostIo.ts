import type { PipelineGraphNode, PipelineGraphPlan } from '$lib/types/pipeline';
import { PIPELINE_INPUT_BACKEND_ID, PIPELINE_OUTPUT_BACKEND_ID } from '../boundary';

export const isHostBridgeBackendId = (backendId: string): boolean =>
  backendId === 'io.host_bridge' || backendId.endsWith(':io.host_bridge') || backendId === PIPELINE_INPUT_BACKEND_ID;

export const isHostOutputBackendId = (backendId: string): boolean =>
  backendId === 'io.host_output' || backendId.endsWith(':io.host_output') || backendId === PIPELINE_OUTPUT_BACKEND_ID;

export const resolveHostIoDirection = (
  node: PipelineGraphNode | null | undefined
): 'input' | 'output' | null => {
  const backendId = (node?.backendId ?? '').toLowerCase();
  if (isHostBridgeBackendId(backendId)) return 'input';
  if (isHostOutputBackendId(backendId)) return 'output';
  return null;
};

export const findHostIoNodeId = (
  plan: PipelineGraphPlan | null | undefined,
  direction: 'input' | 'output'
): string | null => {
  if (!plan) return null;
  const candidates: Array<{ id: string; isHost: boolean; portCount: number }> = [];
  Object.entries(plan.nodes ?? {}).forEach(([id, node]) => {
    if (!node) return;
    const nodeDirection = resolveHostIoDirection(node);
    if (nodeDirection !== direction) return;
    const backendId = (node.backendId ?? '').toLowerCase();
    const isPipeline =
      direction === 'input'
        ? backendId === PIPELINE_INPUT_BACKEND_ID
        : backendId === PIPELINE_OUTPUT_BACKEND_ID;
    const record = direction === 'input' ? node.outputs ?? {} : node.inputs ?? {};
    candidates.push({ id, isHost: isPipeline, portCount: Object.keys(record).length });
  });
  if (candidates.length === 0) return null;
  candidates.sort((a, b) => {
    if (a.isHost !== b.isHost) return a.isHost ? -1 : 1;
    return b.portCount - a.portCount;
  });
  return candidates[0]?.id ?? null;
};

export const ensureHostIoNodeShape = (node: PipelineGraphNode, direction: 'input' | 'output') => {
  const backendId = (node.backendId ?? '').toLowerCase();
  if (direction === 'input' && (backendId === 'io.host_bridge' || backendId.endsWith(':io.host_bridge'))) {
    node.backendId = PIPELINE_INPUT_BACKEND_ID;
  }
  if (direction === 'output' && (backendId === 'io.host_output' || backendId.endsWith(':io.host_output'))) {
    node.backendId = PIPELINE_OUTPUT_BACKEND_ID;
  }
  if (!node.metadata?.name?.trim()) {
    node.metadata = { ...(node.metadata ?? {}), name: direction === 'input' ? 'Pipeline Input' : 'Pipeline Output' };
  }
  const targetId = direction === 'input' ? PIPELINE_INPUT_BACKEND_ID : PIPELINE_OUTPUT_BACKEND_ID;
  node.info = { ...(node.info ?? { id: targetId, location: { x: 0, y: 0 } }), id: targetId };
};
