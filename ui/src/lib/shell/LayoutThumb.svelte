<script lang="ts">
  // A small picture of a layout: boxes in place, each labelled with its panes.
  import Icon from "$lib/components/common/Icon.svelte";
  import type { LayoutNode } from "$lib/core/workspace.svelte";
  import { paneDef } from "$lib/workspace/panes";
  import LayoutThumb from "./LayoutThumb.svelte";

  let { node }: { node: LayoutNode } = $props();
</script>

{#if node.kind === "stack"}
  <div class="box">
    {#each node.tabs as t (t.id)}
      {@const d = paneDef(t.type)}
      <span class="p"><Icon name={d?.icon ?? "circle-dashed"} size={10} />{d?.title ?? t.type}</span>
    {/each}
  </div>
{:else}
  <div class="split {node.dir}">
    {#each node.children as c, i (c.id)}
      <div class="cell" style:flex="{node.sizes[i]} 1 0"><LayoutThumb node={c} /></div>
    {/each}
  </div>
{/if}

<style>
  .split {
    display: flex;
    gap: 2px;
    width: 100%;
    height: 100%;
  }
  .split.col {
    flex-direction: column;
  }
  .cell {
    display: flex;
    min-width: 0;
    min-height: 0;
  }
  .box {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 3px 4px;
    min-width: 0;
    overflow: hidden;
    background: var(--s1);
    border: 1px solid var(--line-strong);
    border-radius: 2px;
  }
  .p {
    display: flex;
    align-items: center;
    gap: 3px;
    font-size: 9px;
    line-height: 1.2;
    color: var(--fg-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .p:not(:first-child) {
    color: var(--fg-4);
  }
</style>
