<script lang="ts">
  // What the pipelines publish and where it goes (NetworkTables, other
  // devices, recordings), with rates and latency.
  import Icon from "#lib/components/common/Icon.svelte";
  import { colorVar, identity } from "#lib/core/identity.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
</script>

<div class="table">
  <div class="head"><span>Stream</span><span>Producer</span><span class="r">Rate</span><span class="r">Latency</span><span>Goes to</span></div>
  {#each cluster.streams as s (s.id)}
    {@const w = cluster.workload(s.producer)}
    <div class="row">
      <span class="nm"><Icon name={s.schema === "poses" ? "target" : s.schema === "detections" ? "tag" : "activity"} size={13} /><b>{s.name}</b><i>{s.schema}</i></span>
      <span>
        {#if w}
          <button type="button" class="ref" style:--c={colorVar(identity.get(w.id).color)} onclick={() => selection.select({ kind: "workload", id: w.id })}><i></i>{identity.name(w.id, w.name)}</button>
        {:else}<span class="dim">{s.producer}</span>{/if}
      </span>
      <span class="r mono">{s.rateHz} Hz</span>
      <span class="r mono">{s.latencyMs.toFixed(1)} ms</span>
      <span class="bridges">{#each s.bridges as b (b)}<span>{b}</span>{/each}</span>
    </div>
  {/each}
</div>

<style>
  .table {
    flex: 1;
    overflow: auto;
    font-size: 12px;
  }
  .head,
  .row {
    display: grid;
    grid-template-columns: minmax(150px, 1.4fr) minmax(110px, 1fr) 60px 64px minmax(120px, 1.4fr);
    align-items: center;
    gap: 10px;
    padding: 0 10px;
  }
  .head {
    position: sticky;
    top: 0;
    height: 24px;
    font-size: 10.5px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--fg-3);
    background: var(--s1);
    border-bottom: 1px solid var(--line);
  }
  .row {
    height: 30px;
    border-bottom: 1px solid var(--line);
  }
  .row:hover {
    background: var(--s2);
  }
  .nm {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--fg-3);
    min-width: 0;
  }
  .nm b {
    color: var(--fg);
    font-weight: 600;
    white-space: nowrap;
  }
  .nm i {
    font-style: normal;
    font-size: 10.5px;
  }
  .ref {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
  }
  .ref i {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--c);
  }
  .r {
    text-align: right;
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11px;
  }
  .dim {
    color: var(--fg-3);
  }
  .bridges {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .bridges span {
    font-size: 10.5px;
    padding: 1px 6px;
    border-radius: var(--r-1);
    color: var(--info);
    background: var(--info-bg);
  }
</style>
