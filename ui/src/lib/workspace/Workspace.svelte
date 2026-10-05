<script lang="ts">
  // A whole workspace: the layout tree, or one maximized stack.
  import { workspaces, type LayoutNode } from "$lib/core/workspace.svelte";
  import LayoutView from "./LayoutView.svelte";
  import StackView from "./StackView.svelte";

  let { id }: { id: string } = $props();
  const ws = $derived(workspaces.get(id));

  function find(node: LayoutNode, stackId: string): Extract<LayoutNode, { kind: "stack" }> | null {
    if (node.kind === "stack") return node.id === stackId ? node : null;
    for (const c of node.children) {
      const hit = find(c, stackId);
      if (hit) return hit;
    }
    return null;
  }
  const maxed = $derived(ws && workspaces.maximized ? find(ws.root, workspaces.maximized) : null);
</script>

<div class="workspace">
  {#if ws}
    {#if maxed}
      <StackView stack={maxed} ws={id} />
    {:else}
      <LayoutView node={ws.root} ws={id} />
    {/if}
  {/if}
</div>

<style>
  .workspace {
    display: flex;
    flex: 1;
    min-width: 0;
    min-height: 0;
    padding: 0 4px 4px 0;
  }
</style>
