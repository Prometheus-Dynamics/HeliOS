<script lang="ts">
  import type { ComponentProps } from 'svelte';
  import type { Writable } from 'svelte/store';
  import { PipelineRegistryPanel } from '$lib';

  type RegistryPanelProps = ComponentProps<typeof PipelineRegistryPanel>;
  type PipelineRegistryStores = {
    entries: Writable<RegistryPanelProps['registryEntries']>;
    search: Writable<RegistryPanelProps['searchTerm']>;
    tag: Writable<RegistryPanelProps['selectedTag']>;
    category: Writable<RegistryPanelProps['selectedCategory']>;
    provider: Writable<RegistryPanelProps['selectedProvider']>;
    sort: Writable<RegistryPanelProps['sort']>;
    view: Writable<RegistryPanelProps['view']>;
    activeGroup: Writable<RegistryPanelProps['activeGroup']>;
    loading: Writable<RegistryPanelProps['loading']>;
    error: Writable<RegistryPanelProps['error']>;
    availableTags: Writable<RegistryPanelProps['availableTags']>;
    availableCategories: Writable<RegistryPanelProps['availableCategories']>;
    availableProviders: Writable<RegistryPanelProps['availableProviders']>;
    hasActiveFilters: Writable<RegistryPanelProps['hasActiveFilters']>;
    groups: Writable<RegistryPanelProps['registryGroups']>;
    visibleEntries: Writable<RegistryPanelProps['visibleEntries']>;
  };

  type PipelineRegistryDrawerProps = {
    open?: boolean;
    stores: PipelineRegistryStores;
    onClose?: () => void;
    onRefresh?: () => void;
    onReset?: () => void;
    onSearch?: (term: string) => void;
    onSelectTag?: (tag: string | null) => void;
    onSelectCategory?: (category: string | null) => void;
    onSelectProvider?: (provider: string | null) => void;
    onSelectGroup?: (group: string | null) => void;
    onChangeSort?: (sort: 'name-asc' | 'name-desc' | 'id-asc') => void;
    onChangeView?: (view: 'grid' | 'table') => void;
    onAddEntry?: (entryId: string) => void;
  };

  let {
    open = false,
    stores,
    onClose = () => {},
    onRefresh = () => {},
    onReset = () => {},
    onSearch = () => {},
    onSelectTag = () => {},
    onSelectCategory = () => {},
    onSelectProvider = () => {},
    onSelectGroup = () => {},
    onChangeSort = () => {},
    onChangeView = () => {},
    onAddEntry = () => {}
  }: PipelineRegistryDrawerProps = $props();

  const registry = $derived.by(() => stores.entries);
  const registrySearch = $derived.by(() => stores.search);
  const registryTag = $derived.by(() => stores.tag);
  const registryCategory = $derived.by(() => stores.category);
  const registryProvider = $derived.by(() => stores.provider);
  const registrySort = $derived.by(() => stores.sort);
  const registryView = $derived.by(() => stores.view);
  const activeRegistryGroup = $derived.by(() => stores.activeGroup);
  const registryLoading = $derived.by(() => stores.loading);
  const registryError = $derived.by(() => stores.error);
  const availableTags = $derived.by(() => stores.availableTags);
  const availableCategories = $derived.by(() => stores.availableCategories);
  const availableProviders = $derived.by(() => stores.availableProviders);
  const hasActiveRegistryFilters = $derived.by(() => stores.hasActiveFilters);
  const registryGroups = $derived.by(() => stores.groups);
  const visibleRegistryEntries = $derived.by(() => stores.visibleEntries);

  export type $$Props = PipelineRegistryDrawerProps;
</script>

{#if open}
  <button
    type="button"
    class="fixed inset-0 z-40 bg-surface-950/70 backdrop-blur-sm transition-opacity"
    onclick={onClose}
    aria-label="Close node registry"
  ></button>
{/if}

<div
  id="pipeline-registry-drawer"
  class={`fixed inset-y-0 right-0 z-50 flex w-full max-w-3xl 2xl:max-w-4xl transform transition-transform duration-200 ${
    open ? 'translate-x-0' : 'translate-x-full'
  }`}
>
  <div class="flex h-full w-full flex-col border-l border-surface-800/70 bg-surface-950/95 shadow-2xl shadow-black/40">
    <header class="flex items-start justify-between gap-4 border-b border-surface-800/70 bg-surface-950/90 px-5 py-4">
      <div>
        <p class="text-xs font-semibold uppercase tracking-[0.22em] text-white">Node Registry</p>
        <p class="mt-1.5 text-micro-tight leading-snug text-surface-400">
          Browse, search, and filter nodes to add to your pipeline graph.
        </p>
      </div>
      <button
        class="btn btn-3xs preset-outline uppercase tracking-[0.2em]"
        type="button"
        onclick={onClose}
      >
        Close
      </button>
    </header>
    <div class="flex-1 overflow-y-auto px-4 py-4">
      <PipelineRegistryPanel
        searchTerm={$registrySearch}
        selectedTag={$registryTag}
        selectedCategory={$registryCategory}
        selectedProvider={$registryProvider}
        activeGroup={$activeRegistryGroup}
        sort={$registrySort}
        view={$registryView}
        availableTags={$availableTags}
        availableCategories={$availableCategories}
        availableProviders={$availableProviders}
        registryGroups={$registryGroups}
        registryEntries={$registry}
        visibleEntries={$visibleRegistryEntries}
        totalEntries={$registry.length}
        loading={$registryLoading}
        error={$registryError}
        hasActiveFilters={$hasActiveRegistryFilters}
        on:refresh={onRefresh}
        on:reset={onReset}
        on:search={(event) => onSearch(event.detail.term)}
        on:selectTag={(event) => onSelectTag(event.detail.tag)}
        on:selectCategory={(event) => onSelectCategory(event.detail.category)}
        on:selectProvider={(event) => onSelectProvider(event.detail.provider)}
        on:selectGroup={(event) => onSelectGroup(event.detail.group)}
        on:changeSort={(event) => onChangeSort(event.detail.sort)}
        on:changeView={(event) => onChangeView(event.detail.view)}
        on:addEntry={(event) => onAddEntry(event.detail.id)}
      />
    </div>
  </div>
</div>
