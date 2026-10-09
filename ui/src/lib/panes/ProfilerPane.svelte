<script lang="ts">
  // Where a pipeline's time goes: each node's share of the tick, in graph
  // order, against the frame budget. Click a row to select the node.
  import { CATALOG_BY_ID } from "#lib/api/catalog.js";
  import { identity } from "#lib/core/identity.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { CATEGORY_COLORS } from "#lib/graph/graph.js";
  import Picker from "#lib/kit/Picker.svelte";
  import Spark from "#lib/kit/Spark.svelte";
  import Seg from "#lib/kit/Seg.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { workspaces } from "#lib/core/workspace.svelte.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "#lib/workspace/panes.js";
  import Empty from "./Empty.svelte";
  import { follow } from "./follow.svelte";

  let { pane, ws }: PaneProps = $props();
  const target = follow(() => pane, () => ws, "workload", () => cluster.workloads[0]?.id);
  const w = $derived(target.id ? cluster.workload(target.id) : undefined);
  const sort = $derived((pane.props?.sort as "graph" | "time") ?? "graph");

  const rows = $derived.by(() => {
    if (!w) return [];
    const list = w.graph.nodes.map((n) => ({ id: n.id, label: n.label, type: CATALOG_BY_ID[n.type], ms: w.perf.nodeMs[n.id] ?? 0 }));
    return sort === "time" ? list.sort((a, b) => b.ms - a.ms) : list;
  });
  const total = $derived(rows.reduce((s, r) => s + r.ms, 0));
  const max = $derived(Math.max(...rows.map((r) => r.ms), 1e-6));
  const budget = 1; // ms per frame target on the CM5
</script>

<PaneBar>
  <Picker
    label="Pipeline"
    icon="schema"
    value={w?.id}
    options={cluster.workloads.map((x) => ({ id: x.id, name: identity.name(x.id, x.name) }))}
    onpick={(id) => (target.pinned ? target.pin(id) : selection.select({ kind: "workload", id }))}
    pinned={target.pinned}
    ontogglepin={() => target.toggle()}
  />
  <span class="sep"></span>
  <Seg label="Sort" value={sort} options={[{ value: "graph", label: "Graph" }, { value: "time", label: "Time" }]} onchange={(v) => workspaces.setProps(ws, pane.id, { sort: v })} />
</PaneBar>

{#if w && w.state === "running"}
  <div class="prof">
    <div class="summary">
      <div class="kpi"><span>p50</span><b>{w.perf.tickP50Ms.toFixed(2)}</b><i>ms</i></div>
      <div class="kpi"><span>p99</span><b>{w.perf.tickP99Ms.toFixed(2)}</b><i>ms</i></div>
      <div class="kpi"><span>rate</span><b>{w.perf.fps}</b><i>fps</i></div>
      <div class="kpi"><span>cpu</span><b>{((w.perf.tickP50Ms * w.perf.fps) / 10).toFixed(1)}</b><i>% core</i></div>
      <div class="budget" data-tip="Share of the 1 ms per-frame budget">
        <div class="track"><div class="fill" class:over={w.perf.tickP50Ms > budget} style:width="{Math.min(100, (w.perf.tickP50Ms / budget) * 100)}%"></div></div>
        <span>{Math.round((w.perf.tickP50Ms / budget) * 100)}% of 1 ms</span>
      </div>
      <Spark values={w.perf.tickHistory} width={110} height={26} max={1.2} color="var(--accent)" />
    </div>
    <div class="rows">
      {#each rows as r (r.id)}
        {@const color = r.type ? CATEGORY_COLORS[r.type.category] : "var(--fg-3)"}
        {@const sel = selection.current?.kind === "graph-node" && selection.current.id === r.id && selection.current.parent === w.id}
        <button type="button" class="row" class:sel style:--cat={color} onclick={() => selection.select({ kind: "graph-node", id: r.id, parent: w.id })}>
          <span class="label">{r.label}</span>
          <span class="bar"><span style:width="{(r.ms / max) * 100}%"></span></span>
          <span class="ms">{r.ms >= 0.1 ? r.ms.toFixed(2) : (r.ms * 1000).toFixed(0)}<i>{r.ms >= 0.1 ? "ms" : "µs"}</i></span>
          <span class="pct">{total ? Math.round((r.ms / total) * 100) : 0}%</span>
        </button>
      {/each}
    </div>
  </div>
{:else if w}
  <Empty icon="stopwatch" text="{identity.name(w.id, w.name)} is {w.state}. Timings show while it runs." />
{:else}
  <Empty icon="stopwatch" text="Select a pipeline." />
{/if}

<style>
  .prof {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .summary {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--line);
    flex-wrap: wrap;
  }
  .kpi {
    display: flex;
    align-items: baseline;
    gap: 4px;
  }
  .kpi span {
    font-size: 10.5px;
    color: var(--fg-3);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .kpi b {
    font-family: var(--font-code);
    font-size: 15px;
    font-weight: 600;
  }
  .kpi i {
    font-style: normal;
    font-size: 11px;
    color: var(--fg-3);
  }
  .budget {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 140px;
    font-size: 11px;
    color: var(--fg-3);
  }
  .track {
    flex: 1;
    height: 5px;
    border-radius: 3px;
    background: var(--line);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--ok);
    border-radius: 3px;
  }
  .fill.over {
    background: var(--err);
  }
  .rows {
    flex: 1;
    overflow-y: auto;
    padding: 3px 0;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(80px, 140px) 1fr 62px 34px;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 22px;
    padding: 0 12px;
    font-size: 12px;
    text-align: left;
  }
  .row:hover {
    background: var(--s2);
  }
  .row.sel {
    background: var(--accent-tint);
  }
  .label {
    color: var(--fg-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .bar {
    height: 10px;
    border-radius: 2px;
    background: color-mix(in oklab, var(--line) 60%, transparent);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--cat);
    opacity: 0.8;
    border-radius: 2px;
  }
  .ms {
    font-family: var(--font-code);
    font-size: 11.5px;
    text-align: right;
  }
  .ms i {
    font-style: normal;
    color: var(--fg-3);
    margin-left: 2px;
    font-size: 10px;
  }
  .pct {
    font-family: var(--font-code);
    font-size: 11px;
    color: var(--fg-3);
    text-align: right;
  }
</style>
