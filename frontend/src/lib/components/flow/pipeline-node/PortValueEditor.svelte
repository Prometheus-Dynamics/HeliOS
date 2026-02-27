<script lang="ts">
  import ColorDropperButton from '$lib/components/controls/ColorDropperButton.svelte';
  import type { PortInteractionHandlers, PortRenderInfo } from './types';

  type PortValueEditorProps = {
    direction: 'input' | 'output';
    port: PortRenderInfo;
    handlers: PortInteractionHandlers;
    pixelColorInputAction: (node: HTMLInputElement, handleId: string) => { destroy: () => void };
  };

  const { direction, port, handlers, pixelColorInputAction }: PortValueEditorProps = $props();

  const isInput = $derived(direction === 'input');
  const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));
  const textWidthCh = (value: string, min: number, max: number) =>
    `${clamp((value?.trim().length ?? 0) + 1, min, max)}ch`;

  const typeLabel = $derived(
    (port.label ?? port.dataTypeKey ?? 'Unknown')?.toString().trim() || 'Unknown'
  );
  const labelWidth = $derived.by(() => textWidthCh(`${port.name} ${typeLabel}`, 6, 32));
  const enumLabel = $derived.by(() => {
    const match = port.variants.find((variant) => variant.raw === port.constantRaw);
    return match?.label ?? port.constantDisplay ?? port.constantRaw ?? '';
  });
  const enumWidth = $derived.by(() => textWidthCh(enumLabel, 2, 10));
  const numericWidth = $derived.by(() => textWidthCh(port.numericDisplay ?? '', 1, 6));

  let pixelInput = $state<HTMLInputElement | null>(null);

  const applyPixelDropper = (hex: string) => {
    const input = pixelInput;
    if (!input) return;
    input.value = hex;
    handlers.handlePixelColorInput(port, { currentTarget: input, stopPropagation: () => {} } as unknown as Event);
  };

  export type $$Props = PortValueEditorProps;
</script>

