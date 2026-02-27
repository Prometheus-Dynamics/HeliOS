import type { PipelineNodeValue } from '$lib/types/pipeline';
import { buildOverrideSignature, normalizePortKey, nodeValueSignature } from '$lib/features/pipelines/overrides/overrideUtils';

export function pipelineOverrideSignature(
  streamId: string,
  pipelineId: string,
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

export { nodeValueSignature };
