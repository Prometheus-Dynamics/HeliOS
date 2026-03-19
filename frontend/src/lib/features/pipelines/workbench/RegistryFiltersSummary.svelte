<script lang="ts">
  import type { PipelineController } from '$lib/features/pipelines/controller';

  type RegistryStores = PipelineController['stores']['registry'];
  type RegistryHelpers = PipelineController['helpers']['registry'];

  const {
    stores,
    helpers
  }: {
    stores: RegistryStores;
    helpers: RegistryHelpers;
  } = $props();

  const search = $derived.by(() => stores.search);
  const tag = $derived.by(() => stores.tag);
  const category = $derived.by(() => stores.category);
  const hasActiveFilters = $derived.by(() => stores.hasActiveFilters);
  const loading = $derived.by(() => stores.loading);
  const sort = $derived.by(() => stores.sort);
  const view = $derived.by(() => stores.view);
  const availableTags = $derived.by(() => stores.availableTags);
  const availableCategories = $derived.by(() => stores.availableCategories);

  const searchTerm = $derived(() => $search.trim());

  function handleSearchChange(event: Event) {
    const value = (event.currentTarget as HTMLInputElement).value;
    search.set(value);
  }

  function handleTagChange(event: Event) {
    const value = (event.currentTarget as HTMLSelectElement).value || null;
    tag.set(value);
  }

  function handleCategoryChange(event: Event) {
    const value = (event.currentTarget as HTMLSelectElement).value || null;
    category.set(value);
  }

  function handleSortChange(event: Event) {
    const value = event.currentTarget instanceof HTMLSelectElement ? (event.currentTarget.value as 'name-asc' | 'name-desc' | 'id-asc') : 'name-asc';
    sort.set(value);
  }

  function toggleView() {
    view.set($view === 'table' ? 'grid' : 'table');
  }
</script>

<section class="space-y-3 rounded border border-surface-800/70 bg-surface-950/70 p-3 text-xs text-surface-400">
  <div class="flex flex-wrap items-center justify-between gap-3">
    <div class="flex-1">
      <label class="text-micro-tight uppercase tracking-[0.3em] text-surface-500" for="registry-search-input">
        Search registry
      </label>
      <input
        id="registry-search-input"
        class="input mt-1 w-full text-sm"
        placeholder="Node name, summary, tag…"
        value={$search}
        oninput={handleSearchChange}
      />
    </div>
    <div class="flex flex-col gap-1">
      <span class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">View</span>
      <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={toggleView}>
        {$view === 'table' ? 'Table' : 'Grid'}
      </button>
    </div>
  </div>

  <div class="grid gap-3 md:grid-cols-2">
    <label class="flex flex-col gap-1">
      <span class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">Tag</span>
      <select class="input text-sm" onchange={handleTagChange} value={$tag ?? ''}>
        <option value="">Any tag</option>
        {#each $availableTags as option (option)}
          <option value={option} selected={option === $tag}>{option}</option>
        {/each}
      </select>
    </label>
    <label class="flex flex-col gap-1">
      <span class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">Category</span>
      <select class="input text-sm" onchange={handleCategoryChange} value={$category ?? ''}>
        <option value="">Any category</option>
        {#each $availableCategories as option (option)}
          <option value={option} selected={option === $category}>{option}</option>
        {/each}
      </select>
    </label>
  </div>

  <div class="flex flex-wrap items-center justify-between gap-2">
    <label class="flex items-center gap-2 text-micro-tight uppercase tracking-[0.3em] text-surface-500">
      Sort
      <select class="input text-xs" onchange={handleSortChange} value={$sort}>
        <option value="name-asc">Name · A→Z</option>
        <option value="name-desc">Name · Z→A</option>
        <option value="id-asc">Identifier</option>
      </select>
    </label>
    <div class="flex items-center gap-2">
      <button
        class="btn btn-3xs preset-outline uppercase tracking-[0.3em]"
        type="button"
        onclick={() => helpers.resetFilters()}
        disabled={!$hasActiveFilters && !searchTerm}
      >
        Reset
      </button>
      <button
        class="btn btn-3xs preset-outline uppercase tracking-[0.3em]"
        type="button"
        onclick={() => helpers.refresh()}
        disabled={$loading}
      >
        {$loading ? 'Refreshing…' : 'Refresh'}
      </button>
    </div>
  </div>
</section>
