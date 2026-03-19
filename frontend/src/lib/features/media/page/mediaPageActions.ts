import type { MediaAsset, MediaAssetType } from '$lib/features/media/api';
import type { MediaAssetsStore } from '$lib/features/media/assetsStore';
import { apiUrl } from '$lib';
import { apiFetch } from '$lib/api/core/http';
import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';

export type MediaClientSort =
  | 'recent_desc'
  | 'recent_asc'
  | 'size_desc'
  | 'size_asc'
  | 'name_asc'
  | 'name_desc';

export function filterMediaAssets(
  payload: MediaAsset[],
  options: {
    searchQuery: string;
    streamFilter: string;
    viewFilter: 'all' | MediaAssetType;
    sortMode: MediaClientSort;
  }
): MediaAsset[] {
  const normalizedSearch = options.searchQuery.trim().toLowerCase();
  const normalizedStream = options.streamFilter.trim().toLowerCase();
  const kindFilter = options.viewFilter;
  const filtered = payload.filter((asset) => {
    if (kindFilter !== 'all' && asset.kind !== kindFilter) return false;
    if (normalizedStream && normalizedStream !== 'all') {
      const source = (asset.cameraSource ?? '').trim().toLowerCase();
      if (source !== normalizedStream) return false;
    }
    if (normalizedSearch && !asset.name.toLowerCase().includes(normalizedSearch)) return false;
    return true;
  });

  const timestamp = (asset: MediaAsset): number => {
    const updated = Date.parse(asset.updatedAt ?? '');
    if (Number.isFinite(updated)) return updated;
    const created = Date.parse(asset.createdAt ?? '');
    return Number.isFinite(created) ? created : 0;
  };

  const sorted = [...filtered];
  switch (options.sortMode) {
    case 'recent_asc':
      sorted.sort((a, b) => timestamp(a) - timestamp(b) || a.name.localeCompare(b.name));
      break;
    case 'size_desc':
      sorted.sort((a, b) => (b.sizeBytes ?? 0) - (a.sizeBytes ?? 0) || a.name.localeCompare(b.name));
      break;
    case 'size_asc':
      sorted.sort((a, b) => (a.sizeBytes ?? 0) - (b.sizeBytes ?? 0) || a.name.localeCompare(b.name));
      break;
    case 'name_asc':
      sorted.sort((a, b) => a.name.localeCompare(b.name));
      break;
    case 'name_desc':
      sorted.sort((a, b) => b.name.localeCompare(a.name));
      break;
    case 'recent_desc':
    default:
      sorted.sort((a, b) => timestamp(b) - timestamp(a) || a.name.localeCompare(b.name));
      break;
  }

  return sorted;
}

export function snapshotAssetsForWorker(payload: MediaAsset[]): MediaAsset[] {
  try {
    return structuredClone(payload) as MediaAsset[];
  } catch {
    return JSON.parse(JSON.stringify(payload)) as MediaAsset[];
  }
}

export function createMediaSelectionHandlers(context: {
  selection: MediaAssetsStore['selection'];
  getFilteredAssets: () => MediaAsset[];
  getDragSelecting: () => boolean;
  setDragSelecting: (next: boolean) => void;
  getDragVisited: () => Record<string, boolean>;
  setDragVisited: (next: Record<string, boolean>) => void;
}) {
  const toggleSelected = (assetId: string): void => {
    context.selection.toggle(assetId);
  };

  const setSelectedOnly = (assetId: string): void => {
    context.selection.setOnly(assetId);
  };

  const addSelected = (assetId: string): void => {
    context.selection.add(assetId);
  };

  const rangeSelect = (toAssetId: string, mode: 'replace' | 'add'): void => {
    const ids = context.getFilteredAssets().map((asset) => asset.id);
    context.selection.selectRange(ids, toAssetId, mode);
  };

  const handleAssetClick = (asset: MediaAsset, event: MouseEvent): void => {
    const heldShift = event.shiftKey;
    const heldCtrl = event.ctrlKey || event.metaKey;

    event.preventDefault();
    event.stopPropagation();
    if (heldShift) {
      rangeSelect(asset.id, heldCtrl ? 'add' : 'replace');
      return;
    }
    if (heldCtrl) {
      toggleSelected(asset.id);
      return;
    }
    setSelectedOnly(asset.id);
  };

  const handleAssetDragStart = (): void => {
    // Modifier-based drag toggling felt unlike standard file explorers and fought click selection.
    context.setDragSelecting(false);
    context.setDragVisited({});
  };

  const handleAssetDragEnter = (): void => {};

  const clearSelection = (): void => {
    context.selection.clear();
  };

  const selectVisible = (): void => {
    context.selection.setAll(context.getFilteredAssets().map((asset) => asset.id));
  };

  return {
    toggleSelected,
    setSelectedOnly,
    addSelected,
    rangeSelect,
    handleAssetClick,
    handleAssetDragStart,
    handleAssetDragEnter,
    clearSelection,
    selectVisible
  };
}

