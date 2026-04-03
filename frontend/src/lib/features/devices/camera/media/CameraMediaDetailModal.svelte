<script lang="ts">
  import { base } from '$app/paths';
  import ImageCropper from '$lib/components/media/ImageCropper.svelte';
  import MediaDetailModal from '$lib/components/media/MediaDetailModal.svelte';
  import { describeTensorEntry } from '$lib/features/media/assetHelpers';
  import type { MediaAsset } from '$lib/features/media/api';
  import {
    assetPlaybackSource,
    assetMp4DownloadSource,
    assetOriginalSource,
    assetPreviewSource,
    formatVideoCodec,
    formatBytes,
    getAssetExtension,
    parseTensorSpec
  } from '$lib/features/media/utils';
  import { mediaKindLabel } from '$lib/features/media/mediaKind';

  type ImageEdit = { rotateDegrees: number; crop: { x: number; y: number; width: number; height: number } };
  type VideoEdit = { startMs: number; endMs: number };

  type Props = {
    open: boolean;
    asset: MediaAsset | null;
    renameBaseValue: string;
    descriptionValue: string;
    tagsValue: string;
    metadataDirty: boolean;
    metadataBusy: boolean;
    imageEdit: ImageEdit;
    imageEditBusy: boolean;
    videoEdit: VideoEdit;
    attachLabelBusy: boolean;
    labelPreview: string | null;
    labelPreviewLoading: boolean;
    labelPreviewError: string | null;
    onClose: () => void;
    onDelete: (asset: MediaAsset) => void;
    onSaveMetadata: () => void;
    onSubmitImageEdits: () => void;
    onSubmitVideoEdits: () => void;
    onAttachLabel: (event: Event) => void;
    onMetadataChange: () => void;
  };

  let {
    open,
    asset,
    renameBaseValue = $bindable(),
    descriptionValue = $bindable(),
    tagsValue = $bindable(),
    metadataDirty,
    metadataBusy,
    imageEdit = $bindable(),
    imageEditBusy,
    videoEdit = $bindable(),
    attachLabelBusy,
    labelPreview,
    labelPreviewLoading,
    labelPreviewError,
    onClose,
    onDelete,
    onSaveMetadata,
    onSubmitImageEdits,
    onSubmitVideoEdits,
    onAttachLabel,
    onMetadataChange
  }: Props = $props();

  let manualLabelInput: HTMLInputElement | null = $state(null);

  function triggerLabelAttach() {
    manualLabelInput?.click();
  }

  function handleCropChange(event: CustomEvent<{ x: number; y: number; width: number; height: number }>) {
    imageEdit = { ...imageEdit, crop: event.detail };
  }

  function handleRotateChange(event: CustomEvent<number>) {
    imageEdit = { ...imageEdit, rotateDegrees: event.detail };
  }

  function handleImagePreviewError(event: Event, fallbackSrc: string): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLImageElement)) return;
    target.src = fallbackSrc;
  }

  function handleVideoStartChange(value: string) {
    videoEdit = { ...videoEdit, startMs: Number(value) };
  }

  function handleVideoEndChange(value: string) {
    videoEdit = { ...videoEdit, endMs: Number(value) };
  }

  function assetHref(path: string): string {
    return path.startsWith('/') ? `${base}${path}` : path;
  }

  function openAsset(path: string): void {
    if (typeof window === 'undefined') return;
    window.open(assetHref(path), '_blank', 'noopener,noreferrer');
  }
</script>

