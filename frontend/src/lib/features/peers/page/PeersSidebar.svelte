<script lang="ts">
  import SidebarFilterList from '$lib/components/filters/SidebarFilterList.svelte';
  import SidebarSearchSection from '$lib/components/filters/SidebarSearchSection.svelte';
  import type { PeerFilterDefinition, PeerFilterOption } from '$lib/features/peers/types';

  type Props = {
    searchQuery: string;
    viewFilter: PeerFilterOption;
    filters: PeerFilterDefinition[];
    onAddPeer: () => void;
    onFilterChange: (id: PeerFilterOption) => void;
  };

  let {
    searchQuery = $bindable(),
    viewFilter = $bindable(),
    filters,
    onAddPeer,
    onFilterChange
  }: Props = $props();
</script>

<aside class="w-full shrink-0 space-y-3 overflow-visible rounded border border-surface-800/60 bg-surface-950/40 p-3 text-xs text-surface-400 lg:max-w-[16rem] xl:max-w-[16.75rem] 2xl:max-w-[17.5rem]">
  <button class="btn btn-xs preset-filled-primary-500 w-full uppercase tracking-[0.22em]" type="button" onclick={() => onAddPeer()}>
    Add peer
  </button>
  <SidebarSearchSection
    label="Search peers"
    placeholder="Alias, id, or endpoint"
    description="Alias, id, network endpoint"
    size="compact"
    bind:value={searchQuery}
  />
  <div>
    <p class="text-micro uppercase tracking-[0.22em] text-surface-500">Filter by platform</p>
    <SidebarFilterList items={filters} selectedId={viewFilter} onSelect={(id) => onFilterChange(id as PeerFilterOption)} />
  </div>
  <p class="mt-2 text-micro-tight text-surface-500">Focus on the peers contributing localization data from each camera platform.</p>
</aside>