export async function deleteSelectedAssets(context: {
  selectedIds: string[];
  deleteMediaAsset: (id: string) => Promise<void>;
  refreshAssets: (options: { resetPage?: boolean; includeCounts?: boolean }) => Promise<void>;
  toaster: { error: (payload: { title: string; description: string }) => void; success: (payload: { title: string; description: string }) => void };
  clearSelection: () => void;
}): Promise<void> {
  const total = context.selectedIds.length;
  if (!total) return;
  if (!confirm(`Delete ${total} file${total === 1 ? '' : 's'}? This cannot be undone.`)) return;
  const failures: string[] = [];
  for (const id of context.selectedIds) {
    try {
      await context.deleteMediaAsset(id);
    } catch (error) {
      failures.push(`${id}: ${buildErrorMessage({ error, fallback: 'Delete failed.' })}`);
    }
  }
  context.clearSelection();
  await context.refreshAssets({ resetPage: true, includeCounts: true });
  if (failures.length) {
    context.toaster.error({ title: 'Some deletes failed', description: failures.join('\n') });
  } else {
    context.toaster.success({ title: 'Deleted', description: `Removed ${total} file${total === 1 ? '' : 's'}` });
  }
}

export async function createMediaReplayStream(context: {
  selectedIds: string[];
  assets: MediaAsset[];
  refreshStreamLabels: () => Promise<void> | void;
  toaster: {
    warning: (payload: { title: string; description: string }) => void;
    success: (payload: { title: string; description: string }) => void;
  };
  clearSelection: () => void;
}): Promise<void> {
  const ids = context.selectedIds;
  if (!ids.length) {
    context.toaster.warning({ title: 'Select media first', description: 'Pick at least one image or video.' });
    return;
  }
  const unsupported = ids.filter((id) => {
    const asset = context.assets.find((item) => item.id === id);
    return asset ? !['image', 'video'].includes(asset.kind) : false;
  });
  const files = ids.filter((id) => !unsupported.includes(id));
  if (!files.length) {
    context.toaster.warning({ title: 'No playable media', description: 'Selected items must be images or videos.' });
    return;
  }

  const alias = `media-replay-${new Date().toISOString().replace(/[:.]/g, '-')}`;
  const selectedAssets = files
    .map((id) => context.assets.find((item) => item.id === id))
    .filter((asset): asset is MediaAsset => Boolean(asset));
  const videoFpsHint =
    selectedAssets.find((asset) => asset.kind === 'video' && Number.isFinite(asset.fps) && asset.fps > 0)?.fps ?? null;
  const replayFps = Math.max(1, Math.min(240, Math.round(videoFpsHint ?? 10)));
  try {
    const payload = await apiFetch<{ stream_id?: string; streamId?: string; error?: string; message?: string }>(apiUrl('/streams/replay/media'), {
      method: 'POST',
      body: {
        files,
        fps: replayFps,
        loopForever: true,
        alias
      }
    });
    const streamId = payload?.stream_id ?? payload?.streamId;
    context.toaster.success({
      title: 'Media stream started',
      description: streamId ? `Stream ${streamId} is live.` : 'Stream is live.'
    });
    context.clearSelection();
    await context.refreshStreamLabels();
  } catch (error) {
    reportError({ title: 'Media stream failed', error });
  } finally {
    if (unsupported.length) {
      context.toaster.warning({
        title: 'Skipped non-media files',
        description: `${unsupported.length} selected item${unsupported.length === 1 ? '' : 's'} were not images/videos.`
      });
    }
  }
}