{#if port.isEnum && port.hasConstant}
  <label class="pipeline-port__enum mt-1 flex flex-col gap-[2px]">
    <span class="pipeline-port__enum-label">Value</span>
    <select
      class="pipeline-port__select"
      value={port.constantRaw ?? ''}
      style={`min-width:${enumWidth};width:${labelWidth};max-width:100%`}
      onclick={(event) => event.stopPropagation()}
      ondblclick={(event) => event.stopPropagation()}
      onchange={(event) => handlers.handleEnumSelect(port.name, event)}
    >
      {#each port.variants as variant (variant.raw)}
        <option value={variant.raw} title={variant.value}>{variant.label}</option>
      {/each}
    </select>
  </label>
{:else if port.isBoolean && port.hasConstant}
  <label
    class="pipeline-port__boolean mt-1 inline-flex items-center gap-2 rounded px-2 py-1 text-micro-tight font-semibold uppercase tracking-[0.1em]"
  >
    <input
      class="pipeline-port__boolean-input"
      type="checkbox"
      checked={port.booleanValue}
      disabled={!port.settable}
      onclick={(event) => event.stopPropagation()}
      onchange={(event) => handlers.handleBooleanToggle(port.name, event)}
    />
    <span class="pipeline-port__boolean-label">{port.booleanValue ? 'True' : 'False'}</span>
  </label>
{:else if port.isNumeric && port.hasConstant}
  <div
    class={`pipeline-port__numeric mt-1 inline-flex items-center rounded px-2 py-[3px] ${isInput ? '' : 'ml-auto'}`.trim()}
  >
    {#if port.numericHasSlider && port.numericMin != null && port.numericMax != null}
      <input
        type="range"
        class="pipeline-port__numeric-range mr-2"
        min={port.numericMin}
        max={port.numericMax}
        step={port.numericSliderStep}
        value={port.numericDisplay ?? ''}
        onclick={(event) => event.stopPropagation()}
        oninput={(event) => handlers.handleNumericInput(port, event)}
      />
    {/if}
    <input
      type="number"
      class={`pipeline-port__numeric-input ${isInput ? '' : 'text-right'}`.trim()}
      min={port.numericMin ?? undefined}
      max={port.numericMax ?? undefined}
      step={port.numericStep}
      inputmode={port.numericStep === '1' ? 'numeric' : 'decimal'}
      value={port.numericDisplay ?? ''}
      style={`min-width:${numericWidth};width:${labelWidth};max-width:100%`}
      onclick={(event) => event.stopPropagation()}
      ondblclick={(event) => event.stopPropagation()}
      oninput={(event) => handlers.handleNumericInput(port, event)}
      onblur={(event) => handlers.handleNumericBlur(port, event)}
      onbeforeinput={(event) => handlers.handleNumericBeforeInput(port, event)}
    />
  </div>
{:else if port.isPixel && port.hasConstant && port.pixelHex}
  {#if port.settable}
    <div
      class={`pipeline-port__pixel-wrapper mt-1 inline-flex max-w-[9.5rem] flex-col gap-1 ${
        isInput ? 'w-full' : 'items-end ml-auto'
      }`}
    >
      <div class="pipeline-port__pixel-actions">
        <button
          type="button"
          class="pipeline-port__pixel inline-flex items-center justify-between gap-2 rounded px-2 py-[3px] text-micro-tight font-semibold uppercase tracking-[0.1em]"
          style={`--pipeline-port-pixel:${port.pixelHex}`}
          onclick={(event) => handlers.handlePixelPreviewClick(port, event)}
        >
          {#if isInput}
            <span class="pipeline-port__pixel-swatch" aria-hidden="true"></span>
            <span class="pipeline-port__pixel-value">
              {port.pixelHex.toUpperCase()}
              {#if port.pixelAlpha != null && port.pixelAlpha !== 255}
                <span class="pipeline-port__pixel-alpha">α {port.pixelAlpha}</span>
              {/if}
            </span>
          {:else}
            <span class="pipeline-port__pixel-value">
              {port.pixelHex.toUpperCase()}
              {#if port.pixelAlpha != null && port.pixelAlpha !== 255}
                <span class="pipeline-port__pixel-alpha">α {port.pixelAlpha}</span>
              {/if}
            </span>
            <span class="pipeline-port__pixel-swatch" aria-hidden="true"></span>
          {/if}
        </button>
        <ColorDropperButton
          title="Pick color from screen"
          ariaLabel="Pick color from screen"
          disabled={!port.settable}
          onPick={applyPixelDropper}
        />
      </div>
      <input
        type="color"
        class="pipeline-port__pixel-input sr-only"
        value={port.pixelHex}
        bind:this={pixelInput}
        use:pixelColorInputAction={port.handleId}
        onchange={(event: Event) => handlers.handlePixelColorInput(port, event)}
      />
    </div>
  {:else}
    <div
      class={`pipeline-port__pixel pipeline-port__pixel--readonly mt-1 inline-flex items-center gap-2 rounded px-2 py-[3px] text-micro-tight font-semibold uppercase tracking-[0.1em] ${
        isInput ? '' : 'ml-auto'
      }`}
      style={`--pipeline-port-pixel:${port.pixelHex}`}
      title="Port is not settable"
    >
      {#if isInput}
        <span class="pipeline-port__pixel-swatch" aria-hidden="true"></span>
        <span class="pipeline-port__pixel-value">
          {port.pixelHex.toUpperCase()}
          {#if port.pixelAlpha != null && port.pixelAlpha !== 255}
            <span class="pipeline-port__pixel-alpha">α {port.pixelAlpha}</span>
          {/if}
        </span>
      {:else}
        <span class="pipeline-port__pixel-value">
          {port.pixelHex.toUpperCase()}
          {#if port.pixelAlpha != null && port.pixelAlpha !== 255}
            <span class="pipeline-port__pixel-alpha">α {port.pixelAlpha}</span>
          {/if}
        </span>
        <span class="pipeline-port__pixel-swatch" aria-hidden="true"></span>
      {/if}
    </div>
  {/if}
{/if}
