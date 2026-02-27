<script lang="ts">
  import type { PortRenderInfo } from './types';

  type PortLabelProps = {
    direction: 'input' | 'output';
    port: PortRenderInfo;
  };

  const { direction, port }: PortLabelProps = $props();

  const isInput = $derived(direction === 'input');
  const typeLabel = $derived(
    (port.label ?? port.dataTypeKey ?? 'Unknown')?.toString().trim() || 'Unknown'
  );

  export type $$Props = PortLabelProps;
</script>

<span
  class={`pipeline-port__name text-micro-tight font-semibold uppercase tracking-[0.16em] ${isInput ? '' : 'ml-auto'}`.trim()}
>
  <span class="pipeline-port__name-text">{port.name}</span>
  <span class="pipeline-port__type-inline">{typeLabel}</span>
</span>
{#if port.detail}
  <span
    class={`pipeline-port__detail text-micro-tight uppercase tracking-[0.08em] ${isInput ? '' : 'ml-auto'}`.trim()}
  >
    {port.detail}
  </span>
{/if}
