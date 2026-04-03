import type { StreamInfo } from '$lib/api/client';
import type { PipelineTuningConstantEntry, PipelineTuningConstantGroup } from '$lib/components/pipelines/types';

export const buildTuneConstantGroups = (
  tuneConstants: PipelineTuningConstantEntry[]
): PipelineTuningConstantGroup[] => {
  const groups = new Map<string, PipelineTuningConstantGroup>();
  for (const entry of tuneConstants) {
    const existing = groups.get(entry.nodeId);
    if (existing) {
      existing.entries.push(entry);
    } else {
      groups.set(entry.nodeId, { nodeId: entry.nodeId, nodeLabel: entry.nodeLabel, entries: [entry] });
    }
  }
  const result = Array.from(groups.values());
  result.sort((a, b) => a.nodeLabel.localeCompare(b.nodeLabel));
  result.forEach((group) => {
    group.entries.sort((a, b) => a.portKey.localeCompare(b.portKey));
  });
  return result;
};

export const buildTuneConstantSearchTokens = (search: string) =>
  search
    .trim()
    .toLowerCase()
    .split(/\s+/u)
    .filter((token) => token.length > 0);

export const filterTuneConstantGroups = (
  groups: PipelineTuningConstantGroup[],
  tokens: string[]
): PipelineTuningConstantGroup[] => {
  if (tokens.length === 0) return groups;
  const matches = (value: string): boolean => tokens.every((token) => value.includes(token));
  return groups
    .map((group) => {
      const groupKey = `${group.nodeLabel} ${group.nodeId}`.toLowerCase();
      if (matches(groupKey)) {
        return group;
      }
      const entries = group.entries.filter((entry) =>
        matches(`${entry.portKey} ${entry.metadata?.description ?? ''}`.toLowerCase())
      );
      if (!entries.length) return null;
      return { ...group, entries };
    })
    .filter((group): group is PipelineTuningConstantGroup => Boolean(group));
};

export const buildTuneMultiplexPalettePipelineIds = (options: {
  tunePreviewStream: StreamInfo | null;
  pipelineLabelById: (pipelineId: string) => string;
  RAW_STREAM_PIPELINE_ID: string;
  RAW_STREAM_PIPELINE_UUID: string;
}): string[] => {
  if (!options.tunePreviewStream) return [];
  const manifest = options.tunePreviewStream.manifest ?? null;
  const ids = new Set<string>();
  const normalize = (value: unknown): string | null => {
    if (typeof value !== 'string') return null;
    const trimmed = value.trim();
    if (!trimmed) return null;
    if (trimmed === options.RAW_STREAM_PIPELINE_UUID) return options.RAW_STREAM_PIPELINE_ID;
    return trimmed;
  };
  const add = (value: unknown) => {
    const normalized = normalize(value);
    if (normalized) ids.add(normalized);
  };

  add(manifest?.active_pipeline_id);
  if (Array.isArray(manifest?.pipelines)) {
    for (const binding of manifest.pipelines) {
      add(binding?.pipeline_id);
    }
  }
  const slots = Array.isArray(manifest?.pipeline_layout?.slots) ? manifest.pipeline_layout.slots : [];
  for (const slot of slots) {
    const raw = slot?.pipeline_id;
    if (raw === options.RAW_STREAM_PIPELINE_UUID) continue;
    add(raw);
  }

  ids.delete(options.RAW_STREAM_PIPELINE_ID);
  const result = Array.from(ids.values());
  result.sort((a, b) => options.pipelineLabelById(a).localeCompare(options.pipelineLabelById(b)));
  return result;
};

export const buildTuneMultiplexOutputOptionsCache = (options: {
  tuneMultiplexPalettePipelineIds: string[];
  tuneMultiplexSlots: Record<string, string | null>;
  outputOptionsForPipeline: (pipelineId: string | null) => string[];
}): Record<string, string[]> => {
  const ids = new Set<string>();
  options.tuneMultiplexPalettePipelineIds.forEach((id) => ids.add(id));
  Object.values(options.tuneMultiplexSlots).forEach((id) => {
    if (id) ids.add(id);
  });
  const map: Record<string, string[]> = {};
  ids.forEach((id) => {
    map[id] = options.outputOptionsForPipeline(id);
  });
  return map;
};
