<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import type { PipelineRegistryGroup, PipelineRegistryView } from './types';
  import PipelineRegistryEntryPreview from './PipelineRegistryEntryPreview.svelte';
  import type { PipelineRegistryEntry } from '$lib/types/pipeline';
  import { getVirtualWindow, virtualViewport } from '$lib/ui/virtualViewport';

  let {
    searchTerm = $bindable(''),
    selectedTag = $bindable<string | null>(null),
    selectedCategory = $bindable<string | null>(null),
    selectedProvider = $bindable<string | null>(null),
    activeGroup = $bindable<string>('all'),
    sort = $bindable<'name-asc' | 'name-desc' | 'id-asc'>('name-asc'),
    view = $bindable<PipelineRegistryView>('table'),
    availableTags = [],
    availableCategories = [],
    availableProviders = [],
    registryGroups = [],
    registryEntries = [],
    visibleEntries = [],
    totalEntries = 0,
    loading = false,
    error = null,
    hasActiveFilters = false
  }: {
    searchTerm?: string;
    selectedTag?: string | null;
    selectedCategory?: string | null;
    selectedProvider?: string | null;
    activeGroup?: string;
    sort?: 'name-asc' | 'name-desc' | 'id-asc';
    view?: PipelineRegistryView;
    availableTags?: string[];
    availableCategories?: string[];
    availableProviders?: string[];
    registryGroups?: PipelineRegistryGroup[];
    registryEntries?: PipelineRegistryEntry[];
    visibleEntries?: PipelineRegistryEntry[];
    totalEntries?: number;
    loading?: boolean;
    error?: string | null;
    hasActiveFilters?: boolean;
  } = $props();

  let filtersOpen = $state(false);
  let filtersManuallySet = $state(false);
  const ROW_HEIGHT = 120;
  const ROW_OVERSCAN = 6;
  const GRID_CARD_HEIGHT = 272;
  let tableScrollTop = $state(0);
  let tableViewportHeight = $state(0);

  const hasSearch = $derived.by(() => searchTerm.trim().length > 0);
  const hasAnyFilters = $derived.by(() => hasActiveFilters || hasSearch);

  const dispatch = createEventDispatcher<{
    refresh: void;
    reset: void;
    search: { term: string };
    selectTag: { tag: string | null };
    selectCategory: { category: string | null };
    selectProvider: { provider: string | null };
    selectGroup: { group: string };
    changeSort: { sort: 'name-asc' | 'name-desc' | 'id-asc' };
    changeView: { view: PipelineRegistryView };
    addEntry: { id: string };
  }>();

  function categoryPaths(entry: PipelineRegistryEntry): string[][] {
    return (entry.metadata.categories ?? []).filter((path): path is string[] => path.length > 0);
  }

  function isExcludedEntry(entry: PipelineRegistryEntry): boolean {
    const provider = (entry.metadata.provider ?? '').trim().toLowerCase();
    if (provider && /^(io|host)(?:$|[-_:._ ])/.test(provider)) return true;
    const id = entry.id.trim().toLowerCase();
    if (/^(io|host)[-_:._]/.test(id)) return true;
    return false;
  }

  const displayEntries = $derived.by(() => {
    const entries = Array.isArray(visibleEntries) ? visibleEntries : [];
    return entries.filter((entry) => !isExcludedEntry(entry));
  });

  const displayProviders = $derived.by(() => {
    const providers = Array.isArray(availableProviders) ? availableProviders : [];
    return providers.filter((provider) => !/^(io|host)(?:$|[-_:._ ])/.test(provider.trim().toLowerCase()));
  });

  const tableWindow = $derived(
    getVirtualWindow({
      itemCount: displayEntries.length,
      rowHeight: ROW_HEIGHT,
      overscan: ROW_OVERSCAN,
      scrollTop: tableScrollTop,
      viewportHeight: tableViewportHeight
    })
  );
  const tableTotalHeight = $derived(tableWindow.totalHeight);
  const tableOffset = $derived(tableWindow.offset);
  const tableSlice = $derived(displayEntries.slice(tableWindow.startIndex, tableWindow.endIndex));

  $effect(() => {
    if (!filtersManuallySet) {
      filtersOpen = hasActiveFilters;
    } else if (filtersOpen === hasActiveFilters) {
      filtersManuallySet = false;
    }
  });

  $effect(() => {
    if (view !== 'grid') {
      view = 'grid';
      dispatch('changeView', { view: 'grid' });
    }
  });

  $effect(() => {
    if (selectedProvider && !displayProviders.includes(selectedProvider)) {
      selectedProvider = null;
      dispatch('selectProvider', { provider: null });
    }
  });

  function toggleFilters() {
    filtersOpen = !filtersOpen;
    filtersManuallySet = true;
  }

