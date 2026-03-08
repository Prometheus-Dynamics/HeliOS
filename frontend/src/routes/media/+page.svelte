<script lang="ts">
  import { browser } from '$app/environment';
  import { onDestroy, onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { toaster } from '$lib';
  import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';
  import { getVirtualWindow } from '$lib/ui/virtualViewport';
  import { createBackoffTimer } from '$lib/utils/backoff';
  import { cancelDebounce, scheduleDebounce, type DebounceHandle } from '$lib/utils/debounce';
  import { readStorage, removeStorage } from '$lib/utils/storage';
  import { connectRealtimeUpdatesStream, type RealtimeUpdateEvent } from '$lib/api/realtimeUpdates';
  import { subscribeMediaMutations } from '$lib/features/media/mutations';
  import MediaFilters from '$lib/components/media/MediaFilters.svelte';
  import MediaLibraryPanel from '$lib/features/media/page/MediaLibraryPanel.svelte';
  import MediaDetailModalView from '$lib/features/media/page/MediaDetailModalView.svelte';
  import { faBoxArchive, faCircleQuestion, faCube, faDatabase, faImage, faLayerGroup, faMicrochip, faVideo } from '@fortawesome/free-solid-svg-icons';
  import type { IconDefinition } from '@fortawesome/free-solid-svg-icons';
  import type { MediaAsset, MediaAssetType, MediaUploadProgress } from '$lib/features/media/api';
  import { buildMediaArchiveDownloadUrl, deleteMediaAsset, uploadMediaAsset } from '$lib/features/media/api';
  import { parseTagList } from '$lib/features/media/utils';
  import { createMediaAssetsStore } from '$lib/features/media/assetsStore';
  import {
    createFileListFromArray,
    detectMediaKind,
    extractFilesFromDataTransfer,
    fileDisplayName,
    inferKindFromFile,
    sumFileSizes
  } from '$lib/features/media/page/mediaPageFiles';
  import {
    createMediaReplayStream as createMediaReplayStreamAction,
    createMediaSelectionHandlers,
    deleteSelectedAssets,
    filterMediaAssets,
    snapshotAssetsForWorker,
    type MediaClientSort
  } from '$lib/features/media/page/mediaPageActions';
  import { uploadFileBatch as uploadFileBatchAction } from '$lib/features/media/page/mediaUploadBatch';
  import {
    combineProgress,
    createProgressSnapshot,
    formatUploadLabel,
    normalizeProgress,
    type UploadToastContext
  } from '$lib/features/media/page/mediaUploadProgress';
  import { SvelteURL } from 'svelte/reactivity';

  type FilterOption = 'all' | MediaAssetType;
  type SortMode = MediaClientSort;
  type LayoutMode = 'grid' | 'list';

  const FILTERS: Array<{ id: FilterOption; label: string; description: string; icon: IconDefinition }> = [
    { id: 'all', label: 'All files', description: 'Everything stored locally', icon: faLayerGroup },
    { id: 'image', label: 'Images', description: 'Still frames and photo references', icon: faImage },
    { id: 'video', label: 'Videos', description: 'Playback clips and capture loops', icon: faVideo },
    { id: 'model', label: 'AI models', description: 'Edge deployable artifacts', icon: faCube },
    { id: 'archive', label: 'Archives', description: 'Compressed bundles and packages', icon: faBoxArchive },
    { id: 'firmware', label: 'Update images', description: 'Firmware or OTA artifacts', icon: faMicrochip },
    { id: 'data', label: 'Data files', description: 'Maps, configs, and logs', icon: faDatabase },
    { id: 'unknown', label: 'Unknown files', description: 'Unclassified assets', icon: faCircleQuestion }
  ];
  const SORT_OPTIONS: Array<{ id: SortMode; label: string }> = [
    { id: 'recent_desc', label: 'Recent (newest)' },
    { id: 'recent_asc', label: 'Recent (oldest)' },
    { id: 'size_desc', label: 'Size (largest)' },
    { id: 'size_asc', label: 'Size (smallest)' },
    { id: 'name_asc', label: 'Name (A-Z)' },
    { id: 'name_desc', label: 'Name (Z-A)' }
  ];
  const PAGE_SIZE = 32;
  const MEDIA_GRID_GAP = 12;
  const MEDIA_GRID_OVERSCAN = 2;
  const MEDIA_LIST_ROW_HEIGHT = 88;
  const MEDIA_LIST_OVERSCAN = 6;
  const MEDIA_REFRESH_BASE_MS = 30_000;
  const MEDIA_REFRESH_MAX_MS = 120_000;
  const MEDIA_CACHE_STALE_MS = 10_000;
  const MEDIA_CACHE_MAX_MS = 120_000;
  const MEDIA_CACHE_KEY_PREFIX = 'media:assets:v1';
  const MEDIA_FILTER_TIMEOUT_MS = 2_000;
  const STREAM_LABELS_CACHE_KEY = 'media:stream-labels:v1';
  const STREAM_LABELS_CACHE_STALE_MS = 30_000;
  const STREAM_LABELS_CACHE_MAX_MS = 120_000;
  const MEDIA_OPEN_ASSET_KEY = 'helios.media.openAsset';
  const LIVE_UPDATES_RECONNECT_MS = 1_500;
  const LIVE_UPDATES_REFRESH_DEBOUNCE_MS = 350;
  const MEDIA_MUTATION_REFRESH_DEBOUNCE_MS = 150;

const mediaAssets = createMediaAssetsStore({
  listOptions: {
    pageSize: PAGE_SIZE,
    includeCounts: true,
    useCache: true,
    cacheKeyPrefix: MEDIA_CACHE_KEY_PREFIX,
    cacheStaleMs: MEDIA_CACHE_STALE_MS,
    cacheMaxAgeMs: MEDIA_CACHE_MAX_MS
  },
  streamLabels: {
    cacheKey: STREAM_LABELS_CACHE_KEY,
    staleMs: STREAM_LABELS_CACHE_STALE_MS,
    maxAgeMs: STREAM_LABELS_CACHE_MAX_MS
  }
});
const mediaList = mediaAssets.list;
const selection = mediaAssets.selection;
const mediaState = mediaList.state;
const assets = $derived($mediaState.assets);
const totalAssets = $derived($mediaState.total);
const loading = $derived($mediaState.loading);
const loadingMore = $derived($mediaState.loadingMore);
let uploadKind = $state<MediaAssetType>('image');
let uploadName = $state('');
let uploadDescription = $state('');
let pendingOpenAssetId = $state<string | null>(null);
let uploadTags = $state('');
let uploadFiles = $state<FileList | null>(null);
let uploadLabelFile = $state<File | null>(null);
let uploadModelInputResolution = $state('');
let uploadModelTensorSpec = $state('');
let uploading = $state(false);
let uploadToastId = $state<string | null>(null);
let viewFilter = $state<FilterOption>('all');
let streamFilter = $state<string>('all');
let searchQuery = $state('');
let sortMode = $state<SortMode>('recent_desc');
let layoutMode = $state<LayoutMode>('grid');
let detailModalOpen = $state(false);
let activeAsset = $state<MediaAsset | null>(null);
let manualFileInput = $state<HTMLInputElement | null>(null);
let dropActive = $state(false);
let shiftDown = $state(false);
let ctrlDown = $state(false);
let dragSelecting = $state(false);
let dragVisited = $state<Record<string, boolean>>({});
let mediaDisplayWorker: Worker | null = null;
let mediaDisplay = $state<Record<string, { sizeLabel: string; updatedLabel: string; dimensionLabel?: string; fpsLabel?: string }>>({});
let mediaFilterWorker: Worker | null = null;
let mediaFilterRequestId = 0;
let mediaFilterLastHandled = 0;
let mediaFilterTimeout: DebounceHandle = null;
let liveUpdatesCleanup: (() => void) | null = null;
let liveUpdatesReconnectHandle: number | null = null;
let liveUpdatesRefreshHandle: number | null = null;
let mediaMutationRefreshHandle: ReturnType<typeof setTimeout> | null = null;
let liveUpdatesNonce = 0;
let mediaScrollTop = $state(0);
let mediaViewportHeight = $state(0);
let mediaViewportWidth = $state(0);
const mediaRefreshTimer = createBackoffTimer({ baseMs: MEDIA_REFRESH_BASE_MS, maxMs: MEDIA_REFRESH_MAX_MS });

const unsubscribeMediaMutations = subscribeMediaMutations(() => {
  if (!browser) return;
  if (mediaMutationRefreshHandle !== null) return;
  mediaMutationRefreshHandle = setTimeout(() => {
    mediaMutationRefreshHandle = null;
    void mediaAssets.refreshStreamLabels();
    void refreshAssets({ resetPage: true, includeCounts: true });
  }, MEDIA_MUTATION_REFRESH_DEBOUNCE_MS);
});

const selectionState = selection.state;
const selectedIds = selection.selectedIds;
const selectedCount = selection.selectedCount;
const selectionHandlers = createMediaSelectionHandlers({
  selection,
  getFilteredAssets: () => filteredAssets,
  getDragSelecting: () => dragSelecting,
  setDragSelecting: (next) => (dragSelecting = next),
  getDragVisited: () => dragVisited,
  setDragVisited: (next) => (dragVisited = next)
});
const {
  toggleSelected,
  handleAssetClick,
  handleAssetDragStart,
  handleAssetDragEnter,
  clearSelection,
  selectVisible
} = selectionHandlers;

  function isEditableEventTarget(target: EventTarget | null): boolean {
    if (!(target instanceof Element)) return false;
    return Boolean(target.closest('input, textarea, select, [contenteditable="true"]'));
  }

  onMount(() => {
    if (browser) {
      try {
        const requested = readStorage(MEDIA_OPEN_ASSET_KEY);
        if (requested) {
          pendingOpenAssetId = requested;
          removeStorage(MEDIA_OPEN_ASSET_KEY);
        }
      } catch {
        pendingOpenAssetId = null;
      }
    }
    if (typeof Worker !== 'undefined') {
      mediaFilterWorker = new Worker(new SvelteURL('$lib/workers/mediaFilterWorker.ts', import.meta.url), { type: 'module' });
      mediaFilterWorker.onmessage = (event) => {
        const payload = event.data as { requestId: number; filtered?: MediaAsset[] };
        if (payload.requestId < mediaFilterLastHandled) return;
        mediaFilterLastHandled = payload.requestId;
        mediaFilterTimeout = cancelDebounce(mediaFilterTimeout);
        filteredAssets = Array.isArray(payload.filtered) ? payload.filtered : [];
      };
    }
    syncMediaFilters(true);
    void mediaAssets.refreshStreamLabels();
    connectLiveUpdates();
    scheduleMediaRefresh();

    const syncKeys = (event: KeyboardEvent) => {
      shiftDown = event.shiftKey;
      ctrlDown = event.ctrlKey || event.metaKey;
    };
    const clearKeys = () => {
      shiftDown = false;
      ctrlDown = false;
    };
    const onKeyDown = (event: KeyboardEvent) => {
      syncKeys(event);
      if (event.defaultPrevented) return;
      if (isEditableEventTarget(event.target)) return;

      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') {
        event.preventDefault();
        selectVisible();
        return;
      }

      if (event.key === 'Escape') {
        event.preventDefault();
        clearSelection();
      }
    };
    const onKeyUp = (event: KeyboardEvent) => syncKeys(event);
    const onPointerUp = () => {
      dragSelecting = false;
      dragVisited = {};
    };

    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keyup', onKeyUp, { passive: true });
    window.addEventListener('blur', clearKeys, { passive: true });
    window.addEventListener('pointerup', onPointerUp, { passive: true });
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('keyup', onKeyUp);
      window.removeEventListener('blur', clearKeys);
      window.removeEventListener('pointerup', onPointerUp);
    };
  });

  onMount(() => {
    if (typeof Worker === 'undefined') return;
    mediaDisplayWorker = new Worker(new SvelteURL('$lib/workers/mediaDisplayWorker.ts', import.meta.url), { type: 'module' });
    mediaDisplayWorker.onmessage = (event) => {
      const payload = event.data as { display?: typeof mediaDisplay };
      mediaDisplay = payload?.display ?? {};
    };
  });

  $effect(() => {
    if (!mediaDisplayWorker) return;
    const payload = assets.map((asset) => ({
      id: asset.id,
      sizeBytes: asset.sizeBytes,
      updatedAt: asset.updatedAt,
      width: asset.width ?? null,
      height: asset.height ?? null,
      fps: asset.fps ?? null
    }));
    mediaDisplayWorker.postMessage({ assets: payload });
  });

  $effect(() => {
    if (!pendingOpenAssetId) return;
    if (!assets.length) return;
    const match = assets.find((item) => item.id === pendingOpenAssetId);
    pendingOpenAssetId = null;
    if (match) {
      handleAssetSelect(match);
    }
  });

  onDestroy(() => {
    unsubscribeMediaMutations();
    disconnectLiveUpdates();
    mediaRefreshTimer.cancel();
    mediaAssets.destroy();
    if (mediaMutationRefreshHandle !== null) {
      clearTimeout(mediaMutationRefreshHandle);
      mediaMutationRefreshHandle = null;
    }
    if (mediaDisplayWorker) {
      mediaDisplayWorker.terminate();
      mediaDisplayWorker = null;
    }
    if (mediaFilterWorker) {
      mediaFilterWorker.terminate();
      mediaFilterWorker = null;
    }
    mediaFilterTimeout = cancelDebounce(mediaFilterTimeout);
  });

  $effect(() => {
    mediaList.setQuery(searchQuery, { debounceMs: 250 });
  });

  const availableStreams = mediaAssets.availableStreams;

  let filteredAssets = $state<MediaAsset[]>([]);
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

  function resetMediaRefreshBackoff(): void {
    mediaRefreshTimer.reset();
  }

  function bumpMediaRefreshBackoff(): number {
    return mediaRefreshTimer.bump();
  }

  function scheduleMediaRefresh(delay = mediaRefreshTimer.getDelay()): void {
    mediaRefreshTimer.schedule(() => {
      if (loading || loadingMore || uploading) {
        scheduleMediaRefresh(delay);
        return;
      }
      void refreshAssets({ resetPage: true, includeCounts: true });
    }, delay);
  }

  function shouldApplyLiveUpdate(event: RealtimeUpdateEvent): boolean {
    if (
      event.path.startsWith('/v1/media') ||
      event.path.startsWith('/v1/streams') ||
      event.path.startsWith('/v1/localization')
    ) {
      return true;
    }
    if (event.kind === 'api') {
      return false;
    }
    return event.kind === 'media' || event.kind === 'streams' || event.kind === 'pipelines' || event.kind === 'localization';
  }

  function scheduleLiveUpdatesRefresh(): void {
    if (!browser) return;
    if (liveUpdatesRefreshHandle != null) return;
    liveUpdatesRefreshHandle = window.setTimeout(() => {
      liveUpdatesRefreshHandle = null;
      if (document.hidden) return;
      void mediaAssets.refreshStreamLabels();
      void refreshAssets({ resetPage: false, includeCounts: true });
    }, LIVE_UPDATES_REFRESH_DEBOUNCE_MS);
  }

  function scheduleLiveUpdatesReconnect(): void {
    if (!browser) return;
    if (liveUpdatesReconnectHandle != null) return;
    liveUpdatesReconnectHandle = window.setTimeout(() => {
      liveUpdatesReconnectHandle = null;
      connectLiveUpdates();
    }, LIVE_UPDATES_RECONNECT_MS);
  }

  function disconnectLiveUpdates(): void {
    liveUpdatesNonce += 1;
    if (liveUpdatesReconnectHandle != null) {
      clearTimeout(liveUpdatesReconnectHandle);
      liveUpdatesReconnectHandle = null;
    }
    if (liveUpdatesRefreshHandle != null) {
      clearTimeout(liveUpdatesRefreshHandle);
      liveUpdatesRefreshHandle = null;
    }
    liveUpdatesCleanup?.();
    liveUpdatesCleanup = null;
  }

  function connectLiveUpdates(): void {
    if (!browser) return;
    const nonce = (liveUpdatesNonce += 1);
    if (liveUpdatesReconnectHandle != null) {
      clearTimeout(liveUpdatesReconnectHandle);
      liveUpdatesReconnectHandle = null;
    }
    liveUpdatesCleanup?.();
    liveUpdatesCleanup = null;
    liveUpdatesCleanup = connectRealtimeUpdatesStream({
      onChange: (event) => {
        if (nonce !== liveUpdatesNonce) return;
        if (!shouldApplyLiveUpdate(event)) return;
        scheduleLiveUpdatesRefresh();
      },
      onClose: () => {
        if (nonce !== liveUpdatesNonce) return;
        scheduleLiveUpdatesReconnect();
      },
      onError: () => {
        if (nonce !== liveUpdatesNonce) return;
        scheduleLiveUpdatesReconnect();
      }
    });
  }

  $effect(() => {
    if (!mediaFilterWorker) {
      filteredAssets = filterMediaAssets(assets, { searchQuery, streamFilter, viewFilter, sortMode });
      return;
    }
    const nextRequestId = ++mediaFilterRequestId;
    const snapshot = snapshotAssetsForWorker(assets);
    mediaFilterWorker.postMessage({
      requestId: nextRequestId,
      assets: snapshot,
      search: searchQuery,
      streamFilter,
      kindFilter: viewFilter,
      sortMode
    });
    mediaFilterTimeout = scheduleDebounce(mediaFilterTimeout, () => {
      if (nextRequestId < mediaFilterLastHandled) return;
      mediaFilterLastHandled = nextRequestId;
      filteredAssets = filterMediaAssets(assets, { searchQuery, streamFilter, viewFilter, sortMode });
    }, MEDIA_FILTER_TIMEOUT_MS);
  });

  type RefreshOptions = { resetPage?: boolean; includeCounts?: boolean; page?: number };

  async function refreshAssets(options: RefreshOptions = { resetPage: true, includeCounts: true }) {
    if (options.resetPage) {
      selection.clear();
    }
    await mediaList.refresh(options);
    if (get(mediaState).error) {
      const nextDelay = bumpMediaRefreshBackoff();
      scheduleMediaRefresh(nextDelay);
    } else {
      resetMediaRefreshBackoff();
      scheduleMediaRefresh();
    }
  }

  async function loadMoreAssets() {
    if (uploading) return;
    await mediaList.loadMore();
  }

  function streamLabel(source: string): string {
    return mediaAssets.streamLabel(source);
  }

  const showSelectionControls = $derived(shiftDown || ctrlDown);

  async function deleteSelected(): Promise<void> {
    await deleteSelectedAssets({
      selectedIds: [...$selectedIds],
      deleteMediaAsset,
      refreshAssets,
      toaster,
      clearSelection
    });
  }

  function downloadSelectedArchive(): void {
    const ids = [...$selectedIds];
    if (!ids.length) {
      toaster.warning({ title: 'Select media first', description: 'Pick at least one file to download.' });
      return;
    }
    if (!browser) return;
    const link = document.createElement('a');
    link.href = buildMediaArchiveDownloadUrl(ids);
    link.rel = 'noreferrer';
    link.download = `media-selection-${new Date().toISOString().replace(/[:.]/g, '-')}.zip`;
    document.body.appendChild(link);
    link.click();
    link.remove();
  }

  async function createMediaReplayStream(): Promise<void> {
    await createMediaReplayStreamAction({
      selectedIds: [...$selectedIds],
      assets,
      refreshStreamLabels: () => mediaAssets.refreshStreamLabels(),
      toaster,
      clearSelection
    });
  }

  async function createMediaReplayStreamForAsset(asset: MediaAsset): Promise<void> {
    await createMediaReplayStreamAction({
      selectedIds: [asset.id],
      assets: [asset],
      refreshStreamLabels: () => mediaAssets.refreshStreamLabels(),
      toaster,
      clearSelection: () => {}
    });
  }

  function syncMediaFilters(resetPage = true): void {
    if (resetPage) {
      selection.clear();
    }
    const source = streamFilter === 'all' ? null : streamFilter;
    const serverSort = sortMode === 'name_asc' || sortMode === 'name_desc' ? 'name' : 'recent';
    mediaList.setFilters(
      {
        kind: viewFilter,
        sort: serverSort,
        cameraSource: source
      },
      { refresh: true, resetPage }
    );
    if (source) {
      mediaAssets.rememberStream(source);
    }
  }

  function handleViewFilterChange(filterId: FilterOption) {
    if (viewFilter === filterId) return;
    viewFilter = filterId;
    syncMediaFilters(true);
  }

  function handleStreamFilterChange(filterId: string) {
    if (streamFilter === filterId) return;
    streamFilter = filterId;
    syncMediaFilters(true);
  }

  function handleSortChange(value: SortMode) {
    if (sortMode === value) return;
    sortMode = value;
    syncMediaFilters(true);
  }

  async function submitUpload(event?: SubmitEvent) {
    event?.preventDefault();
    const filesForUpload = uploadFiles;
    if (!filesForUpload || filesForUpload.length === 0) {
      toaster.warning({ title: 'Select at least one file to upload.' });
      return;
    }

    if (filesForUpload.length > 1) {
      try {
        await uploadFileBatch(Array.from(filesForUpload));
        uploadModelInputResolution = '';
        uploadModelTensorSpec = '';
        uploadName = '';
        uploadDescription = '';
        uploadTags = '';
      } catch (error) {
        reportError({ title: 'Upload failed', error });
      }
      return;
    }

    uploading = true;
    const totalBytes = sumFileSizes(Array.from(filesForUpload));
    const initialProgress = createProgressSnapshot(0, totalBytes);
    showUploadToast(initialProgress, { totalFiles: filesForUpload.length });
    try {
      const asset = await uploadMediaAsset({
        files: filesForUpload,
        kind: uploadKind,
        name: uploadName.trim() || undefined,
        description: uploadDescription.trim() || undefined,
        tags: parseTagList(uploadTags),
        labelFile: uploadLabelFile ?? undefined,
        modelInputResolution: uploadKind === 'model' ? uploadModelInputResolution.trim() || undefined : undefined,
        modelTensorSpec: uploadKind === 'model' ? uploadModelTensorSpec.trim() || undefined : undefined,
        onProgress: (progress) => {
          const aggregated = combineProgress(progress, 0, totalBytes);
          showUploadToast(aggregated, { totalFiles: filesForUpload.length });
        }
      });
      resolveUploadToast(true, asset.name);
      uploadFiles = null;
      uploadLabelFile = null;
      uploadModelInputResolution = '';
      uploadModelTensorSpec = '';
      uploadName = '';
      uploadDescription = '';
      uploadTags = '';
      await refreshAssets({ resetPage: true, includeCounts: true });
    } catch (error) {
      resolveUploadToast(false, buildErrorMessage({ error, fallback: 'Upload failed.' }));
    } finally {
      uploading = false;
    }
  }

  function handleAssetSelect(asset: MediaAsset) {
    activeAsset = asset;
    detailModalOpen = true;
  }

  function handleDragOver(event: DragEvent) {
    event.preventDefault();
    dropActive = true;
  }

  function handleDragLeave(event: DragEvent) {
    const target = event.currentTarget as HTMLElement | null;
    if (!target) return;
    const related = event.relatedTarget as Node | null;
    if (!related || !target.contains(related)) {
      dropActive = false;
    }
  }

  async function handleDrop(event: DragEvent) {
    event.preventDefault();
    dropActive = false;
    const droppedFiles = await extractFilesFromDataTransfer(event.dataTransfer ?? null);
    if (!droppedFiles.length) return;
    const supported: File[] = [];
    const unsupported: File[] = [];
    droppedFiles.forEach((file) => {
      if (detectMediaKind(file)) {
        supported.push(file);
      } else {
        unsupported.push(file);
      }
    });
    if (supported.length === 1 && unsupported.length === 0) {
      uploadFiles = createFileListFromArray(supported);
      uploadKind = inferKindFromFile(supported[0]);
      void submitUpload();
      return;
    }
    void uploadFileBatch(supported, unsupported);
  }

  function handleFilesSelected(files: FileList | null, context: 'quick' | 'form') {
    if (!files || files.length === 0) return;
    if (context === 'quick' && files.length > 1) {
      const supported: File[] = [];
      const unsupported: File[] = [];
      Array.from(files).forEach((file) => {
        if (detectMediaKind(file)) {
          supported.push(file);
        } else {
          unsupported.push(file);
        }
      });
      void uploadFileBatch(supported, unsupported);
      return;
    }
    const detectedKind = detectMediaKind(files[0]);
    if (context === 'quick' && !detectedKind) {
      void uploadFileBatch([], [files[0]]);
      return;
    }
    uploadFiles = files;
    uploadKind = detectedKind ?? inferKindFromFile(files[0]);
    if (context === 'quick') {
      void submitUpload();
    }
  }

  function showUploadToast(progress: MediaUploadProgress | null, context: UploadToastContext) {
    const normalized = progress ?? { loaded: 0, total: undefined, percent: null };
    const label = formatUploadLabel(context, normalized);
    const payload = {
      type: 'loading' as const,
      title: label,
      description: '',
      duration: 600000,
      closable: false,
      meta: { progress: normalized }
    };
    const activeId = uploadToastId;
    if (activeId && typeof toaster.isVisible === 'function' && !toaster.isVisible(activeId)) {
      uploadToastId = null;
    }
    if (uploadToastId) {
      try {
        toaster.update(uploadToastId, payload);
      } catch {
        uploadToastId = toaster.create(payload);
      }
    } else {
      uploadToastId = toaster.create(payload);
    }
    scheduleUploadToastDecoration(normalized);
  }

  function resolveUploadToast(success: boolean, description: string) {
    if (uploadToastId) {
      toaster.update(uploadToastId, {
        type: success ? 'success' : 'error',
        title: success ? 'Upload complete' : 'Upload failed',
        description,
        duration: 5000,
        closable: true,
        meta: {}
      });
      clearUploadToastDecoration();
      uploadToastId = null;
    } else {
      const notify = success ? toaster.success : toaster.error;
      notify({ title: success ? 'Upload complete' : 'Upload failed', description });
    }
  }


  function scheduleUploadToastDecoration(progress: MediaUploadProgress) {
    if (typeof window === 'undefined') return;
    window.requestAnimationFrame(() => decorateUploadToast(progress));
  }

  function decorateUploadToast(progress: MediaUploadProgress) {
    if (typeof document === 'undefined' || !uploadToastId) return;
    const element = document.getElementById(`toast:${uploadToastId}`);
    if (!element) return;
    element.dataset.uploadToast = 'true';
    if (progress.percent != null && Number.isFinite(progress.percent)) {
      element.dataset.uploadState = 'determinate';
      element.style.setProperty('--upload-progress', `${Math.max(0, Math.min(100, progress.percent)).toFixed(0)}%`);
    } else {
      element.dataset.uploadState = 'indeterminate';
      element.style.removeProperty('--upload-progress');
    }
  }

  function clearUploadToastDecoration() {
    if (typeof document === 'undefined' || !uploadToastId) return;
    const element = document.getElementById(`toast:${uploadToastId}`);
    if (!element) return;
    element.removeAttribute('data-upload-toast');
    element.removeAttribute('data-upload-state');
    element.style.removeProperty('--upload-progress');
  }

  async function uploadFileBatch(files: File[], unsupported: File[] = []): Promise<void> {
    await uploadFileBatchAction(files, unsupported, {
      setUploading: (value) => (uploading = value),
      clearUploadState: () => {
        uploadFiles = null;
        uploadLabelFile = null;
      },
      createFileListFromArray,
      detectMediaKind,
      inferKindFromFile,
      fileDisplayName,
      sumFileSizes,
      createProgressSnapshot,
      normalizeProgress,
      combineProgress,
      uploadMediaAsset,
      showUploadToast,
      resolveUploadToast,
      refreshAssets,
      toaster
    });
  }
