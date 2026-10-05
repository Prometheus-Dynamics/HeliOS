<script lang="ts">
  // One node of a workspace layout: a split (children with drag gutters) or a
  // tab stack. Gutters resize live and persist on release.
  import type { LayoutNode } from "$lib/core/workspace.svelte";
  import { workspaces } from "$lib/core/workspace.svelte";
  import LayoutView from "./LayoutView.svelte";
  import StackView from "./StackView.svelte";

  let { node, ws }: { node: LayoutNode; ws: string } = $props();

  let host = $state<HTMLDivElement>();
  const MIN = 0.06;

  function startResize(event: PointerEvent, index: number) {
    if (node.kind !== "split" || !host) return;
    const split = node;
    event.preventDefault();
    const rect = host.getBoundingClientRect();
    const total = split.dir === "row" ? rect.width : rect.height;
    const start = split.dir === "row" ? event.clientX : event.clientY;
    const a = split.sizes[index];
    const b = split.sizes[index + 1];
    const el = event.currentTarget as HTMLElement;
    el.setPointerCapture(event.pointerId);
    document.body.classList.add(split.dir === "row" ? "resizing-x" : "resizing-y");
    const move = (e: PointerEvent) => {
      const delta = ((split.dir === "row" ? e.clientX : e.clientY) - start) / total;
      const next = Math.min(a + b - MIN, Math.max(MIN, a + delta));
      split.sizes[index] = next;
      split.sizes[index + 1] = a + b - next;
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      document.body.classList.remove("resizing-x", "resizing-y");
      workspaces.commitSizes(ws);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  }
</script>

{#if node.kind === "stack"}
  <StackView stack={node} {ws} />
{:else}
  <div class="split {node.dir}" bind:this={host}>
    {#each node.children as child, i (child.id)}
      <div class="cell" style:flex="{node.sizes[i]} 1 0">
        <LayoutView node={child} {ws} />
      </div>
      {#if i < node.children.length - 1}
        <div
          class="gutter"
          role="separator"
          aria-orientation={node.dir === "row" ? "vertical" : "horizontal"}
          onpointerdown={(e) => startResize(e, i)}
          ondblclick={() => {
            if (node.kind !== "split") return;
            const sum = node.sizes[i] + node.sizes[i + 1];
            node.sizes[i] = node.sizes[i + 1] = sum / 2;
            workspaces.commitSizes(ws);
          }}
        ></div>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .split {
    display: flex;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
  }
  .split.col {
    flex-direction: column;
  }
  .cell {
    display: flex;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .gutter {
    flex: 0 0 4px;
    position: relative;
    z-index: 2;
  }
  .row > .gutter {
    cursor: col-resize;
  }
  .col > .gutter {
    cursor: row-resize;
  }
  .gutter::after {
    content: "";
    position: absolute;
    inset: 0;
    margin: auto;
    border-radius: 2px;
    transition: background var(--t-fast);
  }
  .row > .gutter::after {
    width: 2px;
  }
  .col > .gutter::after {
    height: 2px;
  }
  .gutter:hover::after,
  .gutter:active::after {
    background: var(--accent);
  }
  :global(body.resizing-x),
  :global(body.resizing-x *) {
    cursor: col-resize !important;
    user-select: none;
  }
  :global(body.resizing-y),
  :global(body.resizing-y *) {
    cursor: row-resize !important;
    user-select: none;
  }
</style>
