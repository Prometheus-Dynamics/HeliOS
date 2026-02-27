<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { IconDefinition } from '@fortawesome/free-solid-svg-icons';
  import SidebarFilterList from '$lib/components/filters/SidebarFilterList.svelte';
  import SidebarSearchSection from '$lib/components/filters/SidebarSearchSection.svelte';

  type FilterItem<Id extends string = string> = {
    id: Id;
    label: string;
    description?: string | null;
    icon?: IconDefinition | null;
    logo?: string | null;
  };

  type MediaFiltersProps<FilterId extends string = string> = {
    className?: string;
    searchLabel?: string;
    searchPlaceholder?: string;
    searchDescription?: string | null;
    searchValue?: string;
    filters: FilterItem<FilterId>[];
    selectedFilter: FilterId;
    onSelectFilter?: (id: FilterId) => void;
    streams?: string[];
    selectedStream?: string;
    onSelectStream?: (id: string) => void;
    streamLabel?: (id: string) => string;
    actions?: Snippet;
    footer?: Snippet;
  };

  let {
    className = '',
    searchLabel = 'Search media',
    searchPlaceholder = 'Search files or tags',
    searchDescription = 'Name, description, tags',
    searchValue = $bindable(''),
    filters,
    selectedFilter,
    onSelectFilter,
    streams = [],
    selectedStream = 'all',
    onSelectStream,
    streamLabel,
    actions,
    footer
  }: MediaFiltersProps = $props();

  const streamOptions = $derived.by(() => {
    if (!streams?.length) return [];
    return [
      { id: 'all', label: 'All streams', description: 'Any capture source' },
      ...streams.map((source) => ({
        id: source,
        label: streamLabel ? streamLabel(source) : source,
        description: source
      }))
    ];
  });

  export type $$Props = MediaFiltersProps;
</script>

<aside class={`w-full min-h-0 shrink-0 space-y-3 overflow-auto rounded border border-surface-800/60 bg-surface-950/40 p-3 text-xs text-surface-400 ${className}`.trim()}>
  {#if actions}
    {@render actions()}
  {/if}

  <SidebarSearchSection
    label={searchLabel}
    placeholder={searchPlaceholder}
    description={searchDescription}
    size="compact"
    bind:value={searchValue}
  />

  <div>
    <p class="text-micro uppercase tracking-[0.22em] text-surface-500">Filter by type</p>
    <SidebarFilterList items={filters} selectedId={selectedFilter} onSelect={onSelectFilter} />
  </div>

  {#if streamOptions.length}
    <div>
      <p class="text-micro uppercase tracking-[0.22em] text-surface-500">Filter by stream</p>
      <SidebarFilterList items={streamOptions} selectedId={selectedStream} onSelect={onSelectStream} />
    </div>
  {/if}

  {#if footer}
    <div class="pt-1">
      {@render footer()}
    </div>
  {/if}
</aside>
