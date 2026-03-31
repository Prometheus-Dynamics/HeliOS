import { browser } from '$app/environment';
import { get } from 'svelte/store';
import { toaster } from '$lib/toaster';
import { subscribeDomainInvalidations } from '$lib/api/invalidation';
import { scheduleAfterPaint } from '$lib/utils/browserSchedule';
import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';
import { createBackoffTimer } from '$lib/utils/backoff';
import { cancelDebounce, scheduleDebounce, type DebounceHandle } from '$lib/utils/debounce';
import { readStorage, removeStorage } from '$lib/utils/storage';
import { realtimeUpdateMatchesKind, type RealtimeUpdateEvent } from '$lib/api/realtimeUpdates';
import { subscribeMediaMutations } from '$lib/features/media/mutations';
import type { MediaAsset, MediaAssetType, MediaUploadProgress } from '$lib/features/media/api';
import { buildMediaArchiveDownloadUrl, deleteMediaAsset, uploadMediaAsset } from '$lib/features/media/api';
import { parseTagList } from '$lib/features/media/utils';
import { createMediaAssetsStore, type MediaAssetsStore } from '$lib/features/media/assetsStore';
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
import { createMediaDisplayWorker, createMediaFilterWorker } from '$lib/workers/factories';
import { faBoxArchive, faCircleQuestion, faCube, faDatabase, faImage, faLayerGroup, faMicrochip, faVideo } from '@fortawesome/free-solid-svg-icons';
import type { IconDefinition } from '@fortawesome/free-solid-svg-icons';

export type FilterOption = 'all' | MediaAssetType;
export type SortMode = MediaClientSort;
export type LayoutMode = 'grid' | 'list';
export type MediaDisplayState = Record<
  string,
  { sizeLabel: string; updatedLabel: string; dimensionLabel?: string; fpsLabel?: string }
>;

export const FILTERS: Array<{ id: FilterOption; label: string; description: string; icon: IconDefinition }> = [
  { id: 'all', label: 'All files', description: 'Everything stored locally', icon: faLayerGroup },
  { id: 'image', label: 'Images', description: 'Still frames and photo references', icon: faImage },
  { id: 'video', label: 'Videos', description: 'Playback clips and capture loops', icon: faVideo },
  { id: 'model', label: 'AI models', description: 'Edge deployable artifacts', icon: faCube },
  { id: 'archive', label: 'Archives', description: 'Compressed bundles and packages', icon: faBoxArchive },
  { id: 'firmware', label: 'Update images', description: 'Firmware or OTA artifacts', icon: faMicrochip },
  { id: 'data', label: 'Data files', description: 'Maps, configs, and logs', icon: faDatabase },
  { id: 'unknown', label: 'Unknown files', description: 'Unclassified assets', icon: faCircleQuestion }
];

export const SORT_OPTIONS: Array<{ id: SortMode; label: string }> = [
  { id: 'recent_desc', label: 'Recent (newest)' },
  { id: 'recent_asc', label: 'Recent (oldest)' },
  { id: 'size_desc', label: 'Size (largest)' },
  { id: 'size_asc', label: 'Size (smallest)' },
  { id: 'name_asc', label: 'Name (A-Z)' },
  { id: 'name_desc', label: 'Name (Z-A)' }
];

const PAGE_SIZE = 32;
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
const LIVE_UPDATES_REFRESH_DEBOUNCE_MS = 350;
const MEDIA_MUTATION_REFRESH_DEBOUNCE_MS = 150;

