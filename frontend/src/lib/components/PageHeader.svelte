<script lang="ts">
  import type { Snippet } from 'svelte';

type PageHeaderProps = {
  title: string;
  eyebrow?: string;
  subtitle?: string;
  actions?: Snippet;
};

const {
  title,
  eyebrow = '',
  subtitle = '',
  actions
}: PageHeaderProps = $props();

export type $$Props = PageHeaderProps;
export interface $$Slots {
  actions?: Record<string, never>;
}
</script>

<header class="flex flex-wrap items-end justify-between gap-4">
  <div class="space-y-1">
    {#if eyebrow}
      <p class="text-xs uppercase tracking-[0.4em] text-surface-500">{eyebrow}</p>
    {/if}
    <h1 class="text-3xl font-bold tracking-tight">{title}</h1>
    {#if subtitle}
      <p class="text-sm text-surface-500">{subtitle}</p>
    {/if}
  </div>
  {#if actions}
    <div class="flex flex-wrap gap-2">
      {@render actions()}
    </div>
  {/if}
</header>
