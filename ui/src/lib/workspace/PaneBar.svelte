<script lang="ts">
  // A pane's toolbar, drawn in its stack's tab row.
  import type { Snippet } from "svelte";
  import { getBarSlot, portal } from "./bar";

  let { children }: { children: Snippet } = $props();
  const slot = getBarSlot();
</script>

{#if slot?.target && slot.active}
  <div class="pane-bar" use:portal={slot.target}>{@render children()}</div>
{/if}

<style>
  .pane-bar {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
    height: 100%;
  }
  .pane-bar :global(.sep) {
    width: 1px;
    height: 14px;
    margin: 0 4px;
    background: var(--line-strong);
    flex-shrink: 0;
  }
</style>
