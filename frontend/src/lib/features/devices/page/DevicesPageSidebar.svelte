<script lang="ts">
  import type { IconDefinition } from '@fortawesome/free-solid-svg-icons';
  import SidebarSearchSection from '$lib/components/filters/SidebarSearchSection.svelte';
  import SidebarFilterList from '$lib/components/filters/SidebarFilterList.svelte';

  type FilterOption = { id: string; label: string; description: string; icon: IconDefinition };

  type Props = {
    searchQuery: string;
    cameraFilter: string;
    filters: FilterOption[];
    hasActiveFilters: boolean;
    onRegister: () => void;
    onClearFilters: () => void;
  };

  let {
    searchQuery = $bindable(),
    cameraFilter = $bindable(),
    filters,
    hasActiveFilters,
    onRegister,
    onClearFilters
  }: Props = $props();
</script>

<aside class="w-full shrink-0 space-y-3 overflow-visible rounded border border-surface-800/60 bg-surface-950/40 p-3 text-xs text-surface-400 lg:max-w-[16rem] xl:max-w-[16.75rem] 2xl:max-w-[17.5rem]">
  <button
    class="btn btn-xs preset-filled-primary-500 uppercase tracking-[0.22em] w-full"
    type="button"
    onclick={() => onRegister()}
  >
    Register stream
  </button>
  <SidebarSearchSection
    label="Search inventory"
    placeholder="Camera name, pipeline, or sensor"
    size="compact"
    bind:value={searchQuery}
  />
  <div>
    <p class="text-micro uppercase tracking-[0.22em] text-surface-500">Stream state</p>
    <SidebarFilterList
      items={filters}
      selectedId={cameraFilter}
      onSelect={(id) => (cameraFilter = id as string)}
    />
    {#if hasActiveFilters}
      <button
        class="btn btn-2xs preset-tonal mt-2.5 w-full uppercase tracking-[0.22em]"
        type="button"
        onclick={() => onClearFilters()}
      >
        Clear filters
      </button>
    {/if}
  </div>
  <p class="text-micro-tight text-surface-500">
    Stay consistent with the rest of the workspace: search, filter, and action buttons live in this drawer.
  </p>
</aside>
