import type { PipelineDataType, PipelineRegistryEntry } from '$lib/types/pipeline';
import type { ApiPortDescriptor } from '$lib/types/pipeline-api';
import { fromApiPortDescriptor } from '../graphConverters';
import { normalizeNodeStyle } from '../model';

function normalizeRegistryPorts(value: unknown): Record<string, PipelineDataType> | undefined {
  if (!value || typeof value !== 'object') return undefined;
  const result: Record<string, PipelineDataType> = {};
  Object.entries(value as Record<string, unknown>).forEach(([port, descriptor]) => {
    const trimmedPort = port.trim();
    if (!trimmedPort) return;
    const normalized =
      typeof descriptor === 'string'
        ? descriptor.trim()
          ? descriptor.trim()
          : null
        : fromApiPortDescriptor(descriptor as ApiPortDescriptor | null | undefined);
    if (normalized) {
      result[trimmedPort] = normalized;
    }
  });
  return Object.keys(result).length > 0 ? result : undefined;
}

export function normalizeRegistry(descriptors: unknown[]): PipelineRegistryEntry[] {
  return descriptors
    .map((descriptor) => {
      if (!descriptor || typeof descriptor !== 'object') return null;
      const record = descriptor as Record<string, unknown>;
      const id = typeof record.id === 'string' ? record.id : null;
      if (!id) return null;
      const metadataRaw = record.metadata as Record<string, unknown> | undefined;
      const name =
        typeof metadataRaw?.name === 'string' && metadataRaw.name.trim()
          ? metadataRaw.name.trim()
          : 'Unnamed node';
      const summaryText =
        typeof metadataRaw?.summary === 'string' && metadataRaw.summary.trim()
          ? metadataRaw.summary.trim()
          : undefined;
      const doc =
        typeof metadataRaw?.doc === 'string' && metadataRaw.doc.trim()
          ? metadataRaw.doc.trim()
          : undefined;
      const provider =
        typeof metadataRaw?.provider === 'string' && metadataRaw.provider.trim()
          ? metadataRaw.provider.trim()
          : undefined;
      const categories = Array.isArray(metadataRaw?.categories)
        ? (metadataRaw?.categories as unknown[])
            .filter(Array.isArray)
            .map((category) => (category as unknown[]).filter((value): value is string => typeof value === 'string'))
        : undefined;
      const tags = Array.isArray(metadataRaw?.tags)
        ? (metadataRaw?.tags as unknown[]).filter((tag): tag is string => typeof tag === 'string')
        : undefined;
      const style = normalizeNodeStyle(metadataRaw?.style);
      const inputs = normalizeRegistryPorts(record.inputs);
      const outputs = normalizeRegistryPorts(record.outputs);
      return {
        id,
        metadata: {
          name,
          summary: summaryText,
          ...(doc ? { doc } : {}),
          categories,
          tags,
          ...(provider ? { provider } : {}),
          ...(style ? { style } : {})
        },
        ...(inputs ? { inputs } : {}),
        ...(outputs ? { outputs } : {})
      } as PipelineRegistryEntry;
    })
    .filter((entry): entry is PipelineRegistryEntry => Boolean(entry));
}
