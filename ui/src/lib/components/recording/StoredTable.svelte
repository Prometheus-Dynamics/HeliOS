<script lang="ts">
  // Every stored recording, newest first: what it is, how long and how big,
  // what it dropped, and what can be done with it.
  import StoredRow from "./StoredRow.svelte";
  import { recordings } from "#lib/stores/recordings.svelte.js";

  const items = $derived(recordings.stored);
</script>

{#if items.length === 0}
  {#if recordings.listingError}
    <p class="none err">The recordings could not be read: {recordings.listingError}</p>
  {:else}
    <p class="none">No recordings yet. Record a camera above, or capture the last seconds of a ring. They are listed here, newest first.</p>
  {/if}
{:else}
  <div class="table" role="table" aria-label="Stored recordings">
    <div class="head" role="row">
      <span>Recording</span>
      <span>Camera</span>
      <span>Recorded</span>
      <span class="r">Length</span>
      <span class="r">Size</span>
      <span>Drops</span>
      <span></span>
    </div>
    {#each items as item (item.id)}
      <StoredRow {item} />
    {/each}
  </div>
{/if}

<style>
  .table {
    font-size: 12px;
    min-width: 0;
    overflow-x: auto;
  }
  .head,
  :global(.srow) {
    display: grid;
    grid-template-columns: minmax(170px, 1.7fr) minmax(90px, 1fr) 100px 64px 72px minmax(110px, 1fr) auto;
    align-items: center;
    gap: 10px;
    padding: 0 10px;
  }
  .head {
    height: 24px;
    font-size: 10.5px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--fg-3);
    border-bottom: 1px solid var(--line);
  }
  .r {
    text-align: right;
  }
  .none {
    margin: 0;
    padding: 2px 10px 10px;
    font-size: 11.5px;
    color: var(--fg-3);
    line-height: 1.5;
  }
  .err {
    color: var(--err);
  }
</style>
