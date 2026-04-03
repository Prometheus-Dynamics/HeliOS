/// <reference lib="webworker" />

import { normalizeDaedalusRegistry } from '$lib/features/pipelines/controller/daedalusRegistry';
import { cloneRegistryEntry } from '$lib/features/pipelines/cloneHelpers';
import { buildRegistryVariants } from '$lib/features/pipelines/registryUtils';
import type { DaedalusRegistryNode } from '$lib/api/client';
import type { PipelineRegistryEntry, PipelineTypeDescriptor } from '$lib/types/pipeline';
import type { DaedalusRegistryType } from '$lib/features/pipelines/controller/daedalusRegistry/types';

type RegistryNormalizeRequest = {
  requestId: number;
  nodes?: DaedalusRegistryNode[];
  types?: DaedalusRegistryType[] | null;
  palette?: Record<string, PipelineTypeDescriptor>;
};

type RegistryFilterRequest = {
  requestId: number;
  entries?: PipelineRegistryEntry[];
  search?: string;
  tag?: string | null;
  category?: string | null;
  provider?: string | null;
  activeGroup?: string;
  sort?: 'name-asc' | 'name-desc' | 'id-asc';
};

type RegistryNormalizeResponse = {
  requestId: number;
  entries: PipelineRegistryEntry[];
  error?: string;
};

type RegistryFilterResponse = {
  requestId: number;
  availableTags: string[];
  availableCategories: string[];
  availableProviders: string[];
  hasActiveFilters: boolean;
  entriesFiltered: PipelineRegistryEntry[];
  visibleEntries: PipelineRegistryEntry[];
  groups: { key: string; label: string; entries: PipelineRegistryEntry[] }[];
  error?: string;
};

const ctx: DedicatedWorkerGlobalScope = self as unknown as DedicatedWorkerGlobalScope;

function normalizeString(value: string | null | undefined): string {
  return (value ?? '').trim().toLowerCase();
}

function handleNormalizeRequest(payload: RegistryNormalizeRequest): RegistryNormalizeResponse {
  const { requestId, nodes, types, palette } = payload;
  const paletteMap = (palette ?? {}) as Record<string, PipelineTypeDescriptor>;
  const normalized = normalizeDaedalusRegistry(nodes ?? [], types ?? null);
  const processed = buildRegistryVariants(normalized);
  const entries = processed.entries.map((entry) => cloneRegistryEntry(entry, paletteMap));
  return {
    requestId,
    entries
  };
}

