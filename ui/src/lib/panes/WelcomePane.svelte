<script lang="ts">
  // An empty spot in a layout: suggests panes to put here.
  import Icon from "$lib/components/common/Icon.svelte";
  import { pane as makePane, workspaces } from "$lib/core/workspace.svelte";
  import { NEW_PANE_MIME } from "$lib/workspace/library";
  import { allPanes } from "$lib/workspace/panes";
  import type { PaneProps } from "$lib/workspace/panes";

  let { pane, ws }: PaneProps = $props();
  const panes = $derived(allPanes().filter((p) => p.type !== "welcome"));

  function put(type: string) {
    // Replace this placeholder with the chosen pane.
    workspaces.open(ws, makePane(type));
    const w = workspaces.get(ws);
    const visit = (n: NonNullable<typeof w>["root"]): void => {
      if (n.kind === "stack") {
        const i = n.tabs.findIndex((t) => t.id === pane.id);
        if (i >= 0) workspaces.close(ws, n.id, i);
      } else n.children.forEach(visit);
    };
    if (w) visit(w.root);
  }
</script>

<div class="welcome">
  <p>Put something here. Click a pane, or drag one onto any edge to split.</p>
  <div class="grid">
    {#each panes as p (p.type)}
      <button
        type="button"
        draggable="true"
        ondragstart={(e) => e.dataTransfer?.setData(NEW_PANE_MIME, p.type)}
        onclick={() => put(p.type)}
        data-tip={p.summary}
      >
        <Icon name={p.icon} size={15} />
        <span>{p.title}</span>
        <i>{p.group}</i>
      </button>
    {/each}
  </div>
</div>

<style>
  .welcome {
    flex: 1;
    overflow-y: auto;
    padding: 14px;
  }
  p {
    margin: 0 0 10px;
    font-size: 12.5px;
    color: var(--fg-3);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 4px;
  }
  button {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 10px;
    font-size: 12px;
    text-align: left;
    color: var(--fg-2);
    background: var(--s2);
    border: 1px solid var(--line);
    border-radius: var(--r-2);
    cursor: grab;
  }
  button:hover {
    color: var(--fg);
    border-color: var(--line-strong);
  }
  button span {
    flex: 1;
  }
  button i {
    font-style: normal;
    font-size: 10px;
    color: var(--fg-4);
  }
</style>
