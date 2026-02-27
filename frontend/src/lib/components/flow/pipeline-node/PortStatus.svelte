<script lang="ts">
  import type { PortRenderInfo } from './types';

  type PortStatusProps = {
    direction: 'input' | 'output';
    port: PortRenderInfo;
  };

  const { direction, port }: PortStatusProps = $props();

  const isInput = $derived(direction === 'input');
  const hasPixelDisplay = $derived(Boolean(port.isPixel && port.hasConstant && port.pixelHex));
  const showConstant = $derived(
    port.hasConstant &&
      !port.isEnum &&
      !port.isBoolean &&
      !port.isNumeric &&
      !hasPixelDisplay
  );
  const showDefault = $derived(!port.hasConstant && Boolean(port.defaultDisplay));

  export type $$Props = PortStatusProps;
</script>

{#if showConstant}
  <span
    class={`pipeline-port__constant mt-1 inline-flex max-w-[9.5rem] flex-wrap items-center gap-1 rounded px-2 py-[3px] text-micro-tight font-semibold uppercase tracking-[0.1em] ${
      isInput ? 'w-full' : 'ml-auto justify-end'
    }`}
    title={port.constantTooltip ?? undefined}
  >
    Const
    <span class="pipeline-port__constant-value max-w-full break-words text-micro-tight font-medium normal-case tracking-normal">
      {port.constantDisplay ?? '—'}
    </span>
  </span>
{:else if showDefault}
  <span
    class={`pipeline-port__constant pipeline-port__constant--default mt-1 inline-flex max-w-[9.5rem] flex-wrap items-center gap-1 rounded px-2 py-[3px] text-micro-tight font-semibold uppercase tracking-[0.1em] ${
      isInput ? 'w-full' : 'ml-auto justify-end'
    }`}
    title={port.defaultTooltip ?? undefined}
  >
    Default
    <span class="pipeline-port__constant-value max-w-full break-words text-micro-tight font-medium normal-case tracking-normal">
      {port.defaultDisplay}
    </span>
  </span>
{/if}
