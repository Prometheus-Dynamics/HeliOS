<script lang="ts">
  import type { MediaItem, UploadUpdateResponse } from '$lib/ts-bindings/http/client';

  type ImageSourceKind = 'upload' | 'media' | 'url';

  type UpdaterSourceSelectorProps = {
    sourceKind: ImageSourceKind;
    imageSourceLabel: string;
    uploadTarget: File | null;
    uploadBusy: boolean;
    uploadError: string | null;
    uploadStatus: string | null;
    uploadInfo: UploadUpdateResponse | null;
    mediaLoading: boolean;
    mediaError: string | null;
    filteredMedia: MediaItem[];
    selectedMedia: string;
    selectedMediaUrl: string;
    mediaDeleteBusy: boolean;
    imageUrlOverride: string;
    onSourceKindChange: (kind: ImageSourceKind) => void;
    onFileChange: (event: Event) => void;
    onUploadImage: () => void;
    onRefreshMedia: () => void;
    onDeleteMedia: (name: string) => void;
    onSelectMedia: (name: string) => void;
    onImageUrlChange: (value: string) => void;
  };

  const {
    sourceKind,
    imageSourceLabel,
    uploadTarget,
    uploadBusy,
    uploadError,
    uploadStatus,
    uploadInfo,
    mediaLoading,
    mediaError,
    filteredMedia,
    selectedMedia,
    selectedMediaUrl,
    mediaDeleteBusy,
    imageUrlOverride,
    onSourceKindChange,
    onFileChange,
    onUploadImage,
    onRefreshMedia,
    onDeleteMedia,
    onSelectMedia,
    onImageUrlChange
  }: UpdaterSourceSelectorProps = $props();

  function handleSelectedMediaChange(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLSelectElement)) {
      return;
    }
    onSelectMedia(target.value);
  }

  function handleImageUrlInput(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement)) {
      return;
    }
    onImageUrlChange(target.value);
  }

  export type $$Props = UpdaterSourceSelectorProps;
</script>

<section class="space-y-3 border border-surface-700/60 bg-surface-950/35 p-4">
  <header class="flex flex-wrap items-center justify-between gap-3">
    <div>
      <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Select update image</p>
      <p class="text-xs text-surface-500">Choose exactly one source, then apply to start the update.</p>
    </div>
    <div class="flex items-center gap-2 text-xs text-surface-400">
      <span class="uppercase tracking-[0.3em]">Source</span>
      <span class="text-surface-300">{imageSourceLabel}</span>
    </div>
  </header>

  <div class="grid gap-3 lg:grid-cols-3">
    <div class="space-y-2 border border-surface-700/60 bg-surface-900/30 p-3">
      <label class="flex items-center gap-2 text-xs uppercase tracking-[0.3em] text-surface-500">
        <input
          type="radio"
          name="sourceKind"
          value="upload"
          checked={sourceKind === 'upload'}
          onchange={() => onSourceKindChange('upload')}
        />
        Upload
      </label>
      <input class="input w-full" type="file" accept=".img,.tar.gz,.zip,.bin,.xz" onchange={onFileChange} />
      <div class="flex flex-wrap items-center gap-2 text-xs text-surface-400">
        <span>{uploadTarget ? uploadTarget.name : uploadInfo ? uploadInfo.filename : 'No file selected.'}</span>
        {#if uploadError}<span class="text-error-400">{uploadError}</span>{/if}
        {#if uploadStatus}<span class="text-success-400">{uploadStatus}</span>{/if}
      </div>
      <button
        class="btn preset-filled-primary-500 w-full"
        type="button"
        onclick={onUploadImage}
        disabled={uploadBusy || !uploadTarget}
      >
        {uploadBusy ? 'Uploading…' : uploadInfo ? 'Upload a different image' : 'Upload image'}
      </button>
      {#if uploadInfo}
        <div class="flex items-center justify-between gap-2 text-[0.7rem] text-surface-500">
          <span class="break-all">SHA256 {uploadInfo.sha256.slice(0, 16)}…</span>
          <button
            class="btn btn-2xs btn-ghost"
            type="button"
            onclick={() => onDeleteMedia(uploadInfo.filename)}
            disabled={mediaDeleteBusy}
          >
            {mediaDeleteBusy ? 'Removing…' : 'Remove'}
          </button>
        </div>
      {/if}
    </div>

    <div class="space-y-2 border border-surface-700/60 bg-surface-900/30 p-3">
      <div class="flex items-center justify-between gap-2">
        <label class="flex items-center gap-2 text-xs uppercase tracking-[0.3em] text-surface-500">
          <input
            type="radio"
            name="sourceKind"
            value="media"
            checked={sourceKind === 'media'}
            onchange={() => onSourceKindChange('media')}
          />
          Media
        </label>
        <div class="flex items-center gap-2">
          <button class="btn btn-ghost text-xs" type="button" onclick={onRefreshMedia} disabled={mediaLoading}>
            {mediaLoading ? 'Refreshing…' : 'Refresh'}
          </button>
          <button
            class="btn btn-ghost text-xs"
            type="button"
            onclick={() => onDeleteMedia(selectedMedia)}
            disabled={!selectedMedia || mediaDeleteBusy}
          >
            {mediaDeleteBusy ? 'Removing…' : 'Remove'}
          </button>
        </div>
      </div>
      {#if mediaError}
        <p class="text-xs text-error-400">{mediaError}</p>
      {:else if filteredMedia.length === 0}
        <p class="text-xs text-surface-500">No compatible media files found.</p>
      {:else}
        <select
          class="input w-full"
          value={selectedMedia}
          onchange={handleSelectedMediaChange}
        >
          <option value="">Choose uploaded image…</option>
          {#each filteredMedia as item (item.name)}
            <option value={item.name}>{item.name} ({item.size_bytes?.toLocaleString?.() ?? 0} bytes)</option>
          {/each}
        </select>
      {/if}
      <p class="text-[0.7rem] text-surface-500 break-all">{selectedMediaUrl || '—'}</p>
    </div>

    <div class="space-y-2 border border-surface-700/60 bg-surface-900/30 p-3">
      <label class="flex items-center gap-2 text-xs uppercase tracking-[0.3em] text-surface-500">
        <input
          type="radio"
          name="sourceKind"
          value="url"
          checked={sourceKind === 'url'}
          onchange={() => onSourceKindChange('url')}
        />
        URL
      </label>
      <input
        class="input w-full"
        placeholder="https://… or file:///…"
        value={imageUrlOverride}
        oninput={handleImageUrlInput}
      />
      <p class="text-[0.7rem] text-surface-500 break-all">{imageUrlOverride.trim() || '—'}</p>
    </div>
  </div>
</section>
