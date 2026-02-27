<script lang="ts">
  import type { Snippet } from 'svelte';

  type PortsLayout = 'split' | 'stacked';

  type NodePortsProps = {
    layout?: PortsLayout;
    className?: string;
    inputsLabel?: string;
    outputsLabel?: string;
    showLabels?: boolean;
    inputs?: Snippet;
    outputs?: Snippet;
  };

  const {
    layout = 'split',
    className = '',
    inputsLabel = 'Inputs',
    outputsLabel = 'Outputs',
    showLabels = true,
    inputs,
    outputs
  }: NodePortsProps = $props();

  export type $$Props = NodePortsProps;
</script>

<section class={`grid gap-3 ${layout === 'split' ? 'grid-cols-2' : 'grid-cols-1'} ${className}`.trim()}>
  <div class="space-y-2">
    {#if showLabels}
      <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">{inputsLabel}</p>
    {/if}
    {#if inputs}
      {@render inputs()}
    {/if}
  </div>
  {#if outputs}
    <div class="space-y-2">
      {#if showLabels}
        <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">{outputsLabel}</p>
      {/if}
      {@render outputs()}
    </div>
  {/if}
</section>
