<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import type { Snippet } from 'svelte';
  import type { LocalizationFieldDefinition } from '$lib/features/localization/viewers/localizationViewerTypes';
  import LocalizationLegend from '$lib/features/localization/viewers/LocalizationLegend.svelte';

  type Props = {
    activeField: LocalizationFieldDefinition | null;
    minimapExpanded: boolean;
    topContainer: HTMLDivElement | null;
    topCanvas: HTMLCanvasElement | null;
    metricsActive: boolean;
    onMetricsToggle?: (() => void) | null;
    minimapControls?: Snippet;
  };

  let {
    activeField,
    minimapExpanded = $bindable(),
    topContainer = $bindable(),
    topCanvas = $bindable(),
    metricsActive,
    onMetricsToggle,
    minimapControls
  }: Props = $props();
  let settingsOpen = $state(false);
  let viewportWidth = $state(1280);
  let viewportHeight = $state(720);

  const MINIMAP_MARGIN_PX = 24;
  const EXPANDED_MAX_WIDTH_PX = 512;
  const EXPANDED_VIEWPORT_WIDTH_RATIO = 0.8;
  const MIN_EXPANDED_WIDTH_PX = 220;
  const CONTROLS_ALLOWANCE_PX = 220;

  function toggleMinimap(): void {
    minimapExpanded = !minimapExpanded;
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      toggleMinimap();
    }
  }

  function updateViewportBounds(): void {
    viewportWidth = Math.max(0, window.innerWidth);
    viewportHeight = Math.max(0, window.innerHeight);
  }

  const minimapAspect = $derived.by(() => {
    const width = activeField?.width ?? 1;
    const depth = activeField?.depth ?? 1;
    if (!Number.isFinite(width) || !Number.isFinite(depth) || width <= 0 || depth <= 0) {
      return 1;
    }
    return width / depth;
  });

  const expandedMinimapWidthPx = $derived.by(() => {
    const viewportMaxWidth = Math.max(160, viewportWidth - MINIMAP_MARGIN_PX * 2);
    const preferredWidth = Math.min(EXPANDED_MAX_WIDTH_PX, viewportWidth * EXPANDED_VIEWPORT_WIDTH_RATIO);
    const availableHeight = Math.max(140, viewportHeight - MINIMAP_MARGIN_PX * 2 - CONTROLS_ALLOWANCE_PX);
    let width = Math.min(preferredWidth, viewportMaxWidth);
    const projectedHeight = width / minimapAspect;
    if (projectedHeight > availableHeight) {
      width = availableHeight * minimapAspect;
    }
    const minWidth = Math.min(MIN_EXPANDED_WIDTH_PX, viewportMaxWidth);
    return Math.max(minWidth, Math.min(width, viewportMaxWidth));
  });

  onMount(() => {
    updateViewportBounds();
    window.addEventListener('resize', updateViewportBounds);
    return () => {
      window.removeEventListener('resize', updateViewportBounds);
    };
  });

  onDestroy(() => {
    window.removeEventListener('resize', updateViewportBounds);
  });
</script>

<div class="absolute right-6 top-6 z-50 flex flex-col items-end">
  <div
    class={`flex max-h-[calc(100vh-3rem)] flex-col items-end gap-3 overflow-y-auto pr-1 transition-all duration-300 ${
      minimapExpanded ? '' : 'w-48 sm:w-60 lg:w-72'
    }`}
    style={minimapExpanded ? `width:${expandedMinimapWidthPx.toFixed(0)}px;max-width:calc(100vw - 3rem);` : ''}
  >
    <div
      class={`w-full overflow-hidden rounded-md border border-surface-800 bg-surface-950/70 shadow-2xl transition-all duration-300 ${
        minimapExpanded ? 'cursor-zoom-out' : 'cursor-zoom-in'
      }`}
      style={`aspect-ratio:${activeField ? activeField.width / activeField.depth : 1};`}
      bind:this={topContainer}
      role="button"
      tabindex="0"
      aria-pressed={minimapExpanded}
      aria-label={minimapExpanded ? 'Collapse top-down map' : 'Expand top-down map'}
      onclick={toggleMinimap}
      onkeydown={handleKeydown}
    >
      <canvas bind:this={topCanvas} class="h-full w-full" aria-label="Top-down localization view"></canvas>
      <LocalizationLegend {activeField} {minimapExpanded} />
    </div>
    {#if onMetricsToggle}
      <button
        class={`pointer-events-auto w-auto min-w-[10rem] self-end rounded-md border px-4 py-2 text-micro uppercase tracking-[0.35em] shadow-lg transition ${
          metricsActive
            ? 'border-primary-400/70 bg-primary-500/20 text-primary-100'
            : 'border-surface-700/70 bg-surface-950/85 text-surface-200 hover:border-surface-500 hover:text-white'
        }`}
        type="button"
        onclick={() => onMetricsToggle?.()}
      >
        {metricsActive ? 'Hide metrics' : 'Show metrics'}
      </button>
    {/if}
    {#if minimapControls}
      <button
        class={`pointer-events-auto w-auto min-w-[10rem] self-end rounded-md border px-4 py-2 text-micro uppercase tracking-[0.35em] shadow-lg transition ${
          settingsOpen
            ? 'border-primary-400/70 bg-primary-500/20 text-primary-100'
            : 'border-surface-700/70 bg-surface-950/85 text-surface-200 hover:border-surface-500 hover:text-white'
        }`}
        type="button"
        aria-expanded={settingsOpen}
        aria-label={settingsOpen ? 'Hide viewer settings' : 'Show viewer settings'}
        onclick={() => (settingsOpen = !settingsOpen)}
      >
        {settingsOpen ? 'Hide settings' : 'Show settings'}
      </button>
      {#if settingsOpen}
        <div class="w-full">
          {@render minimapControls()}
        </div>
      {/if}
    {/if}
  </div>
</div>
