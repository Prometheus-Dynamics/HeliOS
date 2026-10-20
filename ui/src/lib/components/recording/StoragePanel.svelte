<script lang="ts">
  // Where recordings are written, whether that storage can take them now (the
  // free space against the reserve the recorder keeps), and what is stored.
  import Icon from "#lib/components/common/Icon.svelte";
  import Badge from "#lib/kit/Badge.svelte";
  import { sizeText, storageText } from "#lib/recording/format.js";
  import { recordings } from "#lib/stores/recordings.svelte.js";

  const listing = $derived(recordings.listing);
  const status = $derived(storageText(listing?.storage ?? null, listing?.storage_error ?? recordings.listingError));
  const total = $derived(recordings.stored.reduce((sum, s) => sum + s.bytes, 0));
</script>

<div class="storage">
  <div class="row">
    <Icon name="folder-open" size={14} />
    <span class="dir mono">{listing?.storage?.dir ?? "No folder yet"}</span>
  </div>
  <div class="row"><Badge tone={status.tone} text={status.text} /></div>
  <p class="sum">{recordings.stored.length} stored · {sizeText(total)} in all</p>
</div>

<style>
  .storage {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 2px 10px 10px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    color: var(--fg-2);
  }
  .dir {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11.5px;
  }
  .sum {
    margin: 0;
    font-size: 11px;
    color: var(--fg-3);
  }
</style>
