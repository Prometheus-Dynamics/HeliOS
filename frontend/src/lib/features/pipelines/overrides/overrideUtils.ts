import type { PipelineNodeValue } from '$lib/types/pipeline';

export function normalizePortKey(value: string): string {
  return value.trim().toLowerCase();
}

export function nodeValueSignature(value: PipelineNodeValue | null | undefined): string {
  if (!value) return '';
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

export function buildOverrideSignature(options: {
  streamId: string;
  pipelineId?: string | null;
  inputOverrides: Record<string, PipelineNodeValue>;
  nodeOverrides: Record<string, Record<string, PipelineNodeValue>>;
  normalizeKey?: (key: string) => string;
}): string {
  const normalizeKey = options.normalizeKey ?? ((value: string) => value);
  const inputs = Object.entries(options.inputOverrides ?? {})
    .map(([key, value]) => [normalizeKey(key), nodeValueSignature(value)] as const)
    .filter((entry): entry is [string, string] => Boolean(entry[0]))
    .sort((a, b) => a[0].localeCompare(b[0]));
  const nodes = Object.entries(options.nodeOverrides ?? {})
    .map(([nodeId, overrides]) => {
      const ports = Object.entries(overrides ?? {})
        .map(([portKey, value]) => [normalizeKey(portKey), nodeValueSignature(value)] as const)
        .filter((entry): entry is [string, string] => Boolean(entry[0]))
        .sort((a, b) => a[0].localeCompare(b[0]));
      return [nodeId, ports] as [string, Array<[string, string]>];
    })
    .sort((a, b) => a[0].localeCompare(b[0]));
  return JSON.stringify({
    streamId: options.streamId,
    pipelineId: options.pipelineId ?? null,
    inputs,
    nodes
  });
}
