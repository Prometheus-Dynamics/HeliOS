<script lang="ts">
  // Every pipeline on the robot with its health and speed, and the controls
  // to start, stop, restart or roll back. Click to edit it everywhere.
  import Icon from "$lib/components/common/Icon.svelte";
  import { colorVar, identity } from "$lib/core/identity.svelte";
  import { menu } from "$lib/core/menu.svelte";
  import { selection } from "$lib/core/selection.svelte";
  import { drafts } from "$lib/graph/drafts.svelte";
  import { tableFor, templateGraph, TEMPLATES } from "$lib/graph/graph";
  import IconButton from "$lib/kit/IconButton.svelte";
  import Spark from "$lib/kit/Spark.svelte";
  import Swatch from "$lib/kit/Swatch.svelte";
  import { cluster } from "$lib/stores/cluster.svelte";
  import type { Workload } from "$lib/api/model";
  import PaneBar from "$lib/workspace/PaneBar.svelte";

  const STATE_COLOR = { running: "var(--ok)", starting: "var(--info)", quarantined: "var(--err)", stopped: "var(--fg-4)" };

  function actions(w: Workload) {
    return [
      { heading: identity.name(w.id, w.name) },
      w.state === "stopped"
        ? { label: "Start", icon: "player-play" as const, run: () => cluster.restartWorkload(w.id) }
        : { label: "Stop", icon: "player-stop" as const, run: () => cluster.stopWorkload(w.id) },
      { label: "Restart", icon: "refresh" as const, run: () => cluster.restartWorkload(w.id) },
      { label: `Roll back to r${w.revision - 1}`, icon: "history" as const, disabled: !cluster.canRollBack(w.id), run: () => drafts.get(w.id).rollBack() },
      { separator: true as const },
      { label: "Move to device", icon: "server" as const, items: cluster.nodes.filter((n) => n.kind === "raze").map((n) => ({ label: n.name, checked: n.id === w.nodeId, run: () => (w.nodeId = n.id) })) },
      { label: "Camera input", icon: "camera" as const, items: cluster.cameras.map((c) => ({ label: identity.name(c.resourceId, c.name), color: colorVar(identity.get(c.resourceId).color), run: () => cluster.bindInput(w.id, "camera", c.resourceId) })) },
    ];
  }

  async function create(e: MouseEvent) {
    menu.below(e.currentTarget as Element, [
      { heading: "New pipeline from" },
      ...TEMPLATES.map((t) => ({
        label: t.title,
        icon: t.icon,
        hint: t.summary,
        run: async () => {
          const cam = cluster.cameras.find((c) => !c.foreign) ?? cluster.cameras[0];
          const id = await cluster.createWorkload(`${t.title} ${cluster.workloads.length + 1}`, cam.nodeId, cam.resourceId, templateGraph(t.id, tableFor(cam.nodeId)));
          selection.select({ kind: "workload", id });
        },
      })),
    ], "end");
  }
</script>

<PaneBar>
  <IconButton icon="plus" label="New pipeline" text="New" size={24} onclick={create} />
</PaneBar>

<div class="table">
  <div class="head">
    <span></span><span>Pipeline</span><span>Camera</span><span class="r">p50</span><span class="r">p99</span><span>trend</span><span class="r">fps</span><span class="r">mem</span><span></span>
  </div>
  {#each cluster.workloads as w (w.id)}
    {@const cam = cluster.camera(w.bindings.camera ?? "")}
    {@const sel = selection.last.workload?.id === w.id}
    <div
      class="row"
      class:sel
      style:--c={colorVar(identity.get(w.id).color)}
      role="button"
      tabindex="0"
      onclick={() => selection.select({ kind: "workload", id: w.id })}
      onkeydown={(e) => e.key === "Enter" && selection.select({ kind: "workload", id: w.id })}
      oncontextmenu={menu.context(() => actions(w))}
    >
      <span class="st" style:background={STATE_COLOR[w.state]} data-tip={w.state}></span>
      <span class="name">
        <Swatch id={w.id} icon="schema" size={18} />
        <b>{identity.name(w.id, w.name)}</b>
        {#if drafts.get(w.id).dirty}<i class="draft">draft</i>{/if}
        <span class="host">{cluster.node(w.nodeId)?.name} · r{w.revision}</span>
      </span>
      <span class="cam">
        {#if cam}<span class="cdot" style:background={colorVar(identity.get(cam.resourceId).color)}></span>{identity.name(cam.resourceId, cam.name)}{/if}
      </span>
      {#if w.state === "running"}
        <span class="r mono">{w.perf.tickP50Ms.toFixed(2)}</span>
        <span class="r mono dim">{w.perf.tickP99Ms.toFixed(2)}</span>
        <Spark values={w.perf.tickHistory} color="var(--c)" width={54} height={14} max={1.2} />
        <span class="r mono">{w.perf.fps}</span>
        <span class="r mono dim">{w.perf.memMiB.toFixed(1)}</span>
      {:else}
        <span class="r dim span4">{w.state === "quarantined" ? `quarantined · ${w.restarts} restarts` : w.state}</span>
        <span></span>
      {/if}
      <span class="acts">
        {#if w.state === "running"}
          <IconButton icon="player-stop" label="Stop" size={22} onclick={(e) => { e.stopPropagation(); cluster.stopWorkload(w.id); }} />
        {:else}
          <IconButton icon="player-play" label="Start" size={22} tone="ok" onclick={(e) => { e.stopPropagation(); cluster.restartWorkload(w.id); }} />
        {/if}
        <IconButton icon="dots-vertical" label="More" size={22} onclick={(e) => { e.stopPropagation(); menu.below(e.currentTarget as Element, actions(w), "end"); }} />
      </span>
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
    grid-template-columns: 14px minmax(160px, 2fr) minmax(70px, 1fr) 44px 44px 58px 30px 40px 52px;
    align-items: center;
    gap: 8px;
    padding: 0 6px 0 10px;
  }
  .head {
    position: sticky;
    top: 0;
    height: 24px;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--fg-3);
    background: var(--s1);
    border-bottom: 1px solid var(--line);
  }
  .row {
    position: relative;
    height: 32px;
    border-bottom: 1px solid var(--line);
    cursor: default;
  }
  .row:hover {
    background: var(--s2);
  }
  .row.sel {
    background: color-mix(in oklab, var(--c) 12%, transparent);
  }
  .row.sel::before {
    content: "";
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 2px;
    background: var(--c);
  }
  .st {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
  }
  .name b {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .host {
    font-size: 10.5px;
    color: var(--fg-3);
    white-space: nowrap;
  }
  .draft {
    font-style: normal;
    font-size: 10px;
    font-weight: 600;
    padding: 0 5px;
    border-radius: var(--r-1);
    color: var(--accent-fg);
    background: var(--accent-tint);
  }
  .cam {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--fg-2);
    white-space: nowrap;
    overflow: hidden;
  }
  .cdot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .r {
    text-align: right;
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11.5px;
  }
  .dim {
    color: var(--fg-3);
  }
  .span4 {
    grid-column: span 4;
    font-size: 11.5px;
  }
  .acts {
    display: flex;
    justify-content: flex-end;
    gap: 1px;
  }
</style>
