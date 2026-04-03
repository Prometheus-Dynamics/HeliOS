<script lang="ts">
  import { onMount } from 'svelte';
  import { getVirtualWindow } from '$lib/ui/virtualViewport';
  import MediaFilters from '$lib/components/media/MediaFilters.svelte';
  import MediaLibraryPanel from '$lib/features/media/page/MediaLibraryPanel.svelte';
  import MediaDetailModalView from '$lib/features/media/page/MediaDetailModalView.svelte';
  import type { MediaAsset } from '$lib/features/media/api';
  import {
    FILTERS,
    SORT_OPTIONS,
    createMediaPageAssetsStore,
    createMediaPageSupport,
    type FilterOption,
    type LayoutMode,
    type MediaDisplayState,
    type SortMode
  } from './mediaPageSupport';

  const MEDIA_GRID_GAP = 12;
  const MEDIA_GRID_OVERSCAN = 2;
  const MEDIA_LIST_ROW_HEIGHT = 88;
  const MEDIA_LIST_OVERSCAN = 6;

  const mediaAssets = createMediaPageAssetsStore();
  const mediaList = mediaAssets.list;
  const mediaState = mediaList.state;
  const assets = $derived($mediaState.assets);
  const totalAssets = $derived($mediaState.total);
  const loading = $derived($mediaState.loading);
  const loadingMore = $derived($mediaState.loadingMore);

  let uploading = $state(false);
  let mediaBooting = $state(true);
  let viewFilter = $state<FilterOption>('all');
  let streamFilter = $state<string>('all');
  let searchQuery = $state('');
  let sortMode = $state<SortMode>('recent_desc');
  let layoutMode = $state<LayoutMode>('grid');
  let detailModalOpen = $state(false);
  let activeAsset = $state<MediaAsset | null>(null);
  let manualFileInput = $state<HTMLInputElement | null>(null);
  let dropActive = $state(false);
  let showSelectionControls = $state(false);
  let mediaDisplay = $state<MediaDisplayState>({});
  let mediaScrollTop = $state(0);
  let mediaViewportHeight = $state(0);
  let mediaViewportWidth = $state(0);
  let filteredAssets = $state<MediaAsset[]>([]);

  const mediaSupport = createMediaPageSupport({
    mediaAssets,
    readAssets: () => assets,
    readFilteredAssets: () => filteredAssets,
    setFilteredAssets: (value) => (filteredAssets = value),
    readLoading: () => loading,
    readLoadingMore: () => loadingMore,
    readUploading: () => uploading,
    setUploading: (value) => (uploading = value),
    setMediaBooting: (value) => (mediaBooting = value),
    readViewFilter: () => viewFilter,
    setViewFilter: (value) => (viewFilter = value),
    readStreamFilter: () => streamFilter,
    setStreamFilter: (value) => (streamFilter = value),
    readSortMode: () => sortMode,
    setSortMode: (value) => (sortMode = value),
    setDropActive: (value) => (dropActive = value),
    setMediaDisplay: (value) => (mediaDisplay = value),
    setDetailModalOpen: (value) => (detailModalOpen = value),
    setActiveAsset: (value) => (activeAsset = value),
    setShowSelectionControls: (value) => (showSelectionControls = value)
  });

  const availableStreams = mediaSupport.availableStreams;
  const selectionState = mediaSupport.selectionState;
  const selectedIds = mediaSupport.selectedIds;
  const selectedCount = mediaSupport.selectedCount;

  onMount(() => mediaSupport.start());

  function handleViewFilterSelect(id: string): void {
    if (!FILTERS.some((filter) => filter.id === id)) return;
    mediaSupport.handleViewFilterChange(id as FilterOption);
  }

  function handleSortModeChange(mode: string): void {
    if (!SORT_OPTIONS.some((option) => option.id === mode)) return;
    mediaSupport.handleSortChange(mode as SortMode);
  }

  function handleLayoutModeChange(mode: string): void {
    if (mode !== 'grid' && mode !== 'list') return;
    layoutMode = mode;
  }

  $effect(() => {
    mediaSupport.syncSearchQuery(searchQuery);
  });

  $effect(() => {
    mediaSupport.syncFilteredAssets(assets, searchQuery, streamFilter, viewFilter, sortMode);
  });

  $effect(() => {
    mediaSupport.syncMediaDisplay(assets);
  });

  $effect(() => {
    mediaSupport.resolvePendingOpenAsset(assets);
  });

  const hasMore = $derived(totalAssets > assets.length);
  const mediaGridColumns = $derived.by(() => {
    if (mediaViewportWidth >= 1536) return 9;
    if (mediaViewportWidth >= 1280) return 7;
    if (mediaViewportWidth >= 1024) return 6;
    if (mediaViewportWidth >= 640) return 4;
    return 1;
  });
  const mediaGridTileSize = $derived.by(() => {
    const width = mediaViewportWidth || 0;
    if (width <= 0) return 140;
    const columns = mediaGridColumns || 1;
    const totalGap = MEDIA_GRID_GAP * Math.max(0, columns - 1);
    return Math.max(120, Math.floor((width - totalGap) / columns));
  });
  const mediaGridRowHeight = $derived(mediaGridTileSize + MEDIA_GRID_GAP);
  const mediaGridRowCount = $derived.by(() => (mediaGridColumns > 0 ? Math.ceil(filteredAssets.length / mediaGridColumns) : 0));
  const mediaGridWindow = $derived.by(() =>
    getVirtualWindow({
      itemCount: mediaGridRowCount,
      rowHeight: mediaGridRowHeight,
      overscan: MEDIA_GRID_OVERSCAN,
      scrollTop: mediaScrollTop,
      viewportHeight: mediaViewportHeight
    })
  );
  const mediaGridTotalHeight = $derived.by(() => Math.max(0, mediaGridWindow.totalHeight - MEDIA_GRID_GAP));
  const mediaGridOffset = $derived.by(() => mediaGridWindow.offset);
  const mediaGridSlice = $derived.by(() =>
    filteredAssets.slice(mediaGridWindow.startIndex * mediaGridColumns, mediaGridWindow.endIndex * mediaGridColumns)
  );

  const mediaListWindow = $derived.by(() =>
    getVirtualWindow({
      itemCount: filteredAssets.length,
      rowHeight: MEDIA_LIST_ROW_HEIGHT,
      overscan: MEDIA_LIST_OVERSCAN,
      scrollTop: mediaScrollTop,
      viewportHeight: mediaViewportHeight
    })
  );
  const mediaListTotalHeight = $derived.by(() => mediaListWindow.totalHeight);
  const mediaListOffset = $derived.by(() => mediaListWindow.offset);
  const mediaListSlice = $derived.by(() => filteredAssets.slice(mediaListWindow.startIndex, mediaListWindow.endIndex));
