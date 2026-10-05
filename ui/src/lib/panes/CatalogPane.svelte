<script lang="ts">
  // Node types the robot's plugins provide. Drag onto a canvas, or click to
  // add to the pipeline you are editing.
  import { CATALOG, type NodeType } from "$lib/api/catalog";
  import Icon from "$lib/components/common/Icon.svelte";
  import { selection } from "$lib/core/selection.svelte";
  import { drafts } from "$lib/graph/drafts.svelte";
  import { CATEGORY_COLORS, CATEGORY_ICONS, CATEGORY_ORDER, DRAG_MIME, portColor } from "$lib/graph/graph";
  import { cluster } from "$lib/stores/cluster.svelte";

  let query = $state("");
  const groups = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const match = (t: NodeType) => !q || [t.title, t.id, t.plugin, t.category, t.summary, ...t.inputs.map((p) => p.type), ...t.outputs.map((p) => p.type)].some((s) => s.toLowerCase().includes(q));
    return CATEGORY_ORDER.map((category) => ({ category, types: CATALOG.filter((t) => t.category === category && match(t)) })).filter((g) => g.types.length);
  });
  const workloadId = $derived(selection.last.workload?.id ?? cluster.workloads[0]?.id);

  function add(type: NodeType) {
    if (!workloadId) return;
    const d = drafts.get(workloadId);
    const right = Math.max(0, ...d.nodes.map((n) => n.position.x)) + 260;
    const y = d.nodes.length ? Math.min(...d.nodes.map((n) => n.position.y)) : 0;
    d.add(type, { x: right, y });
    const added = d.nodes.at(-1);
    if (added) selection.select({ kind: "graph-node", id: added.id, parent: workloadId });
  }
</script>

<div class="cat">
  <label class="search">
    <Icon name="search" size={13} />
    <input type="search" placeholder="Search nodes, plugins, port types" bind:value={query} aria-label="Search nodes" />
  </label>
  <div class="scroll">
    {#each groups as g (g.category)}
      <div class="group" style:--cat={CATEGORY_COLORS[g.category]}>
        <Icon name={CATEGORY_ICONS[g.category]} size={11} stroke={2} />
        {g.category}
        <span class="n">{g.types.length}</span>
      </div>
      {#each g.types as t (t.id)}
        <button
          type="button"
          class="item"
          style:--cat={CATEGORY_COLORS[g.category]}
          draggable="true"
          ondragstart={(e) => {
            e.dataTransfer?.setData(DRAG_MIME, t.id);
            if (e.dataTransfer) e.dataTransfer.effectAllowed = "copy";
          }}
          onclick={() => add(t)}
          data-tip="{t.summary}\n{t.plugin} · click to add, or drag onto a canvas"
        >
          <span class="bar"></span>
          <span class="title">{t.title}</span>
          <span class="dots">
            {#each t.inputs as p (p.name)}<i style:background={portColor(p.type)}></i>{/each}
            {#if t.inputs.length && t.outputs.length}<Icon name="arrow-right" size={9} />{/if}
            {#each t.outputs as p (p.name)}<i style:background={portColor(p.type)}></i>{/each}
          </span>
        </button>
      {/each}
    {/each}
  </div>
</div>

<style>
  .cat {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 6px;
    padding: 0 8px;
    height: 26px;
    color: var(--fg-3);
    background: var(--inset);
    border: 1px solid var(--line);
    border-radius: var(--r-1);
  }
  .search:focus-within {
    border-color: var(--accent-ring);
  }
  .search input {
    flex: 1;
    min-width: 0;
    background: transparent;
    border: 0;
    outline: 0;
    font: inherit;
    font-size: 12px;
    color: var(--fg);
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
    padding-bottom: 6px;
  }
  .group {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 8px 10px 3px;
    font-size: 10.5px;
    font-weight: 650;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--cat);
  }
  .n {
    margin-left: auto;
    color: var(--fg-4);
    font-weight: 500;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 26px;
    padding: 0 10px 0 0;
    text-align: left;
    font-size: 12px;
    color: var(--fg-2);
    cursor: grab;
  }
  .item:hover {
    background: var(--s2);
    color: var(--fg);
  }
  .bar {
    width: 2px;
    height: 14px;
    margin-left: 10px;
    border-radius: 1px;
    background: var(--cat);
    opacity: 0.6;
  }
  .title {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dots {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    color: var(--fg-4);
  }
  .dots i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }
</style>
