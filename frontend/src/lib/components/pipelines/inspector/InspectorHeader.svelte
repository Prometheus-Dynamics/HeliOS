<script lang="ts">
  import type { Snippet } from 'svelte';

  type Props = {
    eyebrow?: string;
    title: string;
    summary?: string | null;
    details?: Array<string | null | undefined>;
    docHref?: string | null;
    docBadge?: string | null;
    extra?: Snippet;
    size?: 'lg' | 'xl';
  };

  const {
    eyebrow = '',
    title,
    summary = null,
    details = [],
    docHref = null,
    docBadge = null,
    extra,
    size = 'lg'
  }: Props = $props();

  const titleClass = $derived(size === 'xl' ? 'text-xl' : 'text-lg');
  const resolvedDocHref = $derived.by(() => {
    if (!docHref) return null;
    return docHref;
  });

  function openDoc(): void {
    if (!resolvedDocHref || typeof window === 'undefined') return;
    window.open(resolvedDocHref, '_blank', 'noopener,noreferrer');
  }
</script>

<header class="space-y-1">
  {#if eyebrow}
    <p class="text-xs uppercase tracking-[0.35em] text-surface-500">{eyebrow}</p>
  {/if}
  <h2 class={`${titleClass} font-semibold text-white`}>{title}</h2>
  {#if summary}
    <p class="text-xs text-surface-400">{summary}</p>
  {/if}
  {#each details as line, index (line ?? `detail-${index}`)}
    {#if line}
      <p class="text-xs text-surface-400">{line}</p>
    {/if}
  {/each}
  {#if resolvedDocHref}
    <p class="text-xs">
      <button
        class="inline-flex items-center gap-2 font-semibold text-primary-200 hover:text-primary-100"
        type="button"
        onclick={openDoc}
      >
        Docs
        {#if docBadge}
          <span class="rounded bg-primary-500/10 px-2 py-[2px] text-micro uppercase tracking-[0.22em] text-primary-100">
            {docBadge}
          </span>
        {/if}
      </button>
    </p>
  {/if}
  {#if extra}
    {@render extra()}
  {/if}
</header>
