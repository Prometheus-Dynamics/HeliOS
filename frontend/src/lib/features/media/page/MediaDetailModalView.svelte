<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { MediaAsset } from '$lib/features/media/api';
  import { toaster } from '$lib';
  import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';
  import MediaDetailModal from '$lib/components/media/MediaDetailModal.svelte';
  import ImageCropper from '$lib/components/media/ImageCropper.svelte';
  import MediaImuPlaybackPanel from '$lib/features/media/page/MediaImuPlaybackPanel.svelte';
  import {
    applyImageEdits,
    applyVideoEdits,
    attachLabelFile,
    deleteMediaAsset,
    fetchLabelText
  } from '$lib/features/media/api';
  import {
    assetPlaybackSource,
    assetMp4DownloadSource,
    assetOriginalSource,
    assetPreviewSource,
    deriveModelAffinity,
    formatVideoCodec,
    formatBytes,
    getAssetExtension,
    parseTensorSpec
  } from '$lib/features/media/utils';
  import { mediaKindLabel } from '$lib/features/media/mediaKind';
  import { describeTensorEntry, hydrateAssetDimensions } from '$lib/features/media/assetHelpers';
  import { hydrateMediaMetadata, saveMediaMetadata } from '$lib/features/media/mediaDetailController';

  type MediaDisplay = Record<
    string,
    { sizeLabel: string; updatedLabel: string; dimensionLabel?: string; fpsLabel?: string }
  >;

  type MediaDetailModalViewProps = {
    open?: boolean;
    asset?: MediaAsset | null;
    mediaDisplay?: MediaDisplay;
    onClose?: () => void;
    onCreateMediaStream?: (asset: MediaAsset) => void | Promise<void>;
    onReplaceAsset?: (asset: MediaAsset) => void;
    onRemoveAsset?: (id: string) => void;
  };

  let {
    open = $bindable(false),
    asset = $bindable<MediaAsset | null>(null),
    mediaDisplay = {},
    onClose,
    onCreateMediaStream,
    onReplaceAsset,
    onRemoveAsset
  }: MediaDetailModalViewProps = $props();

  let renameBaseValue = $state('');
  let descriptionValue = $state('');
  let tagsValue = $state('');
  let metadataDirty = $state(false);
  let metadataBusy = $state(false);
  let attachLabelBusy = $state(false);
  let imageEdit = $state({ rotateDegrees: 0, crop: { x: 0, y: 0, width: 0, height: 0 } });
  let imageEditBusy = $state(false);
  let imageEditToastId = $state<string | null>(null);
  let videoEdit = $state({ startMs: 0, endMs: 0 });
  let manualLabelInput = $state<HTMLInputElement | null>(null);
  let labelPreview = $state<string | null>(null);
  let labelPreviewLoading = $state(false);
  let labelPreviewError = $state<string | null>(null);
  let dimensionHydrateSeq = 0;
  let previewVideoEl = $state<HTMLVideoElement | null>(null);
  let previewPlaybackMs = $state(0);
  let previewDurationMs = $state<number | null>(null);
  let previewPlaybackRaf: number | null = null;
  let previewVideoFrameCallbackId: number | null = null;
  let previewVideoFrameCallbackEl: HTMLVideoElement | null = null;
  let previewPlaybackAssetId = $state<string | null>(null);
  let createStreamBusy = $state(false);

  type VideoFrameCallbackCapableElement = HTMLVideoElement & {
    requestVideoFrameCallback?: (callback: (now: number, metadata: unknown) => void) => number;
    cancelVideoFrameCallback?: (handle: number) => void;
  };

  function closeDetailModal() {
    stopPreviewPlaybackTicker();
    open = false;
    onClose?.();
  }

  async function handleCreateStream(target: MediaAsset): Promise<void> {
    if (createStreamBusy || !onCreateMediaStream) return;
    createStreamBusy = true;
    try {
      await onCreateMediaStream(target);
    } catch (error) {
      reportError({ title: 'Media stream failed', error });
    } finally {
      createStreamBusy = false;
    }
  }

  function handleMetadataChange() {
    metadataDirty = true;
  }

  function syncImageEditFromAsset(current: MediaAsset | null) {
    if (!current || current.kind !== 'image') return;
    if (current.imageCrop) {
      imageEdit = {
        rotateDegrees: 0,
        crop: {
          x: current.imageCrop.x,
          y: current.imageCrop.y,
          width: current.imageCrop.width,
          height: current.imageCrop.height
        }
      };
      return;
    }
    resetImageEditToFullAsset(current);
  }

  function resetImageEditToFullAsset(current: MediaAsset) {
    if (current.kind !== 'image') return;
    imageEdit = {
      rotateDegrees: 0,
      crop: {
        x: 0,
        y: 0,
        width: current.width,
        height: current.height
      }
    };
  }

  function syncPreviewPlaybackMs() {
    if (!previewVideoEl) {
      previewPlaybackMs = 0;
      previewDurationMs = null;
      return;
    }
    const next = previewVideoEl.currentTime * 1000;
    const durationMs = previewVideoEl.duration * 1000;
    previewPlaybackMs = Number.isFinite(next) && next > 0 ? next : 0;
    previewDurationMs = Number.isFinite(durationMs) && durationMs > 0 ? durationMs : null;
  }

  function stopPreviewPlaybackTicker() {
    if (previewPlaybackRaf != null) {
      cancelAnimationFrame(previewPlaybackRaf);
      previewPlaybackRaf = null;
    }
    if (previewVideoFrameCallbackEl && previewVideoFrameCallbackId != null) {
      previewVideoFrameCallbackEl.cancelVideoFrameCallback?.(previewVideoFrameCallbackId);
      previewVideoFrameCallbackId = null;
      previewVideoFrameCallbackEl = null;
    }
  }

  function startPreviewPlaybackTicker() {
    if (previewPlaybackRaf != null || previewVideoFrameCallbackId != null) return;
    const frameCallbackVideo = previewVideoEl as VideoFrameCallbackCapableElement | null;
    if (frameCallbackVideo?.requestVideoFrameCallback) {
      const tick = () => {
        previewVideoFrameCallbackId = null;
        syncPreviewPlaybackMs();
        const currentVideo = previewVideoEl as VideoFrameCallbackCapableElement | null;
        if (!currentVideo || currentVideo.paused || currentVideo.ended || !currentVideo.requestVideoFrameCallback) {
          previewVideoFrameCallbackEl = null;
          return;
        }
        previewVideoFrameCallbackEl = currentVideo;
        previewVideoFrameCallbackId = currentVideo.requestVideoFrameCallback(() => tick());
      };
      previewVideoFrameCallbackEl = frameCallbackVideo;
      previewVideoFrameCallbackId = frameCallbackVideo.requestVideoFrameCallback(() => tick());
      return;
    }
    const tick = () => {
      previewPlaybackRaf = null;
      syncPreviewPlaybackMs();
      if (previewVideoEl && !previewVideoEl.paused && !previewVideoEl.ended) {
        previewPlaybackRaf = requestAnimationFrame(tick);
      }
    };
    previewPlaybackRaf = requestAnimationFrame(tick);
  }

  function handlePreviewPlaybackEvent() {
    syncPreviewPlaybackMs();
    if (previewVideoEl && !previewVideoEl.paused && !previewVideoEl.ended) {
      startPreviewPlaybackTicker();
    } else {
      stopPreviewPlaybackTicker();
    }
  }

  function handlePreviewLoadedMetadata() {
    if (previewVideoEl) {
      // Avoid stale browser media position carrying over between sessions.
      previewVideoEl.currentTime = 0;
    }
    previewPlaybackMs = 0;
    handlePreviewPlaybackEvent();
  }

  $effect(() => {
    if (open && asset) {
      const fields = hydrateMediaMetadata(asset);
      renameBaseValue = fields.renameBase;
      descriptionValue = fields.description;
      tagsValue = fields.tags;
      metadataDirty = false;
      if (asset.kind === 'image') {
        syncImageEditFromAsset(asset);
      }
      if (asset.kind === 'video' && asset.videoClip) {
        videoEdit = {
          startMs: asset.videoClip.start_ms,
          endMs: asset.videoClip.end_ms ?? 0
        };
      } else {
        videoEdit = { startMs: 0, endMs: 0 };
      }
      if (asset.kind === 'video') {
        if (previewPlaybackAssetId !== asset.id) {
          previewPlaybackMs = 0;
        }
        previewPlaybackAssetId = asset.id;
      } else {
        stopPreviewPlaybackTicker();
        previewVideoEl = null;
        previewPlaybackMs = 0;
        previewDurationMs = null;
        previewPlaybackAssetId = null;
      }
      if (asset.labelAttached) {
        void loadLabelPreview(asset.id);
      } else {
        labelPreview = null;
        labelPreviewError = null;
      }
      void ensureAssetDimensions(asset);
    } else {
      renameBaseValue = '';
      labelPreview = null;
      labelPreviewError = null;
      stopPreviewPlaybackTicker();
      previewVideoEl = null;
      previewPlaybackMs = 0;
      previewDurationMs = null;
      previewPlaybackAssetId = null;
    }
  });

  onDestroy(() => {
    stopPreviewPlaybackTicker();
  });

  async function persistMetadata() {
    if (!asset) return;
    metadataBusy = true;
    try {
      const updated = await saveMediaMetadata(asset, {
        renameBase: renameBaseValue,
        description: descriptionValue,
        tags: tagsValue
      });
      onReplaceAsset?.(updated);
      asset = updated;
      metadataDirty = false;
      toaster.success({ title: 'Saved changes' });
    } catch (error) {
      reportError({ title: 'Failed to save', error });
    } finally {
      metadataBusy = false;
    }
  }

  async function handleDelete(target: MediaAsset) {
    if (!confirm(`Delete "${target.name}"? This cannot be undone.`)) return;
    try {
      await deleteMediaAsset(target.id);
      toaster.success({ title: 'Asset deleted' });
      onRemoveAsset?.(target.id);
      if (asset?.id === target.id) {
        asset = null;
        open = false;
        onClose?.();
      }
    } catch (error) {
      reportError({ title: 'Delete failed', error });
    }
  }

  function triggerLabelAttach() {
    manualLabelInput?.click();
  }

  async function handleAttachLabel(event: Event) {
    if (!asset) return;
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    attachLabelBusy = true;
    try {
      const updated = await attachLabelFile(asset.id, file);
      onReplaceAsset?.(updated);
      asset = updated;
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

  async function ensureAssetDimensions(current: MediaAsset | null): Promise<void> {
    if (!current || (current.width > 0 && current.height > 0)) return;
    const requestId = ++dimensionHydrateSeq;
    const updated = await hydrateAssetDimensions(current);
    if (!updated || requestId !== dimensionHydrateSeq) return;
    onReplaceAsset?.(updated);
    if (asset?.id === current.id) {
      asset = updated;
    }
  }

  async function submitImageEdits() {
    if (!asset || asset.kind !== 'image' || imageEditBusy) return;
    imageEditBusy = true;
    const pendingToast = toaster.create({
      type: 'loading',
      title: 'Updating image',
      description: asset.name,
      duration: 600000,
      closable: false
    });
    imageEditToastId = pendingToast;
    try {
      const updated = await applyImageEdits(asset.id, {
        rotateDegrees: imageEdit.rotateDegrees,
        crop: imageEdit.crop
      });
      onReplaceAsset?.(updated);
      asset = updated;
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
    if (!asset || asset.kind !== 'video') return;
    try {
      const updated = await applyVideoEdits(asset.id, {
        startMs: videoEdit.startMs,
        endMs: videoEdit.endMs || undefined
      });
      onReplaceAsset?.(updated);
      asset = updated;
      toaster.success({ title: 'Clip updated' });
    } catch (error) {
      reportError({ title: 'Clip edit failed', error });
    }
  }

  export type $$Props = MediaDetailModalViewProps;
</script>

<MediaDetailModal open={open} asset={asset} onClose={closeDetailModal}>
  {#snippet header(current)}
    {@const assetExtension = getAssetExtension(current)}
    <p class="text-micro uppercase tracking-[0.3em] text-surface-500">{mediaKindLabel(current.kind)}</p>
    <div class="mt-2 flex items-center gap-2">
      <input class="input w-full" type="text" bind:value={renameBaseValue} oninput={handleMetadataChange} />
      {#if assetExtension}
        <span class="rounded border border-surface-800/60 bg-surface-900/40 px-2 py-1 text-xs text-surface-400">{assetExtension}</span>
      {/if}
    </div>
  {/snippet}
  {#snippet headerActions(current)}
    {#if current.kind === 'image' || current.kind === 'video'}
      <button
        class="btn btn-3xs preset-filled-primary-500 uppercase tracking-[0.3em]"
        type="button"
        onclick={() => void handleCreateStream(current)}
        disabled={createStreamBusy}
      >
        {createStreamBusy ? 'Starting…' : 'Create stream'}
      </button>
    {/if}
    <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={closeDetailModal}>
      Close
    </button>
    <button class="btn btn-3xs preset-outline text-error-200 uppercase tracking-[0.3em]" type="button" onclick={() => handleDelete(current)}>
      Delete
    </button>
  {/snippet}
  {#snippet primary(current)}
    {@const assetPreviewSrc = assetPreviewSource(current)}
    {@const assetPlaybackSrc = assetPlaybackSource(current)}
    {@const assetOriginalSrc = assetOriginalSource(current)}
    {@const assetMp4DownloadSrc = assetMp4DownloadSource(current)}
    <div class="space-y-3 min-w-0">
      <textarea class="input h-32 w-full" placeholder="Description" bind:value={descriptionValue} oninput={handleMetadataChange}></textarea>
      <input class="input w-full" type="text" placeholder="Tags (comma separated)" bind:value={tagsValue} oninput={handleMetadataChange} />
      <button class="btn btn-3xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={persistMetadata} disabled={!metadataDirty || metadataBusy}>
        {metadataBusy ? 'Saving…' : 'Save details'}
      </button>

      <div>
        {#if current.kind === 'image'}
          <div class="min-w-0 rounded border border-surface-800/60 bg-surface-900/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Preview</p>
            <div class="mt-2 w-full max-h-[min(60vh,42rem)] max-h-[min(60svh,42rem)] max-h-[min(60dvh,42rem)] min-w-0 overflow-auto rounded border border-surface-800/60 bg-surface-950/40">
              <img
                class="block h-auto w-full"
                src={assetPreviewSrc}
                alt={current.name}
                onerror={(event) => ((event.currentTarget as HTMLImageElement).src = assetOriginalSrc)}
              />
            </div>
            <a class="btn btn-3xs mt-3 preset-outline uppercase tracking-[0.3em]" href={assetOriginalSrc} target="_blank" rel="noreferrer">
              Download
            </a>
          </div>
        {:else if current.kind === 'video'}
          <div class="rounded border border-surface-800/60 bg-surface-900/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Preview</p>
            <video
              class="mt-2 w-full max-h-[min(60vh,42rem)] max-h-[min(60svh,42rem)] max-h-[min(60dvh,42rem)] rounded border border-surface-800/60"
              src={assetPlaybackSrc}
              controls
              playsinline
              bind:this={previewVideoEl}
              onloadedmetadata={handlePreviewLoadedMetadata}
              ontimeupdate={handlePreviewPlaybackEvent}
              onseeking={handlePreviewPlaybackEvent}
              onplay={handlePreviewPlaybackEvent}
              onpause={handlePreviewPlaybackEvent}
              onended={handlePreviewPlaybackEvent}
            >
              <track kind="captions" label="Captions" srclang="en" src="data:text/vtt," default />
            </video>
            {#if current.mp4DownloadUrl}
              <div class="mt-3 flex flex-wrap items-center gap-2">
                <a class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" href={assetOriginalSrc} target="_blank" rel="noreferrer">
                  Download raw
                </a>
                <a class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" href={assetMp4DownloadSrc} target="_blank" rel="noreferrer">
                  Download MP4
                </a>
              </div>
            {:else}
              <a class="btn btn-3xs mt-3 preset-outline uppercase tracking-[0.3em]" href={assetOriginalSrc} target="_blank" rel="noreferrer">
                Download
              </a>
            {/if}
          </div>
        {:else if current.kind === 'model'}
          <div class="rounded border border-surface-800/60 bg-surface-900/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Model file</p>
            <a class="btn btn-3xs mt-3 preset-outline uppercase tracking-[0.3em]" href={assetOriginalSrc} target="_blank" rel="noreferrer">
              Download model
            </a>
          </div>
        {:else}
          <div class="rounded border border-surface-800/60 bg-surface-900/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">{mediaKindLabel(current.kind)}</p>
            <a class="btn btn-3xs mt-3 preset-outline uppercase tracking-[0.3em]" href={assetOriginalSrc} target="_blank" rel="noreferrer">
              Download
            </a>
          </div>
        {/if}
      </div>
    </div>
  {/snippet}
  {#snippet secondary(current)}
    {@const assetOriginalSrc = assetOriginalSource(current)}
    {#if current.kind === 'image'}
      <div class="min-w-0 rounded border border-surface-800/60 bg-surface-900/40 p-3 space-y-3 h-full flex flex-col">
        <div class="flex items-center justify-between gap-3">
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Image edits</p>
          <button class="btn btn-3xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={submitImageEdits} disabled={imageEditBusy}>
            {imageEditBusy ? 'Working…' : 'Apply edits'}
          </button>
        </div>
        <ImageCropper
          src={assetOriginalSrc}
          naturalWidth={current.width}
          naturalHeight={current.height}
          crop={imageEdit.crop}
          rotateDegrees={imageEdit.rotateDegrees}
          on:cropChange={(event) => (imageEdit.crop = event.detail)}
          on:rotateChange={(event) => (imageEdit.rotateDegrees = event.detail)}
        />
        <p class="text-xs text-surface-500">Drag the handles to adjust the crop or use the rotate buttons, then apply your changes.</p>
        {#if imageEditBusy}
          <div class="flex items-center gap-2 text-xs text-surface-500">
            <span class="inline-block h-3 w-3 animate-spin rounded-full border-2 border-surface-500 border-t-transparent"></span>
            <span>Applying edits…</span>
          </div>
        {/if}
      </div>
    {:else if current.kind === 'video'}
      <div class="rounded border border-surface-800/60 bg-surface-900/40 p-3 space-y-3 flex min-h-[min(60vh,42rem)] flex-col">
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Clip range (ms)</p>
        <div class="grid grid-cols-2 gap-2">
          <input class="input" type="number" placeholder="Start" value={videoEdit.startMs} oninput={(event) => (videoEdit.startMs = Number((event.target as HTMLInputElement).value))} />
          <input class="input" type="number" placeholder="End" value={videoEdit.endMs} oninput={(event) => (videoEdit.endMs = Number((event.target as HTMLInputElement).value))} />
        </div>
        <button class="btn btn-3xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={submitVideoEdits}>
          Save clip
        </button>
        <MediaImuPlaybackPanel
          imuDataUrl={current.imuDataUrl ?? null}
          frameTimestampsUrl={current.frameTimestampsUrl ?? null}
          imuDataSamples={current.imuDataSamples ?? null}
          playbackTimeMs={previewPlaybackMs}
          playbackDurationMs={previewDurationMs}
        />
      </div>
    {:else if current.kind === 'model'}
      <div class="rounded border border-surface-800/60 bg-surface-900/40 p-3 space-y-2">
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Model labels</p>
        <p class="text-xs text-surface-500">{current.labelAttached ? 'Label attached to model' : 'No label file attached yet.'}</p>
        <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={triggerLabelAttach} disabled={attachLabelBusy}>
          {attachLabelBusy ? 'Uploading…' : current.labelAttached ? 'Replace label file' : 'Attach label file'}
        </button>
        <input class="sr-only" bind:this={manualLabelInput} type="file" accept=".json,.txt" onchange={handleAttachLabel} />
        {#if current.labelAttached}
          <div class="rounded border border-surface-800/60 bg-surface-950/30 p-2 text-xs text-surface-400">
            {#if labelPreviewLoading}
              <p>Loading label contents…</p>
            {:else if labelPreviewError}
              <p class="text-error-300">{labelPreviewError}</p>
            {:else if labelPreview}
              <pre class="max-h-48 overflow-auto whitespace-pre-wrap text-surface-100">{labelPreview}</pre>
            {:else}
              <p>No label preview available.</p>
            {/if}
          </div>
        {/if}
      </div>
    {:else}
      <div class="rounded border border-surface-800/60 bg-surface-900/40 p-3 space-y-2">
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">No extra actions</p>
        <p class="text-xs text-surface-500">This file type does not support edits or labels.</p>
      </div>
    {/if}
  {/snippet}
  {#snippet details(current)}
    <div class="rounded border border-surface-800/60 bg-surface-900/40 p-3 text-xs text-surface-500 space-y-1">
      {#if current.kind === 'image'}
        <p>Resolution: {current.width > 0 && current.height > 0 ? `${current.width}×${current.height}` : 'Unknown'}</p>
        <p>Type: {current.primaryMime ?? (current.primaryExtension ? current.primaryExtension.toUpperCase() : 'Unknown')}</p>
      {:else if current.kind === 'video'}
        <p>Resolution: {current.width > 0 && current.height > 0 ? `${current.width}×${current.height}` : 'Unknown'}</p>
        <p>Frame rate: {current.fps > 0 ? `${current.fps.toFixed(2)} fps` : 'Unknown'}</p>
        <p>Encoded: {formatVideoCodec(current.videoCodec)}</p>
        <p>
          IMU sidecar:
          {#if current.imuDataFileName}
            <span class="text-surface-200">{current.imuDataSamples != null ? `${current.imuDataSamples} samples` : 'available'}</span>
          {:else}
            <span class="text-surface-400">none</span>
          {/if}
        </p>
    {:else if current.kind === 'model'}
        {@const affinity = deriveModelAffinity(current.tags)}
        {@const tensorDetails = parseTensorSpec(current.modelTensorSpec)}
        <p>Format: {current.primaryExtension ? current.primaryExtension.toUpperCase() : current.primaryMime ?? 'Unknown'}</p>
        {#if affinity.runtime || affinity.precision || affinity.quantized}
          <p>
            Affinity:
            <span class="text-surface-200">
              {affinity.runtime ?? 'Unknown'}
              {affinity.precision ? ` · ${affinity.precision}` : ''}
              {affinity.quantized ? ' · Quantized' : ''}
            </span>
          </p>
        {/if}
        <p>Input resolution: {current.modelInputResolution ?? 'Unknown'}</p>
        {#if tensorDetails.inputs.length || tensorDetails.outputs.length}
          <div class="space-y-1">
            {#if tensorDetails.inputs.length}
              <div>
                <p class="uppercase tracking-[0.3em] text-micro-tight text-surface-500">Inputs</p>
                <ul class="ml-4 list-none space-y-0.5 text-surface-400">
                  {#each tensorDetails.inputs as entry (entry)}
                    {@const tensor = describeTensorEntry(entry)}
                    <li>
                      <span class="font-semibold text-surface-200">{tensor.name}</span>
                      {#if tensor.shape}
                        <span class="text-surface-500"> — {tensor.shape}</span>
                      {/if}
                    </li>
                  {/each}
                </ul>
              </div>
            {/if}
            {#if tensorDetails.outputs.length}
              <div>
                <p class="uppercase tracking-[0.3em] text-micro-tight text-surface-500">Outputs</p>
                <ul class="ml-4 list-none space-y-0.5 text-surface-400">
                  {#each tensorDetails.outputs as entry (entry)}
                    {@const tensor = describeTensorEntry(entry)}
                    <li>
                      <span class="font-semibold text-surface-200">{tensor.name}</span>
                      {#if tensor.shape}
                        <span class="text-surface-500"> — {tensor.shape}</span>
                      {/if}
                    </li>
                  {/each}
                </ul>
              </div>
            {/if}
          </div>
        {:else}
          <p>Tensors: {current.modelTensorSpec ?? 'Unknown'}</p>
        {/if}
        <p>Label file: {current.labelFileName ?? 'Not attached'}</p>
      {:else}
        <p>Format: {current.primaryExtension ? current.primaryExtension.toUpperCase() : current.primaryMime ?? 'Unknown'}</p>
        <p>Type: {current.primaryMime ?? 'Unknown'}</p>
      {/if}
      <p>Size: {mediaDisplay[current.id]?.sizeLabel ?? formatBytes(current.sizeBytes)}</p>
    </div>
  {/snippet}
</MediaDetailModal>