export const createMediaPageAssetsStore = (): MediaAssetsStore =>
  createMediaAssetsStore({
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

type RefreshOptions = { resetPage?: boolean; includeCounts?: boolean; page?: number };

type MediaPageSupportOptions = {
  mediaAssets: MediaAssetsStore;
  readAssets: () => MediaAsset[];
  readFilteredAssets: () => MediaAsset[];
  setFilteredAssets: (value: MediaAsset[]) => void;
  readLoading: () => boolean;
  readLoadingMore: () => boolean;
  readUploading: () => boolean;
  setUploading: (value: boolean) => void;
  setMediaBooting: (value: boolean) => void;
  readViewFilter: () => FilterOption;
  setViewFilter: (value: FilterOption) => void;
  readStreamFilter: () => string;
  setStreamFilter: (value: string) => void;
  readSortMode: () => SortMode;
  setSortMode: (value: SortMode) => void;
  setDropActive: (value: boolean) => void;
  setMediaDisplay: (value: MediaDisplayState) => void;
  setDetailModalOpen: (value: boolean) => void;
  setActiveAsset: (value: MediaAsset | null) => void;
  setShowSelectionControls: (value: boolean) => void;
};

const isAbortError = (error: unknown): boolean => {
  if (!error) return false;
  if (error instanceof DOMException && error.name === 'AbortError') return true;
  if (error instanceof Error && error.name === 'AbortError') return true;
  return (error as { name?: string }).name === 'AbortError';
};

export const createMediaPageSupport = (options: MediaPageSupportOptions) => {
  const mediaList = options.mediaAssets.list;
  const mediaState = mediaList.state;
  const selection = options.mediaAssets.selection;

  let uploadKind: MediaAssetType = 'image';
  let uploadName = '';
  let uploadDescription = '';
  let uploadTags = '';
  let uploadFiles: FileList | null = null;
  let uploadLabelFile: File | null = null;
  let uploadModelInputResolution = '';
  let uploadModelTensorSpec = '';
  let uploadToastId: string | null = null;
  let pendingOpenAssetId: string | null = null;
  let dragSelecting = false;
  let dragVisited: Record<string, boolean> = {};

  let mediaDisplayWorker: Worker | null = null;
  let mediaFilterWorker: Worker | null = null;
  let mediaFilterRequestId = 0;
  let mediaFilterLastHandled = 0;
  let mediaFilterTimeout: DebounceHandle = null;
  let mediaMutationRefreshHandle: ReturnType<typeof setTimeout> | null = null;
  let cancelMediaBootstrap: (() => void) | null = null;
  let cancelMediaDisplayBootstrap: (() => void) | null = null;
  let stopLiveUpdates: (() => void) | null = null;
  let unsubscribeMediaMutations: (() => void) | null = null;
  let started = false;

  const mediaRefreshTimer = createBackoffTimer({ baseMs: MEDIA_REFRESH_BASE_MS, maxMs: MEDIA_REFRESH_MAX_MS });

  const {
    toggleSelected,
    handleAssetClick,
    handleAssetDragStart,
    handleAssetDragEnter,
    clearSelection,
    selectVisible
  } = createMediaSelectionHandlers({
    selection,
    getFilteredAssets: options.readFilteredAssets,
    getDragSelecting: () => dragSelecting,
    setDragSelecting: (next) => {
      dragSelecting = next;
    },
    getDragVisited: () => dragVisited,
    setDragVisited: (next) => {
      dragVisited = next;
    }
  });

  const resetMediaRefreshBackoff = (): void => {
    mediaRefreshTimer.reset();
  };

  const bumpMediaRefreshBackoff = (): number => mediaRefreshTimer.bump();

  const scheduleMediaRefresh = (delay = mediaRefreshTimer.getDelay()): void => {
    mediaRefreshTimer.schedule(() => {
      if (options.readLoading() || options.readLoadingMore() || options.readUploading()) {
        scheduleMediaRefresh(delay);
        return;
      }
      void refreshAssets({ resetPage: true, includeCounts: true });
    }, delay);
  };

  const shouldApplyLiveUpdate = (event: RealtimeUpdateEvent): boolean => {
    if (
      event.path.startsWith('/v1/media') ||
      event.path.startsWith('/v1/streams') ||
      event.path.startsWith('/v1/localization')
    ) {
      return true;
    }
    if (realtimeUpdateMatchesKind(event, 'api')) {
      return false;
    }
    return (
      realtimeUpdateMatchesKind(event, 'media') ||
      realtimeUpdateMatchesKind(event, 'streams') ||
      realtimeUpdateMatchesKind(event, 'pipelines') ||
      realtimeUpdateMatchesKind(event, 'localization')
    );
  };

  const syncMediaFilters = (resetPage = true, refresh = true): void => {
    if (resetPage) {
      selection.clear();
    }
    const source = options.readStreamFilter() === 'all' ? null : options.readStreamFilter();
    const serverSort = options.readSortMode() === 'name_asc' || options.readSortMode() === 'name_desc' ? 'name' : 'recent';
    mediaList.setFilters(
      {
        kind: options.readViewFilter(),
        sort: serverSort,
        cameraSource: source
      },
      { refresh, resetPage }
    );
    if (source) {
      options.mediaAssets.rememberStream(source);
    }
  };

  const refreshAssets = async (runtimeOptions: RefreshOptions = { resetPage: true, includeCounts: true }) => {
    if (runtimeOptions.resetPage) {
      selection.clear();
    }
    await mediaList.refresh(runtimeOptions);
    if (get(mediaState).error) {
      const nextDelay = bumpMediaRefreshBackoff();
      scheduleMediaRefresh(nextDelay);
    } else {
      resetMediaRefreshBackoff();
      scheduleMediaRefresh();
    }
  };

  const bootstrapMediaPage = async (): Promise<void> => {
    options.setMediaBooting(true);
    syncMediaFilters(true, false);
    try {
      await Promise.all([options.mediaAssets.refreshStreamLabels(), refreshAssets({ resetPage: true, includeCounts: true })]);
    } finally {
      options.setMediaBooting(false);
    }
  };

  const decorateUploadToast = (progress: MediaUploadProgress): void => {
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
  };

  const clearUploadToastDecoration = (): void => {
    if (typeof document === 'undefined' || !uploadToastId) return;
    const element = document.getElementById(`toast:${uploadToastId}`);
    if (!element) return;
    element.removeAttribute('data-upload-toast');
    element.removeAttribute('data-upload-state');
    element.style.removeProperty('--upload-progress');
  };

  const scheduleUploadToastDecoration = (progress: MediaUploadProgress): void => {
    if (typeof window === 'undefined') return;
    window.requestAnimationFrame(() => decorateUploadToast(progress));
  };

  const showUploadToast = (progress: MediaUploadProgress | null, context: UploadToastContext): void => {
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
  };

  const resolveUploadToast = (success: boolean, description: string): void => {
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
  };

  const resetUploadFields = (): void => {
    uploadFiles = null;
    uploadLabelFile = null;
    uploadModelInputResolution = '';
    uploadModelTensorSpec = '';
    uploadName = '';
    uploadDescription = '';
    uploadTags = '';
  };

  const uploadFileBatch = async (files: File[], unsupported: File[] = []): Promise<void> => {
    await uploadFileBatchAction(files, unsupported, {
      setUploading: options.setUploading,
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
  };

  const submitUpload = async (): Promise<void> => {
    const filesForUpload = uploadFiles;
    if (!filesForUpload || filesForUpload.length === 0) {
      toaster.warning({ title: 'Select at least one file to upload.' });
      return;
    }

    if (filesForUpload.length > 1) {
      try {
        await uploadFileBatch(Array.from(filesForUpload));
        resetUploadFields();
      } catch (error) {
        reportError({ title: 'Upload failed', error });
      }
      return;
    }

    options.setUploading(true);
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
      resetUploadFields();
      await refreshAssets({ resetPage: true, includeCounts: true });
    } catch (error) {
      resolveUploadToast(false, buildErrorMessage({ error, fallback: 'Upload failed.' }));
    } finally {
      options.setUploading(false);
    }
  };

  const scheduleMutationRefresh = (): void => {
    if (!browser) return;
    if (mediaMutationRefreshHandle !== null) return;
    mediaMutationRefreshHandle = setTimeout(() => {
      mediaMutationRefreshHandle = null;
      void options.mediaAssets.refreshStreamLabels();
      void refreshAssets({ resetPage: true, includeCounts: true });
    }, MEDIA_MUTATION_REFRESH_DEBOUNCE_MS);
  };

  const handleAssetSelect = (asset: MediaAsset): void => {
    options.setActiveAsset(asset);
    options.setDetailModalOpen(true);
  };

  const handleViewFilterChange = (filterId: FilterOption): void => {
    if (options.readViewFilter() === filterId) return;
    options.setViewFilter(filterId);
    syncMediaFilters(true);
  };

  const handleStreamFilterChange = (filterId: string): void => {
    if (options.readStreamFilter() === filterId) return;
    options.setStreamFilter(filterId);
    syncMediaFilters(true);
  };

  const handleSortChange = (value: SortMode): void => {
    if (options.readSortMode() === value) return;
    options.setSortMode(value);
    syncMediaFilters(true);
  };

  const handleFilesSelected = (files: FileList | null, context: 'quick' | 'form'): void => {
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
  };

  const handleDragOver = (event: DragEvent): void => {
    event.preventDefault();
    options.setDropActive(true);
  };

  const handleDragLeave = (event: DragEvent): void => {
    const target = event.currentTarget as HTMLElement | null;
    if (!target) return;
    const related = event.relatedTarget as Node | null;
    if (!related || !target.contains(related)) {
      options.setDropActive(false);
    }
  };

  const handleDrop = async (event: DragEvent): Promise<void> => {
    event.preventDefault();
    options.setDropActive(false);
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
  };

  const deleteSelected = async (): Promise<void> => {
    await deleteSelectedAssets({
      selectedIds: [...get(selection.selectedIds)],
      deleteMediaAsset,
      refreshAssets,
      toaster,
      clearSelection
    });
  };

  const downloadSelectedArchive = (): void => {
    const ids = [...get(selection.selectedIds)];
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
  };

  const createMediaReplayStream = async (): Promise<void> => {
    await createMediaReplayStreamAction({
      selectedIds: [...get(selection.selectedIds)],
      assets: options.readAssets(),
      refreshStreamLabels: () => options.mediaAssets.refreshStreamLabels(),
      toaster,
      clearSelection
    });
  };

  const createMediaReplayStreamForAsset = async (asset: MediaAsset): Promise<void> => {
    await createMediaReplayStreamAction({
      selectedIds: [asset.id],
      assets: [asset],
      refreshStreamLabels: () => options.mediaAssets.refreshStreamLabels(),
      toaster,
      clearSelection: () => {}
    });
  };

  const loadMoreAssets = async (): Promise<void> => {
    if (options.readUploading()) return;
    await mediaList.loadMore();
  };

  const streamLabel = (source: string): string => options.mediaAssets.streamLabel(source);

  const syncSearchQuery = (searchQuery: string): void => {
    mediaList.setQuery(searchQuery, { debounceMs: 250 });
  };

  const syncFilteredAssets = (
    assets: MediaAsset[],
    searchQuery: string,
    streamFilter: string,
    viewFilter: FilterOption,
    sortMode: SortMode
  ): void => {
    if (!mediaFilterWorker) {
      options.setFilteredAssets(filterMediaAssets(assets, { searchQuery, streamFilter, viewFilter, sortMode }));
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
      options.setFilteredAssets(filterMediaAssets(assets, { searchQuery, streamFilter, viewFilter, sortMode }));
    }, MEDIA_FILTER_TIMEOUT_MS);
  };

  const syncMediaDisplay = (assets: MediaAsset[]): void => {
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
  };

  const resolvePendingOpenAsset = (assets: MediaAsset[]): void => {
    if (!pendingOpenAssetId) return;
    if (!assets.length) return;
    const match = assets.find((item) => item.id === pendingOpenAssetId);
    pendingOpenAssetId = null;
    if (match) {
      handleAssetSelect(match);
    }
  };

  const start = (): (() => void) => {
    if (started) return destroy;
    started = true;

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

    cancelMediaBootstrap = scheduleAfterPaint(() => {
      if (typeof Worker !== 'undefined') {
        mediaFilterWorker = createMediaFilterWorker();
        mediaFilterWorker.onmessage = (event) => {
          const payload = event.data as { requestId: number; filtered?: MediaAsset[] };
          if (payload.requestId < mediaFilterLastHandled) return;
          mediaFilterLastHandled = payload.requestId;
          mediaFilterTimeout = cancelDebounce(mediaFilterTimeout);
          options.setFilteredAssets(Array.isArray(payload.filtered) ? payload.filtered : []);
        };
      }
      void bootstrapMediaPage();
    }, 1);

    cancelMediaDisplayBootstrap = scheduleAfterPaint(() => {
      if (typeof Worker === 'undefined') return;
      mediaDisplayWorker = createMediaDisplayWorker();
      mediaDisplayWorker.onmessage = (event) => {
        const payload = event.data as { display?: MediaDisplayState };
        options.setMediaDisplay(payload?.display ?? {});
      };
    }, 2);

    stopLiveUpdates = subscribeDomainInvalidations(
      ['media', 'streams', 'pipelines', 'localization'],
      (event) => {
        if (document.hidden) return;
        if (!shouldApplyLiveUpdate(event)) return;
        void options.mediaAssets.refreshStreamLabels();
        void refreshAssets({ resetPage: false, includeCounts: true });
      },
      { debounceMs: LIVE_UPDATES_REFRESH_DEBOUNCE_MS }
    );

    unsubscribeMediaMutations = subscribeMediaMutations(() => {
      scheduleMutationRefresh();
    });

    const isEditableEventTarget = (target: EventTarget | null): boolean => {
      if (!(target instanceof Element)) return false;
      return Boolean(target.closest('input, textarea, select, [contenteditable="true"]'));
    };

    const syncSelectionControls = (event: KeyboardEvent | null): void => {
      const next = Boolean(event?.shiftKey || event?.ctrlKey || event?.metaKey);
      options.setShowSelectionControls(next);
    };

    const clearSelectionControls = (): void => {
      options.setShowSelectionControls(false);
    };

    const onKeyDown = (event: KeyboardEvent) => {
      syncSelectionControls(event);
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

    const onKeyUp = (event: KeyboardEvent) => syncSelectionControls(event);
    const onPointerUp = () => {
      dragSelecting = false;
      dragVisited = {};
    };

    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keyup', onKeyUp, { passive: true });
    window.addEventListener('blur', clearSelectionControls, { passive: true });
    window.addEventListener('pointerup', onPointerUp, { passive: true });

    return () => {
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('keyup', onKeyUp);
      window.removeEventListener('blur', clearSelectionControls);
      window.removeEventListener('pointerup', onPointerUp);
      destroy();
    };
  };

  const destroy = (): void => {
    if (!started) return;
    started = false;
    stopLiveUpdates?.();
    stopLiveUpdates = null;
    unsubscribeMediaMutations?.();
    unsubscribeMediaMutations = null;
    mediaRefreshTimer.cancel();
    options.mediaAssets.destroy();
    cancelMediaBootstrap?.();
    cancelMediaBootstrap = null;
    cancelMediaDisplayBootstrap?.();
    cancelMediaDisplayBootstrap = null;
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
    options.setShowSelectionControls(false);
  };

  return {
    availableStreams: options.mediaAssets.availableStreams,
    selectionState: selection.state,
    selectedIds: selection.selectedIds,
    selectedCount: selection.selectedCount,
    toggleSelected,
    handleAssetClick,
    handleAssetDragStart,
    handleAssetDragEnter,
    clearSelection,
    selectVisible,
    start,
    destroy,
    syncSearchQuery,
    syncFilteredAssets,
    syncMediaDisplay,
    resolvePendingOpenAsset,
    streamLabel,
    loadMoreAssets,
    deleteSelected,
    downloadSelectedArchive,
    createMediaReplayStream,
    createMediaReplayStreamForAsset,
    handleViewFilterChange,
    handleStreamFilterChange,
    handleSortChange,
    handleAssetSelect,
    handleDragOver,
    handleDragLeave,
    handleDrop,
    handleFilesSelected
  };
};
