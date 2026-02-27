<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { MediaAsset } from '$lib/features/media/api';
  import { mediaKindLabel } from '$lib/features/media/mediaKind';

  type SelectedIds = Set<string> | Record<string, boolean> | string[];

  type MediaAssetGridProps = {
    items: MediaAsset[];
    minItemWidth?: number;
    columns?: number;
    rowHeight?: number;
    gap?: number;
    className?: string;
    getKey?: (asset: MediaAsset, index: number) => string | number;
    selectedIds?: SelectedIds;
    style?: string;
    card?: Snippet<[MediaAsset, number, boolean]>;
    empty?: Snippet;
    footer?: Snippet;
  };

  const {
    items,
    minItemWidth = 220,
    columns,
    rowHeight,
    gap = 12,
    className = '',
    getKey,
    selectedIds,
    style = '',
    card,
    empty,
    footer
  }: MediaAssetGridProps = $props();

  const hasItems = $derived(items.length > 0);
  const gridStyle = $derived.by(() => {
    const safeGap = Math.max(4, gap);
    if (typeof columns === 'number' && columns > 0) {
      const rows = typeof rowHeight === 'number' && rowHeight > 0 ? `grid-auto-rows:${rowHeight}px;` : '';
      return `grid-template-columns:repeat(${columns}, minmax(0, 1fr)); gap:${safeGap}px; ${rows} ${style}`.trim();
    }
    return `--media-grid-min:${Math.max(120, minItemWidth)}px; --media-grid-gap:${safeGap}px; ${style}`.trim();
  });

  function isSelected(id: string): boolean {
    if (!selectedIds) return false;
    if (Array.isArray(selectedIds)) return selectedIds.includes(id);
    if (selectedIds instanceof Set) return selectedIds.has(id);
    return Boolean(selectedIds[id]);
  }

  export type $$Props = MediaAssetGridProps;
</script>

<div class={`media-asset-grid ${className}`.trim()} style={gridStyle}>
  {#if hasItems}
    {#each items as asset, index (getKey ? getKey(asset, index) : asset.id ?? index)}
      {#if card}
        {@render card(asset, index, isSelected(asset.id))}
      {:else}
        <div
          class={`rounded border border-surface-800 bg-surface-950 p-3 text-xs text-surface-300 ${
            isSelected(asset.id) ? 'ring-1 ring-sky-500' : ''
          }`.trim()}
        >
          <p class="font-medium text-surface-100">{asset.name}</p>
          <p class="text-micro uppercase text-surface-500">{mediaKindLabel(asset.kind)}</p>
        </div>
      {/if}
    {/each}
  {:else}
    <div class="rounded border border-dashed border-surface-800 p-6 text-center text-sm text-surface-500">
      {#if empty}
        {@render empty()}
      {:else}
        No media assets.
      {/if}
    </div>
  {/if}
</div>

{#if footer}
  <div class="pt-3">
    {@render footer()}
  </div>
{/if}

<style>
  .media-asset-grid {
    display: grid;
    width: 100%;
    min-width: 0;
    grid-template-columns: repeat(auto-fill, minmax(var(--media-grid-min), 1fr));
    gap: var(--media-grid-gap);
  }
</style>
