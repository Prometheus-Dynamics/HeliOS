<script lang="ts">
  // A whole workspace: the layout tree, or one maximized stack. Too narrow for columns (a phone in
  // the pit), the layout collapses to one stack of every pane as tabs; the saved layout is untouched.
  import { workspaces, type LayoutNode, type PaneRef } from "#lib/core/workspace.svelte.js";
  import LayoutView from "./LayoutView.svelte";
  import StackView from "./StackView.svelte";

  let { id }: { id: string } = $props();
  const ws = $derived(workspaces.get(id));

  /** Narrower than this (px), the panes are one stack of tabs. */
  const NARROW_PX = 760;
  let host = $state<HTMLDivElement>();
  let narrow = $state(false);
  $effect(() => {
    if (!host) return;
    const ro = new ResizeObserver(([e]) => (narrow = e.contentRect.width < NARROW_PX));
    ro.observe(host);
    return () => ro.disconnect();
  });

  type Stack = Extract<LayoutNode, { kind: "stack" }>;

  function find(node: LayoutNode, stackId: string): Stack | null {
    if (node.kind === "stack") return node.id === stackId ? node : null;
    for (const c of node.children) {
      const hit = find(c, stackId);
      if (hit) return hit;
    }
    return null;
  }
  const maxed = $derived(ws && workspaces.maximized ? find(ws.root, workspaces.maximized) : null);

  /** Every pane of the layout, in reading order. */
  function tabsOf(node: LayoutNode): PaneRef[] {
    return node.kind === "stack" ? node.tabs : node.children.flatMap(tabsOf);
  }
  // One stack of every pane for the narrow layout. Its tabs are the layout's own refs, so the panes
  // keep their props; activating a tab is kept here, not in the saved layout.
  let flatActive = $state(0);
  const flat = $derived.by((): Stack | null => {
    if (!ws || !narrow) return null;
    const tabs = tabsOf(ws.root);
    return { kind: "stack", id: `${ws.id}-flat`, tabs, active: Math.min(flatActive, Math.max(0, tabs.length - 1)) };
  });
</script>

<div class="workspace" bind:this={host}>
  {#if ws}
    {#if flat}
      <StackView stack={flat} ws={id} onactivate={(i) => (flatActive = i)} />
    {:else if maxed}
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
  @media (max-width: 759px) {
    .workspace {
      padding: 0 4px;
    }
  }
</style>
