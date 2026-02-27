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
</script>

<header class="space-y-1">
  {#if eyebrow}
    <p class="text-xs uppercase tracking-[0.35em] text-surface-500">{eyebrow}</p>
  {/if}
  <h2 class={`${titleClass} font-semibold text-white`}>{title}</h2>
  {#if summary}
    <p class="text-xs text-surface-400">{summary}</p>
  {/if}
  {#each details as line}
    {#if line}
      <p class="text-xs text-surface-400">{line}</p>
    {/if}
  {/each}
  {#if docHref}
    <p class="text-xs">
      <a class="inline-flex items-center gap-2 font-semibold text-primary-200 hover:text-primary-100" href={docHref}>
        Docs
        {#if docBadge}
          <span class="rounded bg-primary-500/10 px-2 py-[2px] text-micro uppercase tracking-[0.22em] text-primary-100">
            {docBadge}
          </span>
        {/if}
      </a>
    </p>
  {/if}
  {#if extra}
    {@render extra()}
  {/if}
</header>
