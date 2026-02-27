import type { PipelineRegistryEntry } from '$lib/types/pipeline';

export type ProcessedRegistry = {
  entries: PipelineRegistryEntry[];
};

const clonePortRecord = (
  ports: Record<string, unknown> | undefined
): Record<string, unknown> | undefined => {
  if (!ports) return undefined;
  const cloneValue = (value: unknown) => {
    if (typeof structuredClone === 'function') {
      try {
        return structuredClone(value);
      } catch {
        // fall through
      }
    }
    try {
      return JSON.parse(JSON.stringify(value)) as unknown;
    } catch {
      return value;
    }
  };
  return Object.fromEntries(
    Object.entries(ports).map(([key, value]) => [
      key,
      typeof value === 'object' && value !== null ? cloneValue(value) : value
    ])
  );
};

export function buildRegistryVariants(rawEntries: PipelineRegistryEntry[]): ProcessedRegistry {
  const entries = rawEntries.map((entry) => ({
    id: entry.id,
    metadata: {
      name: entry.metadata.name,
      summary: entry.metadata.summary,
      categories: entry.metadata.categories ? entry.metadata.categories.map((path) => path.slice()) : undefined,
      tags: entry.metadata.tags ? entry.metadata.tags.slice() : undefined,
      provider: entry.metadata.provider,
      gpu: entry.metadata.gpu ? { ...entry.metadata.gpu } : undefined,
      inputPorts: entry.metadata.inputPorts ? clonePortRecord(entry.metadata.inputPorts) : undefined,
      outputPorts: entry.metadata.outputPorts ? clonePortRecord(entry.metadata.outputPorts) : undefined
    },
    inputs: clonePortRecord(entry.inputs) as PipelineRegistryEntry['inputs'],
    outputs: clonePortRecord(entry.outputs) as PipelineRegistryEntry['outputs'],
    faninInputs: entry.faninInputs
      ? entry.faninInputs.map((fanin) => ({
          prefix: fanin.prefix,
          start: fanin.start,
          dataType: fanin.dataType ?? 'Generic'
        }))
      : undefined
  }));

  entries.sort((a, b) => a.metadata.name.localeCompare(b.metadata.name));
  return { entries };
}
