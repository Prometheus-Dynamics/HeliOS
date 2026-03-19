<script lang="ts">
  import type { PipelineDataType } from '$lib/types/pipeline';
  import PipelinePort from './PipelinePort.svelte';
  import type { PortInteractionHandlers, PortRenderInfo } from './types';

  const props = $props<{
    direction: 'input' | 'output';
    ports: PortRenderInfo[];
    handlers: PortInteractionHandlers;
    interactive?: boolean;
    resolvePortStateClass: (direction: 'input' | 'output', type: PipelineDataType) => string;
    pixelColorInputAction: (node: HTMLInputElement, handleId: string) => { destroy: () => void };
    emptyLabel?: string;
  }>();

  const direction = $derived(props.direction);
  const ports = $derived(props.ports);
  const handlers = $derived(props.handlers);
  const interactive = $derived(props.interactive ?? true);
  const resolvePortStateClass = $derived(props.resolvePortStateClass);
  const pixelColorInputAction = $derived(props.pixelColorInputAction);
  const isInput = $derived(direction === 'input');
  const emptyLabel = $derived(props.emptyLabel ?? (isInput ? 'No inputs' : 'No outputs'));
</script>

<section class={`flex flex-col gap-2 ${isInput ? '-ml-1' : 'items-end -mr-1'}`}>
  {#if ports.length > 0}
    {#each ports as port (port.handleId)}
      <PipelinePort
        {direction}
        {port}
        {handlers}
        {interactive}
        {resolvePortStateClass}
        {pixelColorInputAction}
      />
    {/each}
  {:else}
    <p class="px-2 text-micro-tight uppercase tracking-[0.12em] text-surface-500/70">{emptyLabel}</p>
  {/if}
</section>
