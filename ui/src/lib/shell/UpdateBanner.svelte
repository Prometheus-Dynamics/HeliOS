<script lang="ts">
  // The device was updated while this page was open: its API now runs another build than the one
  // the page started with, so this UI is out of date. The page keeps working; a reload loads the
  // new one. Shown until the reload (nothing else dismisses it).
  import { build } from "#lib/stores/build.svelte.js";
</script>

{#if build.updated}
  <div class="banner glass-layer" role="status">
    <span>HeliOS was updated.</span>
    <button type="button" class="reload" onclick={() => location.reload()}>Reload</button>
  </div>
{/if}

<style>
  .banner {
    position: fixed;
    top: 12px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 60;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 8px 6px 14px;
    font-size: 13px;
    color: var(--fg);
    border-radius: var(--r-2);
  }
  .reload {
    height: 26px;
    padding: 0 10px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--accent-text);
    background: var(--accent);
    border-radius: var(--r-1);
  }
  .reload:hover {
    filter: brightness(1.08);
  }
</style>
