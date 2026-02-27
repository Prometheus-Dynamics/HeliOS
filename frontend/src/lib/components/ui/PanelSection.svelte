<script lang="ts">
  import type { Snippet } from 'svelte';

  type PanelSectionProps = {
    title?: string;
    description?: string;
    className?: string;
    header?: Snippet;
    actions?: Snippet;
    children?: Snippet;
  };

  const { title = '', description = '', className = '', header, actions, children }: PanelSectionProps = $props();

  export type $$Props = PanelSectionProps;
</script>

<section class={`space-y-2 ${className}`.trim()}>
  {#if header || title || description || actions}
    <div class="flex items-start justify-between gap-2">
      <div class="space-y-1">
        {#if header}
          {@render header()}
        {:else}
          {#if title}
            <p class="text-micro-tight uppercase tracking-[0.2em] text-surface-500">{title}</p>
          {/if}
          {#if description}
            <p class="text-xs text-surface-500">{description}</p>
          {/if}
        {/if}
      </div>
      {#if actions}
        <div class="flex items-center gap-2">
          {@render actions()}
        </div>
      {/if}
    </div>
  {/if}

  {#if children}
    <div class="space-y-2">
      {@render children()}
    </div>
  {/if}
</section>
