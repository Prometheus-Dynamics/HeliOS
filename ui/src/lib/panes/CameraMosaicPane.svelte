<script lang="ts">
  // All cameras at once, tiled to fill the pane without scrolling. Click a
  // tile to select it; double-click to open it big.
  import { colorVar, identity } from "#lib/core/identity.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { shell } from "#lib/core/shell.svelte.js";
  import { menu } from "#lib/core/menu.svelte.js";
  import IconButton from "#lib/kit/IconButton.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import Feed from "#lib/vision/Feed.svelte";
  import { DEFAULT_OVERLAYS, overlayMenu, type Overlays } from "#lib/vision/overlays.js";
  import { workspaces } from "#lib/core/workspace.svelte.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "#lib/workspace/panes.js";

  let { pane, ws }: PaneProps = $props();
  const overlays = $derived({ ...DEFAULT_OVERLAYS, histogram: false, ...((pane.props?.overlays as Partial<Overlays>) ?? {}) });
  const onlyNative = $derived(Boolean(pane.props?.native));
  const cams = $derived(cluster.cameras.filter((c) => !onlyNative || !c.foreign));

  let host = $state<HTMLDivElement>();
  let size = $state({ w: 1, h: 1 });
  $effect(() => {
    if (!host) return;
    const ro = new ResizeObserver(([e]) => (size = { w: e.contentRect.width, h: e.contentRect.height }));
    ro.observe(host);
    return () => ro.disconnect();
  });
  // Pick the column count that makes the tiles largest.
  const cols = $derived.by(() => {
    let best = 1;
    let bestArea = 0;
    for (let c = 1; c <= cams.length; c++) {
      const r = Math.ceil(cams.length / c);
      const w = size.w / c;
      const h = size.h / r;
      const tile = Math.min(w, h * 1.6);
      if (tile > bestArea) {
        bestArea = tile;
        best = c;
      }
    }
    return best;
  });
</script>

<PaneBar>
  <IconButton icon="filter-off" label={onlyNative ? "Show all cameras" : "Only HeliOS cameras"} active={onlyNative} size={24} onclick={() => workspaces.setProps(ws, pane.id, { native: !onlyNative })} />
  <IconButton icon="target" label="Overlays" size={24} onclick={(e) => menu.below(e.currentTarget as Element, overlayMenu(overlays, (p) => workspaces.setProps(ws, pane.id, { overlays: { ...overlays, ...p } })), "end")} />
</PaneBar>

<div class="grid" bind:this={host} style:grid-template-columns="repeat({cols}, 1fr)">
  {#each cams as c (c.resourceId)}
    {@const sel = selection.last.camera?.id === c.resourceId}
    <div
      class="tile"
      class:sel
      style:--c={colorVar(identity.get(c.resourceId).color)}
      role="button"
      tabindex="0"
      onclick={() => selection.select({ kind: "camera", id: c.resourceId })}
      ondblclick={() => shell.go("cameras")}
      onkeydown={(e) => e.key === "Enter" && selection.select({ kind: "camera", id: c.resourceId })}
    >
      <Feed camera={c} {overlays} compact />
    </div>
  {/each}
</div>

<style>
  .grid {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-auto-rows: 1fr;
    gap: 3px;
    padding: 3px;
    background: var(--s0);
  }
  .tile {
    display: flex;
    min-height: 0;
    min-width: 0;
    border-radius: var(--r-1);
    overflow: hidden;
    box-shadow: inset 0 0 0 1px transparent;
    position: relative;
  }
  .tile.sel::after {
    content: "";
    position: absolute;
    inset: 0;
    border: 2px solid var(--c);
    border-radius: var(--r-1);
    pointer-events: none;
  }
</style>
