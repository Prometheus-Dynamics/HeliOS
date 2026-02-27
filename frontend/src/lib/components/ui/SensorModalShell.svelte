<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import type { Snippet } from 'svelte';

  type SensorModalLayout = 'split' | 'stacked';

  type SensorModalShellProps = {
    open: boolean;
    title: string;
    subtitle?: string;
    eyebrow?: string;
    layout?: SensorModalLayout;
    maxWidthClass?: string;
    closeLabel?: string;
    closeOnBackdrop?: boolean;
    closeOnEsc?: boolean;
    className?: string;
    header?: Snippet;
    badges?: Snippet;
    actions?: Snippet;
    viewer?: Snippet;
    config?: Snippet;
    footer?: Snippet;
    onClose?: () => void;
  };

  const {
    open,
    title,
    subtitle = '',
    eyebrow = 'Peripheral viewer',
    layout = 'split',
    maxWidthClass = 'max-w-6xl',
    closeLabel = 'Close sensor modal',
    closeOnBackdrop = true,
    closeOnEsc = true,
    className = '',
    header,
    badges,
    actions,
    viewer,
    config,
    footer,
    onClose
  }: SensorModalShellProps = $props();

  const dispatch = createEventDispatcher<{ close: void }>();
  const hasHeader = $derived(Boolean(header || title || subtitle || eyebrow || badges || actions));
  const hasViewer = $derived(Boolean(viewer));
  const hasConfig = $derived(Boolean(config));

  function requestClose() {
    dispatch('close');
    onClose?.();
  }

  $effect(() => {
    if (!open || !closeOnEsc) return;
    const handleKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      requestClose();
    };
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  });

  export type $$Props = SensorModalShellProps;
</script>

{#if open}
  <div class={`fixed inset-0 z-[80] flex items-center justify-center bg-black/60 backdrop-blur-sm ${className}`.trim()}>
    <button class="absolute inset-0" type="button" aria-label={closeLabel} onclick={() => closeOnBackdrop && requestClose()}></button>
    <section
      class={`relative z-10 flex max-h-[90dvh] w-full flex-col ${maxWidthClass} overflow-hidden rounded-xl border border-surface-800/60 bg-surface-900 text-surface-100 shadow-2xl`}
      role="dialog"
      aria-modal="true"
    >
      {#if hasHeader}
        <header class="flex flex-wrap items-start justify-between gap-4 border-b border-surface-800/60 px-6 py-4">
          <div class="space-y-2">
            {#if header}
              {@render header()}
            {:else}
              {#if eyebrow}
                <p class="text-micro uppercase tracking-[0.3em] text-surface-500">{eyebrow}</p>
              {/if}
              <h2 class="text-2xl font-semibold text-surface-50">{title}</h2>
              {#if subtitle}
                <p class="text-sm text-surface-400">{subtitle}</p>
              {/if}
            {/if}
            {#if badges}
              <div class="flex flex-wrap gap-2 text-[0.7rem] uppercase tracking-[0.25em] text-surface-400">
                {@render badges()}
              </div>
            {/if}
          </div>
          <div class="flex flex-wrap items-center gap-2">
            {#if actions}
              {@render actions()}
            {/if}
            <button class="btn btn-2xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={requestClose}>
              Close
            </button>
          </div>
        </header>
      {/if}

      <div class="min-h-0 flex-1 overflow-y-auto px-6 py-6">
        <section class={`grid min-h-0 grid-cols-1 gap-6 ${layout === 'split' && hasConfig ? 'md:grid-cols-2' : 'md:grid-cols-1'}`}>
          {#if hasViewer}
            <div class="flex min-h-0 min-w-0 flex-col gap-4">
              <div
                class="flex min-h-0 flex-col gap-4 overflow-hidden rounded-xl border border-surface-800/60 bg-surface-950/40 p-4 shadow-inner shadow-black/20"
              >
                {#if viewer}
                  {@render viewer()}
                {/if}
              </div>
            </div>
          {/if}
          {#if hasConfig}
            <div class="flex min-h-0 min-w-0 flex-col gap-4">
              <div
                class="flex min-h-0 flex-col gap-4 overflow-hidden rounded-xl border border-surface-800/60 bg-surface-950/40 p-4 shadow-inner shadow-black/20"
              >
                {#if config}
                  {@render config()}
                {/if}
              </div>
            </div>
          {/if}
        </section>

        {#if footer}
          <footer class="flex flex-wrap items-center justify-between gap-4 pt-2">
            {@render footer()}
          </footer>
        {/if}
      </div>
    </section>
  </div>
{/if}
