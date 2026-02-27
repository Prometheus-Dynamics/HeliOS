<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { LocalizationFieldDefinition } from '$lib/features/localization/viewers/localizationViewerTypes';

  type Props = {
    activeField: LocalizationFieldDefinition | null;
    footerStatus?: Snippet;
  };

  const { activeField, footerStatus }: Props = $props();
</script>

<div
  class="pointer-events-none absolute inset-x-0 bottom-0 flex flex-wrap items-center justify-between gap-2 bg-gradient-to-t from-surface-950/90 to-transparent px-4 pb-4 pt-12 text-micro uppercase tracking-[0.35em] text-surface-400"
>
  <div class="flex flex-col gap-1">
    {#if activeField}
      <span>
        {activeField.name} view · {activeField.width.toFixed(2)}m × {activeField.depth.toFixed(2)}m
      </span>
    {/if}
    {#if footerStatus}
      {@render footerStatus()}
    {/if}
  </div>
  <div class="flex flex-wrap gap-3 text-micro-tight tracking-[0.3em]">
    <span>LMB Orbit</span>
    <span>MMB Pan</span>
    <span>Scroll Zoom</span>
  </div>
</div>
