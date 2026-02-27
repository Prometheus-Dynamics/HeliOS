<script lang="ts">
  import { Panel } from '$lib';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { virtualViewport } from '$lib/ui/virtualViewport';
  import MediaAssetGrid from '$lib/components/media/MediaAssetGrid.svelte';
  import MediaAssetList from '$lib/components/media/MediaAssetList.svelte';
  import { faDownload, faTrashCan } from '@fortawesome/free-solid-svg-icons';
  import { assetOriginalSource, assetPreviewSource, deriveModelAffinity, formatBytes } from '$lib/features/media/utils';
  import { mediaKindLabel } from '$lib/features/media/mediaKind';
  import type { MediaAsset } from '$lib/features/media/api';

  type SelectedIds = Set<string> | Record<string, boolean> | string[];
  type LayoutMode = 'grid' | 'list';

  type MediaDisplay = Record<
    string,
    { sizeLabel: string; updatedLabel: string; dimensionLabel?: string; fpsLabel?: string }
  >;

  type SortOption = { id: string; label: string };

  type MediaLibraryPanelProps = {
    loading: boolean;
    loadingMore: boolean;
    hasMore: boolean;
    filteredCount: number;
    selectedCount: number;
    sortMode: string;
    sortOptions: SortOption[];
    onSortChange?: (mode: string) => void;
    layoutMode: LayoutMode;
    onLayoutChange?: (mode: LayoutMode) => void;
    onClearSelection?: () => void;
    onCreateMediaStream?: () => void;
    onDownloadSelected?: () => void;
    onDeleteSelected?: () => void;
    onLoadMore?: () => void;
    onScroll?: (top: number) => void;
    onResize?: (size: { height: number; width: number }) => void;
    mediaGridSlice: MediaAsset[];
    mediaGridColumns: number;
    mediaGridTileSize: number;
    mediaGridTotalHeight: number;
    mediaGridOffset: number;
    mediaGridGap: number;
    mediaListSlice: MediaAsset[];
    mediaListOffset: number;
    mediaListTotalHeight: number;
    mediaListRowHeight: number;
    mediaListWindowStart: number;
    selectedIds: SelectedIds;
    showSelectionControls: boolean;
    toggleSelected: (id: string) => void;
    onAssetClick: (asset: MediaAsset, event: MouseEvent) => void;
    onAssetSelect: (asset: MediaAsset) => void;
    onAssetDragStart: (asset: MediaAsset, event: PointerEvent) => void;
    onAssetDragEnter: (asset: MediaAsset, event: PointerEvent) => void;
    mediaDisplay: MediaDisplay;
  };

  const {
    loading,
    loadingMore,
    hasMore,
    filteredCount,
    selectedCount,
    sortMode,
    sortOptions,
    onSortChange,
    layoutMode,
    onLayoutChange,
    onClearSelection,
    onCreateMediaStream,
    onDownloadSelected,
    onDeleteSelected,
    onLoadMore,
    onScroll,
    onResize,
    mediaGridSlice,
    mediaGridColumns,
    mediaGridTileSize,
    mediaGridTotalHeight,
    mediaGridOffset,
    mediaGridGap,
    mediaListSlice,
    mediaListOffset,
    mediaListTotalHeight,
    mediaListRowHeight,
    mediaListWindowStart,
    selectedIds,
    showSelectionControls,
    toggleSelected,
    onAssetClick,
    onAssetSelect,
    onAssetDragStart,
    onAssetDragEnter,
    mediaDisplay
  }: MediaLibraryPanelProps = $props();

  function handleBackgroundPointerDown(event: PointerEvent): void {
    const target = event.target as HTMLElement | null;
    if (!target) return;
    if (target.closest('[data-media-asset-card="true"]')) return;
    if (target.closest('button, a, input, select, textarea, label')) return;
    onClearSelection?.();
  }

  export type $$Props = MediaLibraryPanelProps;
</script>

