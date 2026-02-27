<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { MediaAsset } from '$lib/features/media/api';
  import { mediaKindLabel } from '$lib/features/media/mediaKind';

  type SelectedIds = Set<string> | Record<string, boolean> | string[];

  type MediaAssetListProps = {
    items: MediaAsset[];
    className?: string;
    getKey?: (asset: MediaAsset, index: number) => string | number;
    selectedIds?: SelectedIds;
    row?: Snippet<[MediaAsset, number, boolean]>;
    empty?: Snippet;
    footer?: Snippet;
  };

  const { items, className = '', getKey, selectedIds, row, empty, footer }: MediaAssetListProps = $props();

  const hasItems = $derived(items.length > 0);

  function isSelected(id: string): boolean {
    if (!selectedIds) return false;
    if (Array.isArray(selectedIds)) return selectedIds.includes(id);
    if (selectedIds instanceof Set) return selectedIds.has(id);
    return Boolean(selectedIds[id]);
  }

  export type $$Props = MediaAssetListProps;
</script>

<div class={`flex min-w-0 flex-col gap-2 ${className}`.trim()}>
  {#if hasItems}
    {#each items as asset, index (getKey ? getKey(asset, index) : asset.id ?? index)}
      {#if row}
        {@render row(asset, index, isSelected(asset.id))}
      {:else}
        <div class={`rounded border border-surface-800 bg-surface-950 px-3 py-2 text-sm text-surface-200 ${
          isSelected(asset.id) ? 'ring-1 ring-sky-500' : ''
        }`.trim()}>
          <div class="flex items-center justify-between gap-3">
            <div>
              <p class="font-medium text-surface-100">{asset.name}</p>
              <p class="text-xs text-surface-500">{mediaKindLabel(asset.kind)}</p>
            </div>
            <span class="text-xs uppercase text-surface-500">{asset.sizeBytes} bytes</span>
          </div>
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
