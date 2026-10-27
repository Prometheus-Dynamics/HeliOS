<script lang="ts">
  // The saved maps a live-map pipeline starts from, in its settings (shown only when its graph runs
  // the live mapper): the map it loads when it starts, saving the map it has now, and the stored
  // maps with their download and delete. A save or delete that is refused says why beside its control.
  import { untrack } from "svelte";
  import type { Workload } from "#lib/api/model.js";
  import { deviceId } from "#lib/api/peers.js";
  import Icon from "#lib/components/common/Icon.svelte";
  import { identity } from "#lib/core/identity.svelte.js";
  import { timeAgo, bytes } from "#lib/format.js";
  import Choice from "#lib/kit/Choice.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import { mapChoices, mapOrigin, mapsSummary } from "#lib/maps/mapping.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { maps } from "#lib/stores/maps.svelte.js";
  import { toasts } from "#lib/stores/toasts.svelte.js";

  let { workload: w }: { workload: Workload } = $props();

  let name = $state("");
  let confirming = $state<string | null>(null);

  const host = $derived(cluster.mapHost(w.nodeId));
  const list = $derived(maps.lists[w.nodeId]);
  const readError = $derived(maps.readErrors[w.nodeId]);
  const saving = $derived(Boolean(maps.busy[`save:${w.id}`]));
  const saveError = $derived(maps.failures[`save:${w.id}`]);

  // The device names a map's pipeline and camera by their ids on that device; the cluster knows them by key.
  const names = {
    pipeline: (id: string) => {
      const p = cluster.workloads.find((x) => x.nodeId === w.nodeId && deviceId(w.nodeId, x.id) === id);
      return p ? identity.name(p.id, p.name) : id;
    },
    camera: (id: string) => {
      const c = cluster.cameras.find((x) => x.nodeId === w.nodeId && deviceId(w.nodeId, x.resourceId) === id);
      return c ? identity.name(c.resourceId, c.name) : id;
    },
  };

  // Read the host's maps when the pipeline (or its device) is shown.
  $effect(() => {
    const current = host;
    untrack(() => void maps.refresh(current));
  });

  const uploading = $derived(Boolean(maps.busy["upload"]));
  const uploadError = $derived(maps.failures["upload"]);

  async function save() {
    const saved = await cluster.saveMap(w.id, name.trim() || undefined);
    if (saved) {
      name = "";
      toasts.success(`Saved the map as ${saved.name}`);
    }
  }

  // The file is kept as a map named after it; the device's refusal (a bad header) is shown here.
  async function upload(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    const kept = await maps.upload(host, file, file.name.replace(/\.eidmap$/, "") || undefined);
    input.value = "";
    if (kept) toasts.success(`Kept the map ${kept.name}`);
  }
</script>

<Prop label="Starts from" help="The saved map the live mapper loads when this pipeline starts or restarts. A pipeline that runs now keeps the map it has until then.">
  <Choice label="Starts from" value={w.map ?? ""} options={mapChoices(list?.maps, w.map ?? null)} onchange={(value) => cluster.setMap(w.id, value || null)} />
</Prop>
<Prop label="Save the map" help="Keeps the map the live mapper has built so far as a saved map. Takes a few seconds, and the pipeline must be running.">
  <span class="save">
    <input class="name" placeholder="Name (optional)" aria-label="Map name" maxlength={120} bind:value={name} disabled={saving} />
    <button type="button" class="act" disabled={saving} onclick={save}>{saving ? "Saving…" : "Save now"}</button>
  </span>
</Prop>
{#if saveError}<div class="err">{saveError}</div>{/if}
<Prop label="Keep a map file" help="Keeps a map file saved elsewhere (a version 1 Eidos map). The board checks it when it is kept.">
  <span class="save">
    <input class="file" type="file" accept=".eidmap,application/octet-stream" aria-label="Map file" disabled={uploading} onchange={upload} />
  </span>
</Prop>
{#if uploadError}<div class="err">{uploadError}</div>{/if}

{#if readError}
  <div class="err">Cannot read the saved maps: {readError}</div>
{:else if !list}
  <div class="none">Reading the saved maps…</div>
{:else}
  <div class="total">{mapsSummary(list)}</div>
  {#each list.maps as map (map.id)}
    {@const failure = maps.failures[`delete:${map.id}`]}
    <div class="map">
      <div class="top">
        <span class="title">{map.name}</span>
        <span class="acts">
          {#if confirming === map.id}
            <span class="ask">Delete it?</span>
            <button
              type="button"
              class="act danger"
              disabled={Boolean(maps.busy[`delete:${map.id}`])}
              onclick={async () => {
                if (await maps.remove(host, map.id)) confirming = null;
              }}>Delete</button
            >
            <button type="button" class="act" onclick={() => (confirming = null)}>Keep</button>
          {:else}
            <a class="act" href={maps.downloadUrl(host, map.id)} download={`${map.id}.eidmap`}><Icon name="download" size={12} />Download</a>
            <button type="button" class="act" aria-label="Delete {map.name}" onclick={() => (confirming = map.id)}><Icon name="trash" size={12} /></button>
          {/if}
        </span>
      </div>
      <div class="sub">{timeAgo(map.saved_at_ms)} · {bytes(map.size_bytes)} · {mapOrigin(map, names)}</div>
      {#if failure}<div class="err">{failure}</div>{/if}
    </div>
  {:else}
    <div class="none">No saved maps yet. Save the live map once it has mapped the field.</div>
  {/each}
{/if}

<style>
  .save {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .name {
    flex: 1;
    min-width: 80px;
    height: 24px;
    padding: 0 8px;
    font: inherit;
    font-size: 12px;
    color: var(--fg);
    background: var(--inset);
    border: 1px solid var(--line);
    border-radius: var(--r-1);
    outline: none;
  }
  .name:focus {
    border-color: var(--accent-ring);
  }
  .file {
    flex: 1;
    min-width: 0;
    font-size: 11.5px;
    color: var(--fg-2);
  }
  .none {
    padding: 4px 10px 6px;
    font-size: 11.5px;
    color: var(--fg-3);
  }
  .err {
    padding: 2px 10px 4px;
    font-size: 11.5px;
    color: var(--err);
  }
  .total {
    padding: 4px 10px 2px;
    font-size: 11px;
    color: var(--fg-3);
  }
  .map {
    padding: 4px 10px 6px;
    border-bottom: 1px solid var(--line);
  }
  .map:hover {
    background: var(--s2);
  }
  .top {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--fg);
  }
  .sub {
    font-size: 10.5px;
    color: var(--fg-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .acts {
    display: flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }
  .ask {
    font-size: 11px;
    color: var(--fg-2);
    margin-right: 2px;
  }
  .act {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 22px;
    padding: 0 8px;
    font-size: 11.5px;
    color: var(--fg-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
    white-space: nowrap;
  }
  .act:disabled {
    opacity: 0.5;
  }
  .act.danger {
    color: var(--on-accent);
    background: var(--err);
    border-color: transparent;
  }
</style>
