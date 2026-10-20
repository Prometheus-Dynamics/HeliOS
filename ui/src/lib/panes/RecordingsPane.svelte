<script lang="ts">
  // Recordings of the device's cameras: record one now, capture the last seconds
  // of a ring, see where the storage stands, and keep, download or export the
  // stored recordings for Eidos.
  import RecordingNow from "#lib/components/recording/RecordingNow.svelte";
  import RingsPanel from "#lib/components/recording/RingsPanel.svelte";
  import StoragePanel from "#lib/components/recording/StoragePanel.svelte";
  import StoredTable from "#lib/components/recording/StoredTable.svelte";
  import { identity } from "#lib/core/identity.svelte.js";
  import IconButton from "#lib/kit/IconButton.svelte";
  import Picker from "#lib/kit/Picker.svelte";
  import Section from "#lib/kit/Section.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { recordings } from "#lib/stores/recordings.svelte.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";

  let picked = $state<string | undefined>(undefined);
  const camera = $derived(picked ?? cluster.cameras[0]?.resourceId);

  // Read the listing when the pane opens; the events keep it current after that.
  $effect(() => {
    void recordings.refresh();
  });
</script>

<PaneBar>
  <Picker
    label="Camera"
    icon="camera"
    value={camera}
    options={cluster.cameras.map((c) => ({ id: c.resourceId, name: identity.name(c.resourceId, c.name), hint: c.mount }))}
    onpick={(id) => (picked = id)}
  />
  <span class="sep"></span>
  <IconButton icon="refresh" label="Refresh" size={24} onclick={() => recordings.refresh()} />
</PaneBar>

<div class="recs">
  <Section title="Recording" key="rec-now">
    <RecordingNow {camera} />
  </Section>
  <Section title="Capture last" key="rec-rings" count={recordings.listing?.rings.length ?? 0}>
    <RingsPanel />
  </Section>
  <Section title="Storage" key="rec-storage">
    <StoragePanel />
  </Section>
  <Section title="Stored recordings" key="rec-stored" count={recordings.stored.length}>
    <StoredTable />
  </Section>
</div>

<style>
  .recs {
    flex: 1;
    overflow-y: auto;
    min-height: 0;
    font-size: 12px;
  }
  .recs > :global(section) {
    background: var(--s1);
  }
</style>
