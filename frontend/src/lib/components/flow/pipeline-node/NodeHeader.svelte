<script lang="ts">
  import type { Snippet } from 'svelte';

  type NodeHeaderProps = {
    title: string;
    subtitle?: string;
    eyebrow?: string;
    className?: string;
    titleClassName?: string;
    eyebrowClassName?: string;
    subtitleClassName?: string;
    metaClassName?: string;
    icon?: Snippet;
    status?: Snippet;
    meta?: Snippet;
    actions?: Snippet;
  };

  const {
    title,
    subtitle = '',
    eyebrow = '',
    className = '',
    titleClassName = '',
    eyebrowClassName = '',
    subtitleClassName = '',
    metaClassName = '',
    icon,
    status,
    meta,
    actions
  }: NodeHeaderProps = $props();

  const hasMeta = $derived(Boolean(subtitle || eyebrow || status || meta));

  export type $$Props = NodeHeaderProps;
</script>

<header class={`flex items-start justify-between gap-3 ${className}`.trim()}>
  <div class="flex min-w-0 items-start gap-3">
    {#if icon}
      <div class="pt-1">
        {@render icon()}
      </div>
    {/if}
    <div class="min-w-0 space-y-1">
      {#if eyebrow}
        <p class={`text-micro-tight uppercase tracking-[0.3em] text-surface-500 ${eyebrowClassName}`.trim()}>{eyebrow}</p>
      {/if}
      <h3 class={`truncate text-sm font-semibold text-surface-50 ${titleClassName}`.trim()}>{title}</h3>
      {#if hasMeta}
        <div class={`flex flex-wrap items-center gap-2 text-xs text-surface-400 ${metaClassName}`.trim()}>
          {#if subtitle}
            <span class={subtitleClassName}>{subtitle}</span>
          {/if}
          {#if status}
            {@render status()}
          {/if}
          {#if meta}
            {@render meta()}
          {/if}
        </div>
      {/if}
    </div>
  </div>
  {#if actions}
    <div class="flex flex-wrap items-center gap-2">
      {@render actions()}
    </div>
  {/if}
</header>
