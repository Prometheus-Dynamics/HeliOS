<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import type { Snippet } from 'svelte';

  type ModalSize = 'sm' | 'md' | 'lg' | 'xl';
  type ModalTone = 'default' | 'contrast';

  const sizeClasses: Record<ModalSize, string> = {
    sm: 'max-w-sm',
    md: 'max-w-lg',
    lg: 'max-w-3xl',
    xl: 'max-w-5xl'
  };

  const toneClasses: Record<ModalTone, string> = {
    default: 'border-surface-800 bg-surface-900/95',
    contrast: 'border-surface-700 bg-surface-950/95'
  };

  type ModalShellProps = {
    open: boolean;
    title?: string;
    subtitle?: string;
    size?: ModalSize;
    tone?: ModalTone;
    closeLabel?: string;
    closeOnBackdrop?: boolean;
    closeOnEsc?: boolean;
    className?: string;
    panelClassName?: string;
    header?: Snippet;
    footer?: Snippet;
    actions?: Snippet;
    children?: Snippet;
    onClose?: () => void;
  };

  const {
    open,
    title = '',
    subtitle = '',
    size = 'md',
    tone = 'default',
    closeLabel = 'Close',
    closeOnBackdrop = true,
    closeOnEsc = true,
    className = '',
    panelClassName = '',
    header,
    footer,
    actions,
    children,
    onClose
  }: ModalShellProps = $props();

  const dispatch = createEventDispatcher<{ close: void }>();

  const hasHeader = $derived(Boolean(header || title || subtitle || actions));

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

  export type $$Props = ModalShellProps;
</script>

{#if open}
  <div class={`fixed inset-0 z-50 flex items-center justify-center ${className}`.trim()} role="dialog" aria-modal="true">
    <button
      type="button"
      class="absolute inset-0 bg-black/60"
      aria-label={closeLabel}
      onclick={() => closeOnBackdrop && requestClose()}
    ></button>

    <section
      class={`relative z-10 flex max-h-[min(90dvh,52rem)] w-full flex-col overflow-hidden rounded-xl border shadow-2xl ${sizeClasses[size]} ${toneClasses[tone]} ${panelClassName}`.trim()}
    >
      {#if hasHeader}
        <header class="flex items-start justify-between gap-4 border-b border-surface-800/60 px-6 py-4">
          <div class="space-y-1">
            {#if header}
              {@render header()}
            {:else}
              {#if title}
                <h2 class="text-xl font-semibold text-surface-50">{title}</h2>
              {/if}
              {#if subtitle}
                <p class="text-sm text-surface-400">{subtitle}</p>
              {/if}
            {/if}
          </div>
          {#if actions}
            <div class="flex flex-wrap gap-2">
              {@render actions()}
            </div>
          {/if}
        </header>
      {/if}

      <div class="flex-1 overflow-y-auto px-6 py-4">
        {#if children}
          {@render children()}
        {/if}
      </div>

      {#if footer}
        <footer class="border-t border-surface-800/60 px-6 py-4">
          {@render footer()}
        </footer>
      {/if}
    </section>
  </div>
{/if}