</script>

<div class="flex h-full min-h-0 flex-col gap-4 text-surface-100">
  <section class="rounded-xl border border-surface-800/70 bg-surface-900/70 p-4 shadow-inner shadow-black/30">
    <div class="flex flex-wrap items-start gap-3">
      <div class="flex min-w-[200px] flex-1 flex-col gap-1.5">
        <input
          id="registry-search-input"
          class="input h-9 text-xs"
          placeholder="Name, id, tags, ports..."
          aria-label="Search nodes"
          value={searchTerm}
          oninput={(event) => {
            searchTerm = event.currentTarget.value;
            dispatch('search', { term: searchTerm });
          }}
        />
      </div>
      <div class="flex items-center gap-1.5 flex-nowrap">
        <select
          class="input h-9 min-w-[144px] text-micro-tight uppercase tracking-[0.2em]"
          bind:value={sort}
          onchange={(event) =>
            dispatch('changeSort', { sort: event.currentTarget.value as 'name-asc' | 'name-desc' | 'id-asc' })
          }
        >
          <option value="name-asc">Name A → Z</option>
          <option value="name-desc">Name Z → A</option>
          <option value="id-asc">Identifier</option>
        </select>
        <button
          class={`btn btn-3xs uppercase tracking-[0.2em] ${
            filtersOpen
              ? 'border-primary-400/60 bg-primary-500/15 text-primary-100'
              : 'preset-tonal text-surface-100'
          }`}
          type="button"
          onclick={toggleFilters}
          aria-expanded={filtersOpen}
        >
          Filter
        </button>
        {#if hasAnyFilters}
          <button class="btn btn-3xs preset-outline uppercase tracking-[0.2em]" type="button" onclick={() => dispatch('reset')}>
            Clear
          </button>
        {/if}
      </div>
    </div>
    <div class="mt-2.5 flex flex-wrap items-center gap-1.5 text-micro-tight uppercase tracking-[0.2em] text-surface-500">
      {#if !hasAnyFilters}
        <span class="rounded border border-surface-700/80 px-2.5 py-[3px] text-surface-400">No filters applied</span>
      {/if}
      {#if hasSearch}
        <button
          class="rounded border border-primary-500/60 bg-primary-500/15 px-2.5 py-[3px] text-primary-100 transition hover:border-primary-400/80"
          type="button"
          onclick={() => {
            searchTerm = '';
            dispatch('search', { term: '' });
          }}
        >
          Search: {searchTerm.trim()} ×
        </button>
      {/if}
      {#if selectedProvider}
        <button
          class="rounded border border-primary-500/60 bg-primary-500/15 px-2.5 py-[3px] text-primary-100 transition hover:border-primary-400/80"
          type="button"
          onclick={() => {
            selectedProvider = null;
            dispatch('selectProvider', { provider: null });
          }}
        >
          Plugin: {selectedProvider} ×
        </button>
      {/if}
      {#if selectedTag}
        <button
          class="rounded border border-primary-500/60 bg-primary-500/15 px-2.5 py-[3px] text-primary-100 transition hover:border-primary-400/80"
          type="button"
          onclick={() => {
            selectedTag = null;
            dispatch('selectTag', { tag: null });
          }}
        >
          Tag: {selectedTag} ×
        </button>
      {/if}
      {#if selectedCategory}
        <button
          class="rounded border border-primary-500/60 bg-primary-500/15 px-2.5 py-[3px] text-primary-100 transition hover:border-primary-400/80"
          type="button"
          onclick={() => {
            selectedCategory = null;
            dispatch('selectCategory', { category: null });
          }}
        >
          Category: {selectedCategory} ×
        </button>
      {/if}
      {#if activeGroup !== 'all'}
        <button
          class="rounded border border-primary-500/60 bg-primary-500/15 px-2.5 py-[3px] text-primary-100 transition hover:border-primary-400/80"
          type="button"
          onclick={() => {
            activeGroup = 'all';
            dispatch('selectGroup', { group: 'all' });
          }}
        >
          Group: {registryGroups.find((group) => group.key === activeGroup)?.label ?? activeGroup} ×
        </button>
      {/if}
    </div>
    <div class="mt-3.5 rounded-lg border border-surface-800/70 bg-surface-950/60 p-3">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <p class="text-micro uppercase tracking-[0.2em] text-surface-400">Plugins</p>
        <p class="text-micro-tight uppercase tracking-[0.2em] text-surface-500">{displayProviders.length} detected</p>
      </div>
      {#if displayProviders.length === 0}
        <p class="mt-3 text-xs text-surface-500">No plugin metadata available.</p>
      {:else}
        <div class="mt-2.5 flex flex-wrap gap-1.5">
          <button
            class={`rounded-lg border px-2.5 py-[3px] text-micro-tight uppercase tracking-[0.2em] transition ${
              selectedProvider === null
                ? 'border-primary-500 bg-primary-500/20 text-primary-100'
                : 'border-surface-700/80 text-surface-300 hover:border-primary-400/50 hover:text-primary-100'
            }`}
            type="button"
            onclick={() => {
              selectedProvider = null;
              dispatch('selectProvider', { provider: null });
            }}
          >
            All plugins
          </button>
          {#each displayProviders as provider (provider)}
            <button
              class={`rounded-lg border px-2.5 py-[3px] text-micro-tight uppercase tracking-[0.2em] transition ${
                selectedProvider === provider
                  ? 'border-primary-500 bg-primary-500/20 text-primary-100'
                  : 'border-surface-700/80 text-surface-300 hover:border-primary-400/50 hover:text-primary-100'
              }`}
              type="button"
              onclick={() => {
                selectedProvider = selectedProvider === provider ? null : provider;
                dispatch('selectProvider', { provider: selectedProvider });
              }}
            >
              {provider}
            </button>
          {/each}
        </div>
      {/if}
    </div>
    {#if filtersOpen}
      <div class="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        <section class="rounded-lg border border-surface-800/70 bg-surface-950/60 p-3">
          <p class="text-micro uppercase tracking-[0.2em] text-surface-400">Tags</p>
          {#if availableTags.length === 0}
            <p class="mt-3 text-xs text-surface-500">No tags available.</p>
          {:else}
            <div class="mt-2.5 flex flex-wrap gap-1.5">
              <button
                class={`rounded-full border px-2.5 py-[3px] text-micro-tight uppercase tracking-[0.2em] transition ${
                  selectedTag === null
                    ? 'border-primary-500 bg-primary-500/20 text-primary-100'
                    : 'border-surface-700/80 text-surface-300 hover:border-primary-400/50 hover:text-primary-100'
                }`}
                type="button"
                onclick={() => {
                  selectedTag = null;
                  dispatch('selectTag', { tag: null });
                }}
              >
                Any tag
              </button>
              {#each availableTags as tag (tag)}
                <button
                  class={`rounded-full border px-2.5 py-[3px] text-micro-tight uppercase tracking-[0.2em] transition ${
                    selectedTag === tag
                      ? 'border-primary-500 bg-primary-500/20 text-primary-100'
                      : 'border-surface-700/80 text-surface-300 hover:border-primary-400/50 hover:text-primary-100'
                  }`}
                  type="button"
                  onclick={() => {
                    selectedTag = tag;
                    dispatch('selectTag', { tag });
                  }}
                >
                  {tag}
                </button>
              {/each}
            </div>
          {/if}
        </section>
        <section class="rounded-lg border border-surface-800/70 bg-surface-950/60 p-3">
          <p class="text-micro uppercase tracking-[0.2em] text-surface-400">Categories</p>
          {#if availableCategories.length === 0}
            <p class="mt-3 text-xs text-surface-500">No categories available.</p>
          {:else}
            <div class="mt-2.5 flex flex-wrap gap-1.5">
              <button
                class={`rounded-full border px-2.5 py-[3px] text-micro-tight uppercase tracking-[0.2em] transition ${
                  selectedCategory === null
                    ? 'border-primary-500 bg-primary-500/20 text-primary-100'
                    : 'border-surface-700/80 text-surface-300 hover:border-primary-400/50 hover:text-primary-100'
                }`}
                type="button"
                onclick={() => {
                  selectedCategory = null;
                  dispatch('selectCategory', { category: null });
                }}
              >
                Any category
              </button>
              {#each availableCategories as category (category)}
                <button
                  class={`rounded-full border px-2.5 py-[3px] text-micro-tight uppercase tracking-[0.2em] transition ${
                    selectedCategory === category
                      ? 'border-primary-500 bg-primary-500/20 text-primary-100'
                      : 'border-surface-700/80 text-surface-300 hover:border-primary-400/50 hover:text-primary-100'
                  }`}
                  type="button"
                  onclick={() => {
                    selectedCategory = selectedCategory === category ? null : category;
                    dispatch('selectCategory', { category: selectedCategory });
                  }}
                >
                  {category}
                </button>
              {/each}
            </div>
          {/if}
        </section>
        <section class="rounded-lg border border-surface-800/70 bg-surface-950/60 p-3">
          <p class="text-micro uppercase tracking-[0.2em] text-surface-400">Groups</p>
          {#if registryGroups.length === 0}
            <p class="mt-3 text-xs text-surface-500">No group information available.</p>
          {:else}
            <div class="mt-2.5 space-y-1.5">
              <button
                class={`w-full rounded-lg border px-2.5 py-1.5 text-left text-micro-tight uppercase tracking-[0.2em] transition ${
                  activeGroup === 'all'
                    ? 'border-primary-500 bg-primary-500/20 text-primary-100'
                    : 'border-surface-700/70 bg-surface-950/40 text-surface-300 hover:border-primary-400/40 hover:text-primary-100'
                }`}
                type="button"
                onclick={() => {
                  activeGroup = 'all';
                  dispatch('selectGroup', { group: 'all' });
                }}
              >
                All groups
                <span class="ml-2 text-micro-tight uppercase tracking-[0.2em] text-surface-500">{totalEntries}</span>
              </button>
              {#each registryGroups as group (group.key)}
                <button
                  class={`w-full rounded-lg border px-2.5 py-1.5 text-left text-micro-tight uppercase tracking-[0.2em] transition ${
                    activeGroup === group.key
                      ? 'border-primary-500 bg-primary-500/20 text-primary-100'
                      : 'border-surface-700/70 bg-surface-950/40 text-surface-300 hover:border-primary-400/40 hover:text-primary-100'
                  }`}
                  type="button"
                  onclick={() => {
                    activeGroup = group.key;
                    dispatch('selectGroup', { group: group.key });
                  }}
                >
                  {group.label}
                  <span class="ml-2 text-micro-tight uppercase tracking-[0.2em] text-surface-500">{group.entries.length}</span>
                </button>
              {/each}
            </div>
          {/if}
        </section>
      </div>
    {/if}
  </section>

  <section class="flex min-h-0 flex-1 flex-col overflow-hidden rounded-2xl border border-surface-800/70 bg-surface-900/60">
    <div class="flex flex-wrap items-center gap-2 border-b border-surface-800/70 px-5 py-3">
      <p class="text-micro-tight uppercase tracking-[0.22em] text-surface-400">
        {loading
          ? 'Loading nodes…'
          : `Showing ${displayEntries.length} ${displayEntries.length === 1 ? 'node' : 'nodes'}${
              hasAnyFilters && totalEntries ? ` of ${totalEntries}` : ''
            }`}
      </p>
    </div>
    <div class="flex-1 overflow-hidden">
      {#if loading}
        <p class="px-5 py-4 text-xs text-surface-400">Loading registry…</p>
      {:else if error}
        <p class="px-5 py-4 text-xs text-error-300">{error}</p>
      {:else if registryEntries.length === 0}
        <p class="px-5 py-4 text-xs text-surface-400">No registry entries match the current filters.</p>
      {:else if displayEntries.length === 0}
        <p class="px-5 py-4 text-xs text-surface-400">No nodes remain after applying filters.</p>
      {:else if view === 'table'}
        <div
          class="h-full overflow-auto"
          use:virtualViewport={{
            onScroll: (top) => (tableScrollTop = top),
            onResize: ({ height }) => (tableViewportHeight = height)
          }}
        >
          <div class="px-3 py-3">
            <div class="relative" style={`height: ${tableTotalHeight}px;`}>
              <div class="absolute left-0 right-0" style={`transform: translateY(${tableOffset}px);`}>
                {#each tableSlice as entry (entry.id)}
                  {@const categories = categoryPaths(entry)}
                  {@const primaryGroup = categories.length > 0 ? categories[0].join(' / ') : 'Uncategorized'}
                  <button
                    class="group flex h-full items-center gap-3 rounded-xl border border-surface-800/70 bg-surface-950/40 px-3 py-2.5 text-xs text-surface-200 shadow-inner shadow-black/20 transition hover:border-primary-500/60 hover:bg-surface-950/60 overflow-hidden cursor-pointer"
                    style={`height: ${ROW_HEIGHT}px;`}
                    type="button"
                    onclick={() => dispatch('addEntry', { id: entry.id })}
                  >
                    <div class="relative">
                      <PipelineRegistryEntryPreview entry={entry} variant="compact" />
                      <div class="pointer-events-none absolute inset-0 rounded-xl bg-gradient-to-t from-black/70 via-black/10 to-transparent"></div>
                      <div class="pointer-events-none absolute inset-0 flex flex-col justify-between p-2">
                        <div class="flex items-start justify-end gap-2 text-micro-tight uppercase tracking-[0.2em] text-surface-100/90">
                          {#if entry.metadata.provider}
                            <span class="rounded-full border border-white/30 bg-black/50 px-2 py-0.5">
                              {entry.metadata.provider}
                            </span>
                          {/if}
                        </div>
                        <div class="space-y-1">
                          <p class="text-xs font-semibold leading-tight text-surface-50 line-clamp-2">
                            {entry.metadata.name}
                          </p>
                          <p class="text-micro-tight uppercase tracking-[0.2em] text-surface-200/80 line-clamp-1">
                            {primaryGroup}
                          </p>
                        </div>
                      </div>
                    </div>
                  </button>
                {/each}
              </div>
            </div>
          </div>
        </div>
      {:else}
        <div class="h-full overflow-auto px-5 py-4">
          <div class="grid gap-4 sm:grid-cols-2 md:grid-cols-3">
            {#each displayEntries as entry (entry.id)}
              {@const categories = categoryPaths(entry)}
              {@const primaryGroup = categories.length > 0 ? categories[0].join(' / ') : 'Uncategorized'}
              <button
                class="group flex h-full items-center justify-center rounded-xl border border-surface-800/70 bg-surface-950/40 p-3 shadow-inner shadow-black/30 transition hover:border-primary-500/60 hover:bg-surface-950/60 cursor-pointer"
                style={`height: ${GRID_CARD_HEIGHT}px;`}
                type="button"
                onclick={() => dispatch('addEntry', { id: entry.id })}
              >
                <div class="relative">
                  <PipelineRegistryEntryPreview entry={entry} variant="tile" />
                  <div class="pointer-events-none absolute inset-0 rounded-2xl bg-gradient-to-t from-black/70 via-black/10 to-transparent"></div>
                  <div class="pointer-events-none absolute inset-0 flex flex-col justify-between p-2.5">
                    <div class="flex items-start justify-end gap-2 text-micro-tight uppercase tracking-[0.2em] text-surface-100/90">
                      {#if entry.metadata.provider}
                        <span class="rounded-full border border-white/30 bg-black/50 px-2 py-0.5">
                          {entry.metadata.provider}
                        </span>
                      {/if}
                    </div>
                    <div class="space-y-1">
                      <p class="text-sm font-semibold leading-tight text-surface-50 line-clamp-2">
                        {entry.metadata.name}
                      </p>
                      <p class="text-micro-tight uppercase tracking-[0.2em] text-surface-200/80 line-clamp-1">
                        {primaryGroup}
                      </p>
                    </div>
                  </div>
                </div>
              </button>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  </section>
</div>