</script>

<section
  class="flex h-full min-h-0 min-w-0 flex-1 flex-col gap-6 overflow-hidden"
  aria-label="Media library workspace"
  ondragover={mediaSupport.handleDragOver}
  ondragleave={mediaSupport.handleDragLeave}
  ondrop={mediaSupport.handleDrop}
>
  <div class="flex min-h-0 min-w-0 flex-1 flex-col gap-6 overflow-hidden lg:flex-row">
    <MediaFilters
      className="min-h-0 lg:h-full lg:max-w-[16rem] xl:max-w-[16.75rem] 2xl:max-w-[17.5rem]"
      bind:searchValue={searchQuery}
      filters={FILTERS}
      selectedFilter={viewFilter}
      onSelectFilter={handleViewFilterSelect}
      streams={$availableStreams}
      selectedStream={streamFilter}
      onSelectStream={mediaSupport.handleStreamFilterChange}
      streamLabel={mediaSupport.streamLabel}
    >
      {#snippet actions()}
        <button
          class="btn btn-xs preset-filled-primary-500 w-full uppercase tracking-[0.22em]"
          type="button"
          disabled={uploading}
          onclick={() => manualFileInput?.click()}
        >
          {uploading ? 'Uploading…' : 'Upload files'}
        </button>
        <input
          class="sr-only"
          type="file"
          bind:this={manualFileInput}
          multiple
          disabled={uploading}
          onchange={(event) => mediaSupport.handleFilesSelected(event.currentTarget.files, 'quick')}
        />
      {/snippet}
      {#snippet footer()}
        <p class="mt-3 text-xs text-surface-500">
          Focus on the assets powering perception, playback, and deployment. We load media in pages; use filters or search
          to narrow results and click “Load more” to fetch the next batch.
        </p>
      {/snippet}
    </MediaFilters>

    <div class="flex min-h-0 min-w-0 flex-1 flex-col gap-6 overflow-hidden">
      <MediaLibraryPanel
        loading={mediaBooting || loading}
        loadingMore={loadingMore}
        hasMore={hasMore}
        filteredCount={filteredAssets.length}
        selectedCount={$selectedCount}
        sortMode={sortMode}
        sortOptions={SORT_OPTIONS}
        onSortChange={handleSortModeChange}
        layoutMode={layoutMode}
        onLayoutChange={handleLayoutModeChange}
        onClearSelection={mediaSupport.clearSelection}
        onCreateMediaStream={() => void mediaSupport.createMediaReplayStream()}
        onDownloadSelected={mediaSupport.downloadSelectedArchive}
        onDeleteSelected={() => void mediaSupport.deleteSelected()}
        onLoadMore={mediaSupport.loadMoreAssets}
        onScroll={(top) => (mediaScrollTop = top)}
        onResize={({ height, width }) => {
          mediaViewportHeight = height;
          mediaViewportWidth = width;
        }}
        mediaGridSlice={mediaGridSlice}
        mediaGridColumns={mediaGridColumns}
        mediaGridTileSize={mediaGridTileSize}
        mediaGridTotalHeight={mediaGridTotalHeight}
        mediaGridOffset={mediaGridOffset}
        mediaGridGap={MEDIA_GRID_GAP}
        mediaListSlice={mediaListSlice}
        mediaListOffset={mediaListOffset}
        mediaListTotalHeight={mediaListTotalHeight}
        mediaListRowHeight={MEDIA_LIST_ROW_HEIGHT}
        mediaListWindowStart={mediaListWindow.startIndex}
        selectedIds={$selectionState.selected}
        showSelectionControls={showSelectionControls}
        toggleSelected={mediaSupport.toggleSelected}
        onAssetClick={mediaSupport.handleAssetClick}
        onAssetSelect={mediaSupport.handleAssetSelect}
        onAssetDragStart={mediaSupport.handleAssetDragStart}
        onAssetDragEnter={mediaSupport.handleAssetDragEnter}
        mediaDisplay={mediaDisplay}
      />
    </div>
  </div>
</section>

{#if dropActive}
  <div class="pointer-events-none fixed inset-0 z-40 flex items-center justify-center bg-primary-500/10">
    <div class="rounded border-2 border-dashed border-primary-400 bg-surface-950/80 px-6 py-4 text-center text-surface-200">
      Drop files to upload
    </div>
  </div>
{/if}

<MediaDetailModalView
  bind:open={detailModalOpen}
  bind:asset={activeAsset}
  mediaDisplay={mediaDisplay}
  onCreateMediaStream={(asset) => mediaSupport.createMediaReplayStreamForAsset(asset)}
  onReplaceAsset={(asset) => mediaList.replaceAsset(asset)}
  onRemoveAsset={(assetId) => mediaList.removeAsset(assetId)}
/>

<style>
  @import './mediaPage.css';
</style>
