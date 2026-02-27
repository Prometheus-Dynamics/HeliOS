<script lang="ts">
  import type { Snippet } from 'svelte';

  type ConsoleTone = 'default' | 'contrast';

  const toneClasses: Record<ConsoleTone, string> = {
    default: 'bg-surface-900/40',
    contrast: 'bg-surface-950/70'
  };

  type ConsoleSurfaceProps = {
    title?: string;
    subtitle?: string;
    eyebrow?: string;
    status?: string;
    tone?: ConsoleTone;
    className?: string;
    headerClassName?: string;
    bodyClassName?: string;
    header?: Snippet;
    actions?: Snippet;
    toolbar?: Snippet;
    footer?: Snippet;
    children?: Snippet;
  };

  const {
    title = '',
    subtitle = '',
    eyebrow = '',
    status = '',
    tone = 'default',
    className = '',
    headerClassName = '',
    bodyClassName = '',
    header,
    actions,
    toolbar,
    footer,
    children
  }: ConsoleSurfaceProps = $props();

  const hasHeader = $derived(Boolean(header || title || subtitle || eyebrow || status || actions));

  export type $$Props = ConsoleSurfaceProps;
</script>

<section class={`flex h-full flex-col gap-4 rounded-xl border border-surface-800 ${toneClasses[tone]} ${className}`.trim()}>
  {#if hasHeader}
    <header class={`flex flex-wrap items-start justify-between gap-4 border-b border-surface-800/70 px-4 pt-4 ${headerClassName}`.trim()}>
      <div class="space-y-1">
        {#if header}
          {@render header()}
        {:else}
          {#if eyebrow}
            <p class="text-micro uppercase tracking-[0.3em] text-surface-500">{eyebrow}</p>
          {/if}
          {#if title}
            <h2 class="text-xl font-semibold text-surface-50">{title}</h2>
          {/if}
          {#if subtitle}
            <p class="text-sm text-surface-400">{subtitle}</p>
          {/if}
          {#if status}
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">{status}</p>
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

  {#if toolbar}
    <div class="border-b border-surface-800/70 px-4 pb-3">
      {@render toolbar()}
    </div>
  {/if}

  <div class={`flex-1 overflow-hidden px-4 pb-4 ${bodyClassName}`.trim()}>
    {#if children}
      {@render children()}
    {/if}
  </div>

  {#if footer}
    <footer class="border-t border-surface-800/70 px-4 py-3">
      {@render footer()}
    </footer>
  {/if}
</section>
