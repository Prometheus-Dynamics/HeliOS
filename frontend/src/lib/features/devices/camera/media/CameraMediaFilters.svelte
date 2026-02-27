<script lang="ts">
  import type { MediaAssetType, MediaListSort } from '$lib/features/media/api';
  import { MEDIA_KIND_OPTIONS, mediaKindLabel } from '$lib/features/media/mediaKind';

  type Props = {
    query: string;
    kind: 'all' | MediaAssetType;
    sort: MediaListSort;
    onQueryChange: (value: string) => void;
  };

  let {
    query,
    kind = $bindable(),
    sort = $bindable(),
    onQueryChange
  }: Props = $props();
</script>

<div class="flex flex-wrap items-center gap-2">
  <label class="flex-1 min-w-[180px]">
    <span class="sr-only">Search media</span>
    <input
      class="w-full rounded border border-surface-700/70 bg-surface-900/60 px-3 py-2 text-xs text-surface-100 placeholder:text-surface-500 focus:border-primary-400 focus:outline-none"
      type="search"
      placeholder="Search by filename"
      value={query}
      oninput={(event) => onQueryChange((event.currentTarget as HTMLInputElement).value)}
    />
  </label>
  <label class="flex items-center gap-2 text-micro-tight uppercase tracking-[0.3em] text-surface-500">
    Kind
    <select
      class="rounded border border-surface-700/70 bg-surface-900/60 px-2 py-1 text-xs text-surface-100"
      bind:value={kind}
    >
      <option value="all">All</option>
      {#each MEDIA_KIND_OPTIONS as option (option)}
        <option value={option}>{mediaKindLabel(option)}</option>
      {/each}
    </select>
  </label>
  <label class="flex items-center gap-2 text-micro-tight uppercase tracking-[0.3em] text-surface-500">
    Sort
    <select
      class="rounded border border-surface-700/70 bg-surface-900/60 px-2 py-1 text-xs text-surface-100"
      bind:value={sort}
    >
      <option value="recent">Recent</option>
      <option value="name">Name</option>
    </select>
  </label>
</div>
