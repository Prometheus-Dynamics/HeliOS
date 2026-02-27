<script lang="ts">
  import type { Snippet } from 'svelte';

  type Props = {
    title?: string;
    eyebrow?: string;
    subtitle?: string;
    tone?: 'default' | 'warning' | 'muted';
    className?: string;
    children?: Snippet;
  };

  const {
    title = '',
    eyebrow = '',
    subtitle = '',
    tone = 'default',
    className = '',
    children
  }: Props = $props();

  const toneClass = $derived.by(() => {
    if (tone === 'warning') return 'border-amber-500/40 bg-amber-500/10 text-amber-50';
    if (tone === 'muted') return 'border-surface-700/60 bg-surface-900/70';
    return 'border-surface-700/70 bg-surface-900/70';
  });

  const titleClass = $derived.by(() => {
    if (tone === 'warning') return 'text-amber-100';
    if (tone === 'muted') return 'text-surface-400';
    return 'text-surface-500';
  });
</script>

<section class={`space-y-2 rounded border p-3 ${toneClass} ${className}`.trim()}>
  {#if eyebrow}
    <p class="text-[0.68rem] uppercase tracking-[0.26em] text-surface-400">{eyebrow}</p>
  {/if}
  {#if title}
    <p class={`text-[0.7rem] uppercase tracking-[0.3em] ${titleClass}`.trim()}>{title}</p>
  {/if}
  {#if subtitle}
    <p class="text-xs text-surface-400">{subtitle}</p>
  {/if}
  {#if children}
    {@render children()}
  {/if}
</section>
