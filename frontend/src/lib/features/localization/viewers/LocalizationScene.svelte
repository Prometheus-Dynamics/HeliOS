<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { LocalizationFieldDefinition } from '$lib/features/localization/viewers/localizationViewerTypes';
  import LocalizationControls from '$lib/features/localization/viewers/LocalizationControls.svelte';

  type Props = {
    activeField: LocalizationFieldDefinition | null;
    footerStatus?: Snippet;
    mainContainer: HTMLDivElement | null;
    mainCanvas: HTMLCanvasElement | null;
  };

  let {
    activeField,
    footerStatus,
    mainContainer = $bindable(),
    mainCanvas = $bindable()
  }: Props = $props();
</script>

<div
  class="relative h-full w-full overflow-hidden bg-surface-950/30"
  bind:this={mainContainer}
>
  <canvas bind:this={mainCanvas} class="h-full w-full" aria-label="3D localization viewport"></canvas>
  <LocalizationControls {activeField} {footerStatus} />
</div>
