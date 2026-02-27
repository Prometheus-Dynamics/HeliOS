<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { MediaAsset } from '$lib/features/media/api';
  import { mediaKindLabel } from '$lib/features/media/mediaKind';

  type MediaDetailModalProps = {
    open: boolean;
    asset: MediaAsset | null;
    title?: string;
    subtitle?: string;
    maxWidthClass?: string;
    className?: string;
    closeOnBackdrop?: boolean;
    closeOnEscape?: boolean;
    onClose?: () => void;
    header?: Snippet<[MediaAsset]>;
    headerActions?: Snippet<[MediaAsset]>;
    primary?: Snippet<[MediaAsset]>;
    secondary?: Snippet<[MediaAsset]>;
    details?: Snippet<[MediaAsset]>;
    footer?: Snippet<[MediaAsset]>;
  };

  const {
    open,
    asset,
    title,
    subtitle,
    maxWidthClass = 'max-w-5xl',
    className = '',
    closeOnBackdrop = true,
    closeOnEscape = true,
    onClose,
    header,
    headerActions,
    primary,
    secondary,
    details,
    footer
  }: MediaDetailModalProps = $props();

  function handleBackdropClick(event: MouseEvent) {
    if (!closeOnBackdrop) return;
    if (event.target === event.currentTarget) {
      onClose?.();
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!closeOnEscape) return;
    if (event.key === 'Escape') {
      onClose?.();
    }
  }

  export type $$Props = MediaDetailModalProps;
</script>

{#if open && asset}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-surface-950/70 px-4"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    onclick={handleBackdropClick}
    onkeydown={handleKeydown}
  >
    <div class={`relative w-full ${maxWidthClass} rounded border border-surface-800/70 bg-surface-950/95 p-6 text-sm text-surface-400 shadow-2xl max-h-[90vh] max-h-[90svh] max-h-[90dvh] overflow-y-auto ${className}`.trim()}>
      <div class="flex items-start justify-between gap-4">
        <div class="flex-1">
          {#if header}
            {@render header(asset)}
          {:else}
            {#if title || subtitle}
              <p class="text-micro uppercase tracking-[0.3em] text-surface-500">{subtitle ?? ''}</p>
              <p class="mt-2 text-lg font-semibold text-surface-100">{title}</p>
            {:else}
              <p class="text-micro uppercase tracking-[0.3em] text-surface-500">{mediaKindLabel(asset.kind)}</p>
              <p class="mt-2 text-lg font-semibold text-surface-100">{asset.name}</p>
            {/if}
          {/if}
        </div>
        <div class="flex gap-2">
          {#if headerActions}
            {@render headerActions(asset)}
          {:else if onClose}
            <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={onClose}>
              Close
            </button>
          {/if}
        </div>
      </div>

      <div class="mt-4 grid grid-cols-1 gap-6 lg:grid-cols-[1.2fr_1fr] items-start">
        {#if primary}
          {@render primary(asset)}
        {/if}
        {#if secondary}
          {@render secondary(asset)}
        {/if}
        {#if details}
          {@render details(asset)}
        {/if}
      </div>

      {#if footer}
        <div class="mt-4">
          {@render footer(asset)}
        </div>
      {/if}
    </div>
  </div>
{/if}