<MediaDetailModal {open} {asset} onClose={onClose}>
  {#snippet header(asset)}
    {@const assetExtension = getAssetExtension(asset)}
    <p class="text-micro uppercase tracking-[0.3em] text-surface-500">{mediaKindLabel(asset.kind)}</p>
    <div class="mt-2 flex items-center gap-2">
      <input class="input w-full" type="text" bind:value={renameBaseValue} oninput={onMetadataChange} />
      {#if assetExtension}
        <span class="rounded border border-surface-800/60 bg-surface-900/40 px-2 py-1 text-xs text-surface-400">{assetExtension}</span>
      {/if}
    </div>
  {/snippet}
  {#snippet headerActions(asset)}
    <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onClose}>
      Close
    </button>
    <button class="btn btn-3xs preset-outline text-error-200 uppercase tracking-[0.3em]" type="button" onclick={() => onDelete(asset)}>
      Delete
    </button>
  {/snippet}
  {#snippet primary(asset)}
    {@const assetPreviewSrc = assetPreviewSource(asset)}
    {@const assetPlaybackSrc = assetPlaybackSource(asset)}
    {@const assetOriginalSrc = assetOriginalSource(asset)}
    {@const assetMp4DownloadSrc = assetMp4DownloadSource(asset)}
    <div class="space-y-3 min-w-0">
      <textarea
        class="input h-32 w-full"
        placeholder="Description"
        bind:value={descriptionValue}
        oninput={onMetadataChange}
      ></textarea>
      <input
        class="input w-full"
        type="text"
        placeholder="Tags (comma separated)"
        bind:value={tagsValue}
        oninput={onMetadataChange}
      />
      <button
        class="btn btn-3xs preset-filled-primary-500 uppercase tracking-[0.3em]"
        type="button"
        onclick={onSaveMetadata}
        disabled={!metadataDirty || metadataBusy}
      >
        {metadataBusy ? 'Saving…' : 'Save details'}
      </button>

      <div>
        {#if asset.kind === 'image'}
          <div class="min-w-0 rounded border border-surface-800/60 bg-surface-900/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Preview</p>
            <div class="mt-2 w-full max-h-[min(60vh,42rem)] max-h-[min(60svh,42rem)] max-h-[min(60dvh,42rem)] min-w-0 overflow-auto rounded border border-surface-800/60 bg-surface-950/40">
              <img
                class="block h-auto w-full"
                src={assetPreviewSrc}
                alt={asset.name}
                onerror={(event) => handleImagePreviewError(event, assetOriginalSrc)}
              />
            </div>
            <button class="btn btn-3xs mt-3 preset-outline uppercase tracking-[0.3em]" type="button" onclick={() => openAsset(assetOriginalSrc)}>
              Download
            </button>
          </div>
        {:else if asset.kind === 'video'}
          <div class="rounded border border-surface-800/60 bg-surface-900/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Preview</p>
            <video class="mt-2 w-full max-h-[min(60vh,42rem)] max-h-[min(60svh,42rem)] max-h-[min(60dvh,42rem)] rounded border border-surface-800/60" src={assetPlaybackSrc} controls playsinline>
              <track kind="captions" label="Captions" srclang="en" src="data:text/vtt," default />
            </video>
            {#if asset.mp4DownloadUrl}
              <div class="mt-3 flex flex-wrap items-center gap-2">
                <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={() => openAsset(assetOriginalSrc)}>
                  Download raw
                </button>
                <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={() => openAsset(assetMp4DownloadSrc)}>
                  Download MP4
                </button>
              </div>
            {:else}
              <button class="btn btn-3xs mt-3 preset-outline uppercase tracking-[0.3em]" type="button" onclick={() => openAsset(assetOriginalSrc)}>
                Download
              </button>
            {/if}
          </div>
        {:else if asset.kind === 'model'}
          <div class="rounded border border-surface-800/60 bg-surface-900/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Model file</p>
            <button class="btn btn-3xs mt-3 preset-outline uppercase tracking-[0.3em]" type="button" onclick={() => openAsset(assetOriginalSrc)}>
              Download model
            </button>
          </div>
        {:else}
          <div class="rounded border border-surface-800/60 bg-surface-900/40 p-3">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">{mediaKindLabel(asset.kind)}</p>
            <button class="btn btn-3xs mt-3 preset-outline uppercase tracking-[0.3em]" type="button" onclick={() => openAsset(assetOriginalSrc)}>
              Download
            </button>
          </div>
        {/if}
      </div>
    </div>
  {/snippet}
  {#snippet secondary(asset)}
    {@const assetOriginalSrc = assetOriginalSource(asset)}
    {#if asset.kind === 'image'}
      <div class="min-w-0 rounded border border-surface-800/60 bg-surface-900/40 p-3 space-y-3 h-full flex flex-col">
        <div class="flex items-center justify-between gap-3">
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Image edits</p>
          <button class="btn btn-3xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={onSubmitImageEdits} disabled={imageEditBusy}>
            {imageEditBusy ? 'Working…' : 'Apply edits'}
          </button>
        </div>
        <ImageCropper
          src={assetOriginalSrc}
          naturalWidth={asset.width}
          naturalHeight={asset.height}
          crop={imageEdit.crop}
          rotateDegrees={imageEdit.rotateDegrees}
          on:cropChange={handleCropChange}
          on:rotateChange={handleRotateChange}
        />
        <p class="text-xs text-surface-500">Drag the handles to adjust the crop or use the rotate buttons, then apply your changes.</p>
        {#if imageEditBusy}
          <div class="flex items-center gap-2 text-xs text-surface-500">
            <span class="inline-block h-3 w-3 animate-spin rounded-full border-2 border-surface-500 border-t-transparent"></span>
            <span>Applying edits…</span>
          </div>
        {/if}
      </div>
    {:else if asset.kind === 'video'}
      <div class="rounded border border-surface-800/60 bg-surface-900/40 p-3 space-y-2">
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Clip range (ms)</p>
        <div class="grid grid-cols-2 gap-2">
          <input class="input" type="number" placeholder="Start" value={videoEdit.startMs} oninput={(event) => handleVideoStartChange(event.currentTarget.value)} />
          <input class="input" type="number" placeholder="End" value={videoEdit.endMs} oninput={(event) => handleVideoEndChange(event.currentTarget.value)} />
        </div>
        <button class="btn btn-3xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={onSubmitVideoEdits}>
          Save clip
        </button>
      </div>
    {:else if asset.kind === 'model'}
      <div class="rounded border border-surface-800/60 bg-surface-900/40 p-3 space-y-2">
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Model labels</p>
        <p class="text-xs text-surface-500">{asset.labelAttached ? 'Label attached to model' : 'No label file attached yet.'}</p>
        <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={triggerLabelAttach} disabled={attachLabelBusy}>
          {attachLabelBusy ? 'Uploading…' : asset.labelAttached ? 'Replace label file' : 'Attach label file'}
        </button>
        <input class="sr-only" bind:this={manualLabelInput} type="file" accept=".json,.txt" onchange={onAttachLabel} />
        {#if asset.labelAttached}
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
  {#snippet details(asset)}
    <div class="rounded border border-surface-800/60 bg-surface-900/40 p-3 text-xs text-surface-500 space-y-1">
      {#if asset.kind === 'image'}
        <p>Resolution: {asset.width > 0 && asset.height > 0 ? `${asset.width}x${asset.height}` : 'Unknown'}</p>
        <p>Type: {asset.primaryMime ?? (asset.primaryExtension ? asset.primaryExtension.toUpperCase() : 'Unknown')}</p>
      {:else if asset.kind === 'video'}
        <p>Resolution: {asset.width > 0 && asset.height > 0 ? `${asset.width}x${asset.height}` : 'Unknown'}</p>
        <p>Frame rate: {asset.fps > 0 ? `${asset.fps.toFixed(2)} fps` : 'Unknown'}</p>
        <p>Encoded: {formatVideoCodec(asset.videoCodec)}</p>
        <p>
          IMU sidecar:
          {#if asset.imuDataFileName}
            <span class="text-surface-200">{asset.imuDataSamples != null ? `${asset.imuDataSamples} samples` : 'available'}</span>
          {:else}
            <span class="text-surface-400">none</span>
          {/if}
        </p>
      {:else if asset.kind === 'model'}
        {@const tensorDetails = parseTensorSpec(asset.modelTensorSpec)}
        <p>Format: {asset.primaryExtension ? asset.primaryExtension.toUpperCase() : asset.primaryMime ?? 'Unknown'}</p>
        <p>Input resolution: {asset.modelInputResolution ?? 'Unknown'}</p>
        {#if tensorDetails.inputs.length || tensorDetails.outputs.length}
          <div class="space-y-1">
            {#if tensorDetails.inputs.length}
              <div>
                <p class="uppercase tracking-[0.3em] text-micro-tight text-surface-500">Inputs</p>
                <ul class="ml-4 list-none space-y-0.5 text-surface-400">
                  {#each tensorDetails.inputs as entry (`input:${entry}`)}
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
                  {#each tensorDetails.outputs as entry (`output:${entry}`)}
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
          <p>Tensors: {asset.modelTensorSpec ?? 'Unknown'}</p>
        {/if}
        <p>Label file: {asset.labelFileName ?? 'Not attached'}</p>
      {:else}
        <p>Format: {asset.primaryExtension ? asset.primaryExtension.toUpperCase() : asset.primaryMime ?? 'Unknown'}</p>
        <p>Type: {asset.primaryMime ?? 'Unknown'}</p>
      {/if}
      <p>Size: {formatBytes(asset.sizeBytes)}</p>
    </div>
  {/snippet}
</MediaDetailModal>
