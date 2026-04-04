<script lang="ts">
  import { MEDIA_KIND_OPTIONS, mediaKindLabel } from '$lib/features/media/mediaKind';
  import type { MediaAsset } from '$lib/features/media/api';

  let {
    open = false,
    mediaRoot,
    selectedCount,
    mediaPickerQuery,
    mediaPickerSort,
    mediaPickerKind,
    mediaPickerLoading,
    mediaPickerError,
    mediaPickerAssets,
    mediaPickerSelected,
    onClose,
    onApplySelection,
    onRefresh,
    onClearSelection,
    onSelectAll,
    onQueryChange,
    onSortChange,
    onKindClick,
    onItemClick,
    onToggleSelection
  }: {
    open: boolean;
    mediaRoot: string;
    selectedCount: number;
    mediaPickerQuery: string;
    mediaPickerSort: 'name' | 'recent';
    mediaPickerKind: 'all' | MediaAsset['kind'];
    mediaPickerLoading: boolean;
    mediaPickerError: string | null;
    mediaPickerAssets: MediaAsset[];
    mediaPickerSelected: Record<string, boolean>;
    onClose: () => void;
    onApplySelection: () => void;
    onRefresh: () => void;
    onClearSelection: () => void;
    onSelectAll: () => void;
    onQueryChange: (value: string) => void;
    onSortChange: (event: Event) => void;
    onKindClick: (option: string) => void;
    onItemClick: (asset: MediaAsset, event: MouseEvent) => void;
    onToggleSelection: (name: string) => void;
  } = $props();

  function mediaPickerKindLabel(option: string): string {
    return option === 'all' ? 'All kinds' : mediaKindLabel(option as MediaAsset['kind']);
  }
</script>

{#if open}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-surface-950/70 px-4"
    role="dialog"
    aria-modal="true"
    onclick={(event) => {
      if (event.target === event.currentTarget) onClose();
    }}
    onkeydown={(event) => {
      if (event.key === 'Escape') onClose();
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') {
        event.preventDefault();
        onSelectAll();
      }
    }}
    tabindex="-1"
  >
    <div class="w-full max-w-5xl rounded border border-surface-800/70 bg-surface-950/95 p-6 text-sm text-surface-400 shadow-2xl max-h-[90vh] max-h-[90svh] max-h-[90dvh] overflow-y-auto">
      <div class="flex flex-wrap items-center justify-between gap-4">
        <div>
          <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Media library</p>
          <p class="text-sm text-surface-300">{selectedCount} selected</p>
        </div>
        <div class="flex gap-2">
          <button class="btn btn-2xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onClose}>
            Cancel
          </button>
          <button class="btn btn-2xs preset-filled-primary-500 uppercase tracking-[0.3em]" type="button" onclick={onApplySelection}>
            Apply selection
          </button>
        </div>
      </div>

      <div class="mt-4 flex flex-wrap gap-3">
        <input
          class="input flex-1 min-w-[220px]"
          type="search"
          placeholder="Search media"
          value={mediaPickerQuery}
          oninput={(e) => onQueryChange(e.currentTarget.value)}
        />
        <button class="btn btn-2xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={onRefresh}>
          Refresh
        </button>
        <select
          class="input w-40"
          value={mediaPickerSort}
          onchange={onSortChange}
        >
          <option value="name">Sort: Name</option>
          <option value="recent">Sort: Recent</option>
        </select>
        <button class="btn btn-2xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onClearSelection}>
          Clear
        </button>
        <button class="btn btn-2xs preset-tonal uppercase tracking-[0.3em]" type="button" onclick={onSelectAll}>
          Select all
        </button>
      </div>

      <div class="mt-3 flex flex-wrap gap-2">
        {#each ['all', ...MEDIA_KIND_OPTIONS] as option (option)}
          <button
            class={`btn btn-2xs uppercase tracking-[0.3em] ${
              mediaPickerKind === option ? 'preset-filled-primary-500' : 'preset-tonal'
            }`}
            type="button"
            onclick={() => onKindClick(option)}
          >
            {mediaPickerKindLabel(option)}
          </button>
        {/each}
      </div>

      {#if mediaPickerLoading}
        <div class="mt-6 text-sm text-surface-500">Loading media…</div>
      {:else if mediaPickerError}
        <div class="mt-6 text-sm text-error-200">{mediaPickerError}</div>
      {:else if !mediaPickerAssets.length}
        <div class="mt-6 text-sm text-surface-500">No media files found.</div>
      {:else}
        <div class="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {#each mediaPickerAssets as asset (asset.id)}
            {@const isSelected = Boolean(mediaPickerSelected[asset.name])}
            <button
              class={`flex items-center gap-3 rounded border px-3 py-2 text-left transition ${
                isSelected ? 'border-primary-400 bg-primary-500/10' : 'border-surface-800/60 bg-surface-950/40 hover:border-primary-400/60'
              }`}
              type="button"
              onclick={(event) => onItemClick(asset, event)}
            >
              {#if asset.previewUrl}
                <img class="h-12 w-16 rounded object-cover" src={asset.previewUrl} alt={asset.name} loading="lazy" />
              {:else}
                <div class="flex h-12 w-16 items-center justify-center rounded bg-surface-900 text-micro-tight uppercase tracking-[0.2em] text-surface-400">
                  {mediaKindLabel(asset.kind)}
                </div>
              {/if}
              <div class="min-w-0 flex-1">
                <p class="truncate text-sm text-surface-200">{asset.name}</p>
                <p class="text-xs text-surface-500">{mediaKindLabel(asset.kind)}</p>
              </div>
              <input
                type="checkbox"
                checked={isSelected}
                aria-label={isSelected ? 'Deselect media' : 'Select media'}
                onclick={(e) => e.stopPropagation()}
                onchange={() => onToggleSelection(asset.name)}
              />
            </button>
          {/each}
        </div>
      {/if}
      <p class="mt-4 text-xs text-surface-500">Selected files map to `{mediaRoot}/&lt;name&gt;` when the stream starts.</p>
    </div>
  </div>
{/if}
