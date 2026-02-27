import type { PipelineGraphPlan, PipelineRegistryEntry } from '$lib/types/pipeline';

type ResolveRegistryEntry = (
  node: PipelineGraphPlan['nodes'][string] | undefined
) => PipelineRegistryEntry | null;

const normalizeBackendId = (value: string | undefined | null): string | null => {
  if (!value) return null;
  const lower = value.toLowerCase();
  const atIndex = lower.indexOf('@');
  if (atIndex >= 0) {
    return lower.slice(0, atIndex);
  }
  return lower;
};

export const createRegistryResolver = (
  registryEntries: PipelineRegistryEntry[]
): ResolveRegistryEntry => {
  const directMap = new Map<string, PipelineRegistryEntry>();
  const normalizedMap = new Map<string, PipelineRegistryEntry>();
  for (const entry of registryEntries) {
    directMap.set(entry.id.toLowerCase(), entry);
    const normalized = normalizeBackendId(entry.id);
    if (normalized && !normalizedMap.has(normalized)) {
      normalizedMap.set(normalized, entry);
    }
  }

  const findBestMatch = (id: string | undefined | null): PipelineRegistryEntry | null => {
    if (!id) return null;
    const normalized = id.toLowerCase();
    let best: { entry: PipelineRegistryEntry; score: number } | null = null;
    for (const entry of registryEntries) {
      const entryId = entry.id.toLowerCase();
      if (entryId === normalized) {
        best = { entry, score: Number.POSITIVE_INFINITY };
        break;
      }
      if (normalized.includes(entryId)) {
        const score = entryId.length;
        if (!best || score > best.score) {
          best = { entry, score };
        }
      } else if (entryId.includes(normalized)) {
        const score = normalized.length / entryId.length;
        if (!best || score > best.score) {
          best = { entry, score };
        }
      }
    }
    return best ? best.entry : null;
  };

  return (node) => {
    if (!node) return null;
    const backendId = node.backendId?.toLowerCase();
    if (!backendId) {
      return null;
    }
    const direct = directMap.get(backendId);
    if (direct) {
      return direct;
    }
    const normalized = normalizeBackendId(node.backendId);
    if (normalized) {
      const resolved = normalizedMap.get(normalized);
      if (resolved) {
        return resolved;
      }
    }
    return findBestMatch(node.backendId);
  };
};
