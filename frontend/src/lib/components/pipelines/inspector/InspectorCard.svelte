<script lang="ts">
  import type { Snippet } from 'svelte';

  type Props = {
    children?: Snippet;
    className?: string;
  };

  const { children, className = '' }: Props = $props();
  const cardClass = $derived(`space-y-4 rounded border border-surface-800/80 bg-surface-950/80 p-5 text-sm text-surface-200 ${className}`.trim());
</script>

<div class={cardClass}>
  {#if children}
    {@render children()}
  {/if}
</div>