function handleFilterRequest(payload: RegistryFilterRequest): RegistryFilterResponse {
  const entries = Array.isArray(payload.entries) ? payload.entries : [];
  const search = payload.search ?? '';
  const tag = payload.tag ?? null;
  const category = payload.category ?? null;
  const provider = payload.provider ?? null;
  const activeGroup = payload.activeGroup ?? 'all';
  const sort = payload.sort ?? 'name-asc';
  const tags = new Set<string>();
  const categories = new Set<string>();
  const providers = new Set<string>();

  const displayEntries = entries.filter((entry) => {
    const entryProvider = normalizeString(entry.metadata.provider);
    if (entryProvider && /^(io|host)(?:$|[-_:._ ])/.test(entryProvider)) return false;
    const id = normalizeString(entry.id);
    if (/^(io|host)[-_:._]/.test(id)) return false;
    return true;
  });

  displayEntries.forEach((entry) => {
    entry.metadata.tags?.forEach((entryTag) => tags.add(entryTag));
    entry.metadata.categories?.forEach((group) => group.forEach((entryCategory) => categories.add(entryCategory)));
    if (entry.metadata.provider) providers.add(entry.metadata.provider);
  });

  const availableTags = Array.from(tags).sort((a, b) => a.localeCompare(b));
  const availableCategories = Array.from(categories).sort((a, b) => a.localeCompare(b));
  const availableProviders = Array.from(providers).sort((a, b) => a.localeCompare(b));
  const normalizedSearch = normalizeString(search);
  const normalizedCategory = normalizeString(category);
  const normalizedProvider = normalizeString(provider);
  const entriesFiltered = displayEntries.filter((entry) => {
    if (tag && !(entry.metadata.tags ?? []).includes(tag)) return false;
    if (normalizedCategory) {
      const match = (entry.metadata.categories ?? []).some((path) =>
        path.some((segment) => normalizeString(segment) === normalizedCategory)
      );
      if (!match) return false;
    }
    if (normalizedProvider) {
      const entryProvider = normalizeString(entry.metadata.provider);
      if (entryProvider !== normalizedProvider) return false;
    }
    if (normalizedSearch) {
      const ports = [...Object.keys(entry.inputs ?? {}), ...Object.keys(entry.outputs ?? {})];
      const haystack = [
        entry.id,
        entry.metadata.name,
        entry.metadata.provider ?? '',
        ...(entry.metadata.tags ?? []),
        ...ports
      ]
        .join(' ')
        .toLowerCase();
      if (!haystack.includes(normalizedSearch)) return false;
    }
    return true;
  });

  const groups = new Map<string, { key: string; label: string; entries: PipelineRegistryEntry[] }>();
  entriesFiltered.forEach((entry) => {
    const primary = entry.metadata.categories?.find((path) => path.length > 0)?.[0] ?? null;
    const key = normalizeString(primary ?? 'uncategorized');
    const label = primary ?? 'Uncategorized';
    const bucket = groups.get(key);
    if (bucket) {
      bucket.entries.push(entry);
    } else {
      groups.set(key, { key, label, entries: [entry] });
    }
  });

  const visibleEntries = activeGroup && activeGroup !== 'all'
    ? entriesFiltered.filter((entry) => {
        const primary = entry.metadata.categories?.find((path) => path.length > 0)?.[0] ?? null;
        const key = normalizeString(primary ?? 'uncategorized');
        return key === normalizeString(activeGroup);
      })
    : entriesFiltered.slice();

  switch (sort) {
    case 'name-desc':
      visibleEntries.sort((a, b) => b.metadata.name.localeCompare(a.metadata.name));
      break;
    case 'id-asc':
      visibleEntries.sort((a, b) => a.id.localeCompare(b.id));
      break;
    default:
      visibleEntries.sort((a, b) => a.metadata.name.localeCompare(b.metadata.name));
      break;
  }

  return {
    requestId: payload.requestId,
    availableTags,
    availableCategories,
    availableProviders,
    hasActiveFilters: Boolean(tag || category || provider || (activeGroup && activeGroup !== 'all')),
    entriesFiltered,
    visibleEntries,
    groups: Array.from(groups.values()).map((group) => ({
      ...group,
      entries: group.entries.slice().sort((a, b) => a.metadata.name.localeCompare(b.metadata.name))
    }))
  };
}

ctx.onmessage = (event: MessageEvent<RegistryNormalizeRequest | RegistryFilterRequest>) => {
  const payload = event.data ?? ({} as RegistryNormalizeRequest | RegistryFilterRequest);
  const requestId = Number((payload as { requestId?: number }).requestId ?? 0);
  if (!Number.isFinite(requestId) || requestId <= 0) {
    return;
  }
  try {
    if ('nodes' in payload || 'types' in payload || 'palette' in payload) {
      ctx.postMessage(handleNormalizeRequest(payload as RegistryNormalizeRequest));
      return;
    }
    ctx.postMessage(handleFilterRequest(payload as RegistryFilterRequest));
  } catch (error) {
    const message = error instanceof Error ? error.message : 'pipeline registry worker failure';
    if ('nodes' in payload || 'types' in payload || 'palette' in payload) {
      const response: RegistryNormalizeResponse = { requestId, entries: [], error: message };
      ctx.postMessage(response);
      return;
    }
    const response: RegistryFilterResponse = {
      requestId,
      availableTags: [],
      availableCategories: [],
      availableProviders: [],
      hasActiveFilters: false,
      entriesFiltered: [],
      visibleEntries: [],
      groups: [],
      error: message
    };
    ctx.postMessage(response);
  }
};