</script>

<section
  class="flex h-full min-h-0 min-w-0 flex-1 flex-col gap-6 overflow-hidden"
  aria-label="Media library workspace"
  ondragover={handleDragOver}
  ondragleave={handleDragLeave}
  ondrop={handleDrop}
>
  <div class="flex min-h-0 min-w-0 flex-1 flex-col gap-6 overflow-hidden lg:flex-row">
    <MediaFilters
      className="min-h-0 lg:h-full lg:max-w-[16rem] xl:max-w-[16.75rem] 2xl:max-w-[17.5rem]"
      bind:searchValue={searchQuery}
      filters={FILTERS}
      selectedFilter={viewFilter}
      onSelectFilter={(id) => handleViewFilterChange(id as FilterOption)}
      streams={$availableStreams}
      selectedStream={streamFilter}
      onSelectStream={handleStreamFilterChange}
      streamLabel={streamLabel}
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
          onchange={(event) => handleFilesSelected((event.currentTarget as HTMLInputElement).files, 'quick')}
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
        loading={loading}
        loadingMore={loadingMore}
        hasMore={hasMore}
        filteredCount={filteredAssets.length}
        selectedCount={$selectedCount}
        sortMode={sortMode}
        sortOptions={SORT_OPTIONS}
        onSortChange={(mode) => handleSortChange(mode as SortMode)}
        layoutMode={layoutMode}
        onLayoutChange={(mode) => (layoutMode = mode as LayoutMode)}
        onClearSelection={clearSelection}
        onCreateMediaStream={() => void createMediaReplayStream()}
        onDownloadSelected={downloadSelectedArchive}
        onDeleteSelected={() => void deleteSelected()}
        onLoadMore={loadMoreAssets}
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
        toggleSelected={toggleSelected}
        onAssetClick={handleAssetClick}
        onAssetSelect={handleAssetSelect}
        onAssetDragStart={handleAssetDragStart}
        onAssetDragEnter={handleAssetDragEnter}
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
  onCreateMediaStream={(asset) => createMediaReplayStreamForAsset(asset)}
  onReplaceAsset={(asset) => mediaList.replaceAsset(asset)}
  onRemoveAsset={(assetId) => mediaList.removeAsset(assetId)}
/>

<style>
  @import './mediaPage.css';
</style>
