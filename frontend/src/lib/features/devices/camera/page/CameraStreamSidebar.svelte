<script lang="ts">
  import type { Snippet } from 'svelte';
  import CameraTabs from '$lib/features/devices/camera/page/CameraTabs.svelte';

  type TabItem = { id: string; label: string; ready: boolean };

  type Props = {
    tabs: TabItem[];
    activeTab: string;
    content?: Snippet;
  };

  let { tabs, activeTab = $bindable(), content }: Props = $props();
</script>

<div class="camera-stream-sidebar flex flex-col gap-2.5 xl:min-h-0 xl:overflow-hidden">
  <CameraTabs {tabs} bind:activeTab />
  <div class="rounded border border-surface-800/60 bg-surface-900/70 p-2.5 xl:flex-1 xl:min-h-0 xl:overflow-y-auto">
    {#if content}
      {@render content()}
    {/if}
  </div>
</div>

<style>
  .camera-stream-sidebar :global(.text-sm) {
    font-size: 0.8rem;
    line-height: 1.2rem;
  }

  .camera-stream-sidebar :global(.text-xs) {
    font-size: 0.72rem;
    line-height: 1rem;
  }

  .camera-stream-sidebar :global(.text-2xs),
  .camera-stream-sidebar :global(.text-micro) {
    font-size: 0.66rem;
    line-height: 0.9rem;
  }

  .camera-stream-sidebar :global(.tracking-\[0\.3em\]) {
    letter-spacing: 0.22em;
  }

  .camera-stream-sidebar :global(.btn.btn-2xs),
  .camera-stream-sidebar :global(.btn.btn-3xs) {
    letter-spacing: 0.22em;
  }
  .camera-stream-sidebar :global(.btn.btn-2xs),
  .camera-stream-sidebar :global(.btn.btn-3xs),
  .camera-stream-sidebar :global(.select.select-2xs) {
    font-size: 0.66rem;
  }
</style>
