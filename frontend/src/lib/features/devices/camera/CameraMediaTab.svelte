<script lang="ts">
  import { onDestroy } from 'svelte';
  import { toaster } from '$lib';
  import CameraMediaDetailModal from '$lib/features/devices/camera/media/CameraMediaDetailModal.svelte';
  import CameraMediaFilters from '$lib/features/devices/camera/media/CameraMediaFilters.svelte';
  import CameraMediaGrid from '$lib/features/devices/camera/media/CameraMediaGrid.svelte';
  import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';
  import {
    applyImageEdits,
    applyVideoEdits,
    attachLabelFile,
    deleteMediaAsset,
    fetchLabelText,
    type MediaAsset,
    type MediaAssetType,
    type MediaListSort
  } from '$lib/features/media/api';
  import { createMediaAssetsStore } from '$lib/features/media/assetsStore';
  import { subscribeMediaMutations, type MediaMutationEvent } from '$lib/features/media/mutations';
  import { hydrateAssetDimensions } from '$lib/features/media/assetHelpers';
  import { hydrateMediaMetadata, saveMediaMetadata } from '$lib/features/media/mediaDetailController';

  let {
    streamId
  }: {
    streamId: string | null;
  } = $props();

  const mediaAssets = createMediaAssetsStore({ listOptions: { includeCounts: false } });
  const mediaList = mediaAssets.list;
  const mediaState = mediaList.state;
  const assets = $derived($mediaState.assets);
  const total = $derived($mediaState.total);
  const loading = $derived($mediaState.loading);
  const error = $derived($mediaState.error);
  let kind = $state<'all' | MediaAssetType>('all');
  let sort = $state<MediaListSort>('recent');
  let query = $state('');
  let detailModalOpen = $state(false);
  let activeAsset = $state<MediaAsset | null>(null);
  let metadataBusy = $state(false);
  let renameBaseValue = $state('');
  let descriptionValue = $state('');
  let tagsValue = $state('');
  let metadataDirty = $state(false);
  let attachLabelBusy = $state(false);
  let imageEdit = $state({ rotateDegrees: 0, crop: { x: 0, y: 0, width: 0, height: 0 } });
  let imageEditBusy = $state(false);
  let imageEditToastId = $state<string | null>(null);
  let videoEdit = $state({ startMs: 0, endMs: 0 });
  let labelPreview = $state<string | null>(null);
  let labelPreviewLoading = $state(false);
  let labelPreviewError = $state<string | null>(null);
  let dimensionHydrateSeq = 0;
  let mediaMutationRefreshTimer: ReturnType<typeof setTimeout> | null = null;

  const visibleAssets = $derived(assets);

  function scheduleMediaRefresh(event?: MediaMutationEvent): void {
    const eventSource = String(event?.cameraSource ?? '').trim();
    const selectedSource = String(streamId ?? '').trim();
    if (eventSource.length && selectedSource.length && eventSource !== selectedSource) return;
    if (mediaMutationRefreshTimer !== null) return;
    mediaMutationRefreshTimer = setTimeout(() => {
      mediaMutationRefreshTimer = null;
      void mediaList.refresh({ resetPage: true, includeCounts: false });
    }, 150);
  }

  const unsubscribeMediaMutations = subscribeMediaMutations((event) => {
    scheduleMediaRefresh(event);
  });

  function syncImageEditFromAsset(asset: MediaAsset | null) {
    if (!asset || asset.kind !== 'image') return;
    if (asset.imageCrop) {
      imageEdit = {
        rotateDegrees: 0,
        crop: {
          x: asset.imageCrop.x,
          y: asset.imageCrop.y,
          width: asset.imageCrop.width,
          height: asset.imageCrop.height
        }
      };
      return;
    }
    resetImageEditToFullAsset(asset);
  }

  function resetImageEditToFullAsset(asset: MediaAsset) {
    if (asset.kind !== 'image') return;
    imageEdit = {
      rotateDegrees: 0,
      crop: {
        x: 0,
        y: 0,
        width: asset.width,
        height: asset.height
      }
    };
  }

  async function ensureAssetDimensions(asset: MediaAsset): Promise<void> {
    if (!asset || (asset.width > 0 && asset.height > 0)) return;
    const requestId = ++dimensionHydrateSeq;
    const updated = await hydrateAssetDimensions(asset);
    if (!updated || requestId !== dimensionHydrateSeq) return;
    mediaList.replaceAsset(updated);
    if (activeAsset?.id === asset.id) {
      activeAsset = updated;
    }
  }

  $effect(() => {
    mediaList.setFilters({ cameraSource: streamId, kind, sort });
  });

  // Drain paginated media responses so the tab shows the full filtered set.
  $effect(() => {
    if (!streamId) return;
    const snapshot = $mediaState;
    if (snapshot.loading || snapshot.loadingMore || snapshot.error) return;
    if (snapshot.assets.length === 0 || snapshot.assets.length >= snapshot.total) return;
    void mediaList.loadMore();
  });

  $effect(() => {
    if (detailModalOpen && activeAsset) {
      const fields = hydrateMediaMetadata(activeAsset);
      renameBaseValue = fields.renameBase;
      descriptionValue = fields.description;
      tagsValue = fields.tags;
      metadataDirty = false;
      if (activeAsset.kind === 'image') {
        syncImageEditFromAsset(activeAsset);
      }
      if (activeAsset.kind === 'video' && activeAsset.videoClip) {
        videoEdit = {
          startMs: activeAsset.videoClip.start_ms,
          endMs: activeAsset.videoClip.end_ms ?? 0
        };
      } else {
        videoEdit = { startMs: 0, endMs: 0 };
      }
      if (activeAsset.labelAttached) {
        void loadLabelPreview(activeAsset.id);
      } else {
        labelPreview = null;
        labelPreviewError = null;
      }
      void ensureAssetDimensions(activeAsset);
    } else {
      renameBaseValue = '';
      labelPreview = null;
      labelPreviewError = null;
    }
  });

  function scheduleSearch(value: string): void {
    query = value;
    mediaList.setQuery(value, { debounceMs: 250 });
  }

  function handleMetadataChange() {
    metadataDirty = true;
  }

  async function persistMetadata() {
    if (!activeAsset) return;
    metadataBusy = true;
    try {
      const updated = await saveMediaMetadata(activeAsset, {
        renameBase: renameBaseValue,
        description: descriptionValue,
        tags: tagsValue
      });
      mediaList.replaceAsset(updated);
      activeAsset = updated;
      metadataDirty = false;
      toaster.success({ title: 'Saved changes' });
    } catch (error) {
      reportError({ title: 'Failed to save', error });
    } finally {
      metadataBusy = false;
    }
  }

  async function handleDelete(asset: MediaAsset) {
    if (!confirm(`Delete \"${asset.name}\"? This cannot be undone.`)) return;
    try {
      await deleteMediaAsset(asset.id);
      toaster.success({ title: 'Asset deleted' });
      mediaList.removeAsset(asset.id);
      if (activeAsset?.id === asset.id) {
        activeAsset = null;
        detailModalOpen = false;
      }
    } catch (error) {
      reportError({ title: 'Delete failed', error });
    }
  }

  async function handleAttachLabel(event: Event) {
    if (!activeAsset) return;
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    attachLabelBusy = true;
    try {
      const updated = await attachLabelFile(activeAsset.id, file);
      mediaList.replaceAsset(updated);
      activeAsset = updated;
      await loadLabelPreview(updated.id);
      toaster.success({ title: 'Label attached' });
    } catch (error) {
      reportError({ title: 'Attach failed', error });
    } finally {
      attachLabelBusy = false;
      input.value = '';
    }
  }

  async function loadLabelPreview(assetId: string) {
    labelPreviewLoading = true;
    labelPreviewError = null;
    try {
      labelPreview = await fetchLabelText(assetId);
    } catch (error) {
      labelPreview = null;
      labelPreviewError = buildErrorMessage({ error, fallback: 'Unable to load label preview.' });
    } finally {
      labelPreviewLoading = false;
    }
  }

  async function submitImageEdits() {
    if (!activeAsset || activeAsset.kind !== 'image' || imageEditBusy) return;
    imageEditBusy = true;
    const pendingToast = toaster.create({
      type: 'loading',
      title: 'Updating image',
      description: activeAsset.name,
      duration: 600000,
      closable: false
    });
    imageEditToastId = pendingToast;
    try {
      const updated = await applyImageEdits(activeAsset.id, {
        rotateDegrees: imageEdit.rotateDegrees,
        crop: imageEdit.crop
      });
      mediaList.replaceAsset(updated);
      activeAsset = updated;
      resetImageEditToFullAsset(updated);
      if (imageEditToastId) {
        toaster.update(imageEditToastId, {
          type: 'success',
          title: 'Image updated',
          description: updated.name,
          duration: 4000,
          closable: true
        });
        imageEditToastId = null;
      } else {
        toaster.success({ title: 'Image updated' });
      }
    } catch (error) {
      const message = buildErrorMessage({ error });
      if (imageEditToastId) {
        toaster.update(imageEditToastId, {
          type: 'error',
          title: 'Image edit failed',
          description: message,
          duration: 6000,
          closable: true
        });
        imageEditToastId = null;
      } else {
        reportError({ title: 'Image edit failed', description: message });
      }
    } finally {
      imageEditBusy = false;
    }
  }

  async function submitVideoEdits() {
    if (!activeAsset || activeAsset.kind !== 'video') return;
    try {
      const updated = await applyVideoEdits(activeAsset.id, {
        startMs: videoEdit.startMs,
        endMs: videoEdit.endMs || undefined
      });
      mediaList.replaceAsset(updated);
      activeAsset = updated;
      toaster.success({ title: 'Clip updated' });
    } catch (error) {
      reportError({ title: 'Clip edit failed', error });
    }
  }

  function handleAssetSelect(asset: MediaAsset) {
    activeAsset = asset;
    detailModalOpen = true;
  }

  function closeDetailModal() {
    detailModalOpen = false;
  }

  onDestroy(() => {
    if (mediaMutationRefreshTimer !== null) {
      clearTimeout(mediaMutationRefreshTimer);
      mediaMutationRefreshTimer = null;
    }
    unsubscribeMediaMutations();
    mediaAssets.destroy();
  });

  function openMediaViewer(asset: MediaAsset): void {
    handleAssetSelect(asset);
  }
</script>

<div class="flex h-full min-h-0 flex-col gap-3">
  <CameraMediaFilters bind:kind bind:sort {query} onQueryChange={scheduleSearch} />

  <CameraMediaGrid
    {streamId}
    {loading}
    {error}
    {assets}
    {visibleAssets}
    {total}
    onSelect={openMediaViewer}
  />
</div>

<CameraMediaDetailModal
  open={detailModalOpen}
  asset={activeAsset}
  bind:renameBaseValue
  bind:descriptionValue
  bind:tagsValue
  {metadataDirty}
  {metadataBusy}
  bind:imageEdit
  {imageEditBusy}
  bind:videoEdit
  {attachLabelBusy}
  {labelPreview}
  {labelPreviewLoading}
  {labelPreviewError}
  onClose={closeDetailModal}
  onDelete={handleDelete}
  onSaveMetadata={persistMetadata}
  onSubmitImageEdits={submitImageEdits}
  onSubmitVideoEdits={submitVideoEdits}
  onAttachLabel={handleAttachLabel}
  onMetadataChange={handleMetadataChange}
/>
