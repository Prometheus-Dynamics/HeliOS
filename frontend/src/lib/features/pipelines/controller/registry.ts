import { get, writable } from 'svelte/store';
import type { PipelineRegistryEntry } from '$lib/types/pipeline';
import type { GraphContextMenuState } from './types';
import { createRegistryFilterState } from './state';

export const normalizeRegistryBackendId = (value: string | null | undefined): string | null => {
  if (!value) return null;
  const lower = value.toLowerCase();
  const atIndex = lower.indexOf('@');
  return atIndex >= 0 ? lower.slice(0, atIndex) : lower;
};

export const resolveRegistryEntryForBackendId = (
  backendId: string | null | undefined,
  entries: PipelineRegistryEntry[]
): PipelineRegistryEntry | null => {
  if (!backendId) return null;
  if (entries.length === 0) return null;
  const lower = backendId.toLowerCase();
  const direct = entries.find((entry) => entry.id.toLowerCase() === lower);
  if (direct) return direct;
  const normalized = normalizeRegistryBackendId(backendId);
  if (normalized) {
    const normalizedMatch = entries.find((entry) => normalizeRegistryBackendId(entry.id) === normalized);
    if (normalizedMatch) return normalizedMatch;
  }
  return null;
};

export const createPipelineRegistryState = () => {
  const graphContextMenu = writable<GraphContextMenuState>({ visible: false, port: null });
  const graphContextSearch = writable('');
  const contextRegistryOptions = writable<PipelineRegistryEntry[]>([]);
  const registry = writable<PipelineRegistryEntry[]>([]);
  const registrySearch = writable('');
  const registryTag = writable<string | null>(null);
  const registryCategory = writable<string | null>(null);
  const registryProvider = writable<string | null>(null);
  const registrySort = writable<'name-asc' | 'name-desc' | 'id-asc'>('name-asc');
  const registryView = writable<'table' | 'grid'>('grid');
  const activeRegistryGroup = writable('all');
  const registryLoading = writable(false);
  const registryError = writable<string | null>(null);

  const {
    availableTags,
    availableCategories,
    availableProviders,
    hasActiveRegistryFilters,
    registryEntriesFiltered,
    visibleRegistryEntries,
    registryGroups,
    dispose: disposeRegistryFilters
  } = createRegistryFilterState({
    registry,
    registrySearch,
    registryTag,
    registryCategory,
    registryProvider,
    activeRegistryGroup,
    registrySort
  });

  const unsubscribe = graphContextSearch.subscribe((term) => {
    const menu = get(graphContextMenu);
    if (!menu.visible || menu.mode !== 'registry') {
      contextRegistryOptions.set([]);
      return;
    }
    const entries = get(registryEntriesFiltered);
    const query = term.trim().toLowerCase();
    const matches = query
      ? entries.filter((entry) =>
          [
            entry.id,
            entry.metadata.name,
            entry.metadata.summary ?? '',
            ...(entry.metadata.tags ?? []),
            ...(entry.metadata.categories?.flat() ?? [])
          ]
            .join(' ')
            .toLowerCase()
            .includes(query)
        )
      : entries.slice();
    contextRegistryOptions.set(matches.slice(0, 12));
  });

  const setGraphContextSearch = (value: string) => graphContextSearch.set(value);

  const updateRegistryFilters = (options: {
    search?: string;
    tag?: string | null;
    category?: string | null;
    provider?: string | null;
  }) => {
    if (options.search !== undefined) registrySearch.set(options.search);
    if (options.tag !== undefined) registryTag.set(options.tag);
    if (options.category !== undefined) registryCategory.set(options.category);
    if (options.provider !== undefined) registryProvider.set(options.provider);
  };

  const selectRegistryGroup = (groupKey: string) => {
    activeRegistryGroup.set(groupKey);
  };

  const resetRegistryFilters = () => {
    registrySearch.set('');
    registryTag.set(null);
    registryCategory.set(null);
    registryProvider.set(null);
    activeRegistryGroup.set('all');
    registrySort.set('name-asc');
  };

  const dispose = () => {
    unsubscribe();
    disposeRegistryFilters();
  };

  return {
    graphContextMenu,
    graphContextSearch,
    contextRegistryOptions,
    registry,
    registrySearch,
    registryTag,
    registryCategory,
    registryProvider,
    registrySort,
    registryView,
    activeRegistryGroup,
    registryLoading,
    registryError,
    availableTags,
    availableCategories,
    availableProviders,
    hasActiveRegistryFilters,
    registryEntriesFiltered,
    visibleRegistryEntries,
    registryGroups,
    setGraphContextSearch,
    updateRegistryFilters,
    selectRegistryGroup,
    resetRegistryFilters,
    dispose
  };
};
