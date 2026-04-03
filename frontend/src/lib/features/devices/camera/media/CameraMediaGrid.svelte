<script lang="ts">
  import MediaAssetGrid from '$lib/components/media/MediaAssetGrid.svelte';
  import type { MediaAsset } from '$lib/features/media/api';
  import { assetOriginalSource, assetPreviewSource, formatBytes } from '$lib/features/media/utils';
  import { mediaKindLabel } from '$lib/features/media/mediaKind';

  type Props = {
    streamId: string | null;
    loading: boolean;
    error: string | null;
    assets: MediaAsset[];
    visibleAssets: MediaAsset[];
    total: number | null;
    onSelect: (asset: MediaAsset) => void;
  };

  const {
    streamId,
    loading,
    error,
    assets,
    visibleAssets,
    total,
    onSelect
  }: Props = $props();

  function handleImagePreviewError(event: Event, fallbackSrc: string): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLImageElement)) return;
    target.src = fallbackSrc;
  }
</script>

<div class="min-h-0 flex-1 rounded border border-surface-800/60 bg-surface-950/30 p-3">
  {#if !streamId}
    <p class="text-sm text-surface-500">Stream id not available yet.</p>
  {:else if loading}
    <div class="grid grid-cols-3 gap-3">
      {#each Array.from({ length: 6 }, (_unused, idx) => idx) as idx (idx)}
        <div class="h-32 rounded border border-surface-800/70 bg-surface-900/50 p-3 animate-pulse"></div>
      {/each}
    </div>
  {:else if error}
    <p class="text-sm text-error-300">{error}</p>
  {:else if assets.length === 0}
    <div class="flex flex-col items-center justify-center gap-2 py-12 text-center text-surface-500">
      <p>No media yet for this stream.</p>
      <p class="text-xs uppercase tracking-[0.3em] text-surface-600">Capture or upload media to populate this view.</p>
    </div>
  {:else}
    <MediaAssetGrid items={visibleAssets} columns={3} gap={12}>
      {#snippet card(asset)}
        {@const previewSrc = assetPreviewSource(asset)}
        {@const originalSrc = assetOriginalSource(asset)}
        <button
          class="group flex h-64 flex-col overflow-hidden rounded border border-surface-800/70 bg-surface-900/40 text-left transition hover:border-primary-400/70 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary-400/60"
          type="button"
          onclick={() => onSelect(asset)}
        >
          <div class="relative h-44 w-full overflow-hidden">
            {#if asset.kind === 'image'}
              <img
                class="h-full w-full object-cover transition duration-300 group-hover:scale-105"
                src={previewSrc}
                alt={asset.name}
                loading="lazy"
                onerror={(event) => handleImagePreviewError(event, originalSrc)}
              />
            {:else if asset.kind === 'video'}
              <img
                class="h-full w-full object-cover transition duration-300 group-hover:scale-105"
                src={previewSrc}
                alt={asset.name}
                loading="lazy"
              />
            {:else}
              <div class="flex h-full w-full items-center justify-center bg-gradient-to-br from-surface-900/70 to-surface-950 text-micro-tight uppercase tracking-[0.3em] text-surface-300">
                {mediaKindLabel(asset.kind)}
              </div>
            {/if}
          </div>
          <div class="flex flex-1 flex-col justify-between gap-1 px-2 py-1">
            <p class="truncate text-xs text-surface-100" title={asset.name}>{asset.name}</p>
            <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">
              {mediaKindLabel(asset.kind)} - {formatBytes(asset.sizeBytes)}
            </p>
          </div>
        </button>
      {/snippet}
    </MediaAssetGrid>
    <div class="mt-3 flex items-center text-micro-tight uppercase tracking-[0.3em] text-surface-500">
      <span>{visibleAssets.length} of {total || assets.length} shown</span>
    </div>
  {/if}
</div>