<Panel title="Media library" className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
  {#snippet actions()}
    <div class="flex flex-wrap items-center gap-3">
      {#if selectedCount}
        <div class="flex items-center gap-2">
          <span class="rounded border border-primary-500/40 bg-primary-500/10 px-2 py-1 text-micro uppercase tracking-[0.3em] text-primary-100">
            {selectedCount} selected
          </span>
          <button class="btn btn-2xs h-8 preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={onCreateMediaStream}>
            Create stream
          </button>
          <button
            class="btn btn-2xs h-8 w-8 preset-tonal !px-0"
            type="button"
            onclick={onDownloadSelected}
            aria-label="Download selected"
            title="Download selected"
          >
            <FaIcon icon={faDownload} class="h-4 w-4" />
          </button>
          <button
            class="btn btn-2xs h-8 w-8 preset-filled-error-500 !px-0"
            type="button"
            onclick={onDeleteSelected}
            aria-label="Delete selected"
            title="Delete selected"
          >
            <FaIcon icon={faTrashCan} class="h-4 w-4" />
          </button>
        </div>
      {/if}
      <select
        class="input w-48"
        value={sortMode}
        onchange={(event) => onSortChange?.((event.target as HTMLSelectElement).value)}
      >
        {#each sortOptions as option (option.id)}
          <option value={option.id}>{option.label}</option>
        {/each}
      </select>
      <div class="flex gap-2">
        <button
          class={`btn btn-2xs ${layoutMode === 'grid' ? 'preset-filled-primary-500' : 'preset-tonal'}`}
          type="button"
          onclick={() => onLayoutChange?.('grid')}
        >
          Grid
        </button>
        <button
          class={`btn btn-2xs ${layoutMode === 'list' ? 'preset-filled-primary-500' : 'preset-tonal'}`}
          type="button"
          onclick={() => onLayoutChange?.('list')}
        >
          List
        </button>
      </div>
    </div>
  {/snippet}

  <div
    class="flex-1 min-h-0 min-w-0 overflow-auto overflow-x-hidden rounded border border-surface-800/60 bg-surface-950/20 p-4"
    use:virtualViewport={{
      onScroll: (top) => onScroll?.(top),
      onResize: (size) => onResize?.(size)
    }}
    onpointerdown={handleBackgroundPointerDown}
  >
    {#if loading}
      <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 2xl:grid-cols-5">
        {#each Array.from({ length: 8 }, (_unused, idx) => idx) as idx (idx)}
          <div class="aspect-square rounded border border-surface-800/70 bg-surface-900/60 p-3 animate-pulse">
            <div class="flex h-full flex-col justify-between">
              <div class="h-4 w-1/2 rounded bg-surface-800/70"></div>
              <div class="space-y-2">
                <div class="h-3 w-2/3 rounded bg-surface-800/60"></div>
                <div class="h-3 w-1/2 rounded bg-surface-800/60"></div>
              </div>
            </div>
          </div>
        {/each}
      </div>
    {:else if filteredCount === 0}
      <div class="flex h-full flex-col items-center justify-center text-center text-surface-500">
        <p>No files found.</p>
        <p class="text-xs uppercase tracking-[0.3em] text-surface-600">Upload media or adjust filters to see content.</p>
      </div>
    {:else if layoutMode === 'grid'}
      <div class="relative min-w-0" style={`height: ${mediaGridTotalHeight}px;`}>
        <div class="absolute left-0 right-0 min-w-0" style={`transform: translateY(${mediaGridOffset}px);`}>
          <MediaAssetGrid
            items={mediaGridSlice}
            columns={mediaGridColumns}
            rowHeight={mediaGridTileSize}
            gap={mediaGridGap}
            selectedIds={selectedIds}
          >
            {#snippet card(asset, index, isSelected)}
              {@const assetPreviewSrc = assetPreviewSource(asset)}
              {@const assetOriginalSrc = assetOriginalSource(asset)}
              {@const affinity = asset.kind === 'model' ? deriveModelAffinity(asset.tags) : null}
              <button
                class={`group relative min-w-0 w-full overflow-hidden rounded border bg-surface-950/50 text-left shadow transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary-400/60 ${
                  isSelected ? 'border-primary-400 ring-2 ring-primary-400/40' : 'border-surface-800/70 hover:border-primary-400'
                }`}
                style={`height: ${mediaGridTileSize}px;`}
                type="button"
                data-media-asset-card="true"
                data-grid-index={index}
                onclick={(event) => onAssetClick(asset, event)}
                ondblclick={() => onAssetSelect(asset)}
                onpointerdown={(event) => onAssetDragStart(asset, event)}
                onpointerenter={(event) => onAssetDragEnter(asset, event)}
                oncontextmenu={(event) => {
                  if (event.ctrlKey || event.metaKey) event.preventDefault();
                }}
              >
                <div class="absolute inset-0">
                  {#if asset.kind === 'image'}
                    <img
                      class="h-full w-full object-cover transition duration-300 group-hover:scale-105"
                      src={assetPreviewSrc}
                      alt={asset.name}
                      draggable="false"
                      onerror={(event) => ((event.currentTarget as HTMLImageElement).src = assetOriginalSrc)}
                    />
                  {:else if asset.kind === 'video'}
                    <img
                      class="h-full w-full object-cover transition duration-300 group-hover:scale-105"
                      src={assetPreviewSrc}
                      alt={asset.name}
                      draggable="false"
                      loading="lazy"
                    />
                  {:else}
                    <div class="flex h-full w-full items-center justify-center bg-gradient-to-br from-surface-900/70 to-surface-950 text-xs uppercase tracking-[0.3em] text-surface-300">
                      {mediaKindLabel(asset.kind)}
                    </div>
                  {/if}
                </div>
                <div class="absolute inset-0 flex flex-col justify-between">
                  <div class="flex items-start justify-between p-1.5">
                    {#if showSelectionControls}
                      <label class="inline-flex items-center gap-1.5 rounded border border-surface-200/30 bg-surface-950/55 px-1.5 py-0.5 text-[0.62rem] uppercase tracking-[0.2em] text-surface-100 backdrop-blur-sm">
                        <input
                          type="checkbox"
                          checked={isSelected}
                          onclick={(event) => event.stopPropagation()}
                          onchange={() => toggleSelected(asset.id)}
                          aria-label={isSelected ? 'Unselect asset' : 'Select asset'}
                        />
                        Select
                      </label>
                    {/if}
                    <span class="rounded-full border border-surface-200/40 bg-surface-950/45 px-1.5 py-0.5 text-[0.62rem] uppercase tracking-[0.2em] text-surface-100 backdrop-blur-sm">
                      {mediaKindLabel(asset.kind)}
                    </span>
                  </div>
                  <div class="min-w-0 bg-gradient-to-t from-surface-950/95 via-surface-950/40 to-transparent p-2 text-surface-100">
                    <h3 class="truncate text-sm font-semibold leading-tight text-surface-50" title={asset.name}>{asset.name}</h3>
                    <p class="text-[0.68rem] text-surface-200/90">
                      {mediaDisplay[asset.id]?.sizeLabel ?? formatBytes(asset.sizeBytes)} · Updated {mediaDisplay[asset.id]?.updatedLabel ?? new Date(asset.updatedAt).toLocaleString()}
                    </p>
                    {#if affinity && (affinity.runtime || affinity.precision || affinity.quantized)}
                      <div class="mt-1.5 flex flex-wrap gap-1.5 text-[0.62rem] uppercase tracking-[0.18em] text-surface-300">
                        {#if affinity.runtime}
                          <span class="rounded-full border border-surface-700/70 bg-surface-900/70 px-1.5 py-[1px] text-surface-100">{affinity.runtime}</span>
                        {/if}
                        {#if affinity.precision}
                          <span class="rounded-full border border-surface-700/70 bg-surface-900/70 px-1.5 py-[1px] text-surface-100">{affinity.precision}</span>
                        {/if}
                        {#if affinity.quantized}
                          <span class="rounded-full border border-surface-700/70 bg-surface-900/70 px-1.5 py-[1px] text-surface-100">Quantized</span>
                        {/if}
                      </div>
                    {/if}
                  </div>
                </div>
              </button>
            {/snippet}
          </MediaAssetGrid>
        </div>
      </div>
    {:else}
      <div class="relative min-w-0" style={`height: ${mediaListTotalHeight}px;`}>
        <div class="absolute left-0 right-0 min-w-0" style={`transform: translateY(${mediaListOffset}px);`}>
          <MediaAssetList items={mediaListSlice} className="gap-0 rounded border border-surface-800/60" selectedIds={selectedIds}>
            {#snippet row(asset, index, isSelected)}
              {@const frameRate = typeof asset.fps === 'number' && Number.isFinite(asset.fps) ? asset.fps : null}
              {@const hasFrameRate = asset.kind === 'video' && frameRate !== null}
              {@const hasDimensions = typeof asset.width === 'number' && Number.isFinite(asset.width) && typeof asset.height === 'number' && Number.isFinite(asset.height)}
              {@const absoluteIndex = mediaListWindowStart + index}
              {@const affinity = asset.kind === 'model' ? deriveModelAffinity(asset.tags) : null}
              {@const assetPreviewSrc = assetPreviewSource(asset)}
              {@const assetOriginalSrc = assetOriginalSource(asset)}
              <button
                class={`flex w-full flex-col gap-2 border-b bg-surface-950/30 px-4 py-3 text-left md:flex-row md:items-center md:justify-between ${
                  isSelected ? 'border-primary-400/70 ring-1 ring-inset ring-primary-400/40' : 'border-surface-800/60'
                } ${absoluteIndex === filteredCount - 1 ? 'border-b-0' : ''}`}
                style={`height: ${mediaListRowHeight}px;`}
                type="button"
                data-media-asset-card="true"
                onclick={(event) => onAssetClick(asset, event)}
                ondblclick={() => onAssetSelect(asset)}
                onpointerdown={(event) => onAssetDragStart(asset, event)}
                onpointerenter={(event) => onAssetDragEnter(asset, event)}
                oncontextmenu={(event) => {
                  if (event.ctrlKey || event.metaKey) event.preventDefault();
                }}
              >
                <div class="flex min-w-0 items-center gap-3">
                  <div class="h-12 w-12 shrink-0 overflow-hidden rounded border border-surface-800/70 bg-surface-900/50">
                    {#if asset.kind === 'image' || asset.kind === 'video'}
                      <img
                        class="h-full w-full object-cover"
                        src={assetPreviewSrc}
                        alt={asset.name}
                        draggable="false"
                        loading="lazy"
                        onerror={(event) => ((event.currentTarget as HTMLImageElement).src = assetOriginalSrc)}
                      />
                    {:else}
                      <div class="flex h-full w-full items-center justify-center bg-gradient-to-br from-surface-900/70 to-surface-950 text-[0.58rem] uppercase tracking-[0.16em] text-surface-300">
                        {mediaKindLabel(asset.kind)}
                      </div>
                    {/if}
                  </div>
                  <div class="min-w-0">
                    <div class="flex items-center gap-2">
                      {#if showSelectionControls}
                        <input
                          type="checkbox"
                          checked={isSelected}
                          onclick={(event) => event.stopPropagation()}
                          onchange={() => toggleSelected(asset.id)}
                          aria-label={isSelected ? 'Unselect asset' : 'Select asset'}
                        />
                      {/if}
                      <p class="text-micro uppercase tracking-[0.3em] text-surface-500">{mediaKindLabel(asset.kind)}</p>
                    </div>
                    <p class="truncate text-base font-semibold text-surface-50" title={asset.name}>{asset.name}</p>
                    <p class="text-xs text-surface-500">
                      {mediaDisplay[asset.id]?.sizeLabel ?? formatBytes(asset.sizeBytes)} · Updated {mediaDisplay[asset.id]?.updatedLabel ?? new Date(asset.updatedAt).toLocaleString()}
                    </p>
                    {#if affinity && (affinity.runtime || affinity.precision || affinity.quantized)}
                      <div class="mt-1 flex flex-wrap gap-2 text-micro-tight uppercase tracking-[0.3em] text-surface-400">
                        {#if affinity.runtime}
                          <span>{affinity.runtime}</span>
                        {/if}
                        {#if affinity.precision}
                          <span>{affinity.precision}</span>
                        {/if}
                        {#if affinity.quantized}
                          <span>Quantized</span>
                        {/if}
                      </div>
                    {/if}
                  </div>
                </div>
                <div class="flex items-center gap-2 text-xs text-surface-500">
                  {#if hasDimensions}
                    <span>{asset.width}×{asset.height}</span>
                  {/if}
                  {#if hasDimensions && hasFrameRate}
                    <span>·</span>
                  {/if}
                  {#if hasFrameRate && frameRate !== null}
                    <span>{frameRate.toFixed(2)} fps</span>
                  {/if}
                </div>
              </button>
            {/snippet}
          </MediaAssetList>
        </div>
      </div>
    {/if}
    {#if !loading && filteredCount > 0 && hasMore}
      <div class="mt-4 flex justify-center">
        <button
          class="btn btn-sm preset-tonal uppercase tracking-[0.3em]"
          type="button"
          onclick={() => onLoadMore?.()}
          disabled={loadingMore}
        >
          {loadingMore ? 'Loading…' : 'Load more'}
        </button>
      </div>
    {/if}
  </div>
</Panel>
