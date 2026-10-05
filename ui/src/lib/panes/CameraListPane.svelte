<script lang="ts">
  // Every camera on the robot in one dense list: colour, name, host, live
  // rate and tags. Click selects (everything following the selection
  // switches); right-click for actions.
  import Icon from "$lib/components/common/Icon.svelte";
  import { colorVar, identity } from "$lib/core/identity.svelte";
  import { menu } from "$lib/core/menu.svelte";
  import { selection } from "$lib/core/selection.svelte";
  import { shell } from "$lib/core/shell.svelte";
  import Swatch from "$lib/kit/Swatch.svelte";
  import { cluster } from "$lib/stores/cluster.svelte";
  import type { Camera } from "$lib/api/model";

  const groups = $derived.by(() => {
    const out: { node: string; cams: Camera[] }[] = [];
    for (const c of cluster.cameras) {
      let g = out.find((x) => x.node === c.nodeId);
      if (!g) out.push((g = { node: c.nodeId, cams: [] }));
      g.cams.push(c);
    }
    return out;
  });

  function actions(c: Camera) {
    return menu.context(() => [
      { heading: identity.name(c.resourceId, c.name) },
      { label: "Show controls", icon: "adjustments-horizontal", run: () => { selection.select({ kind: "camera", id: c.resourceId }); shell.go("cameras"); } },
      { label: "Calibrate…", icon: "grid-4x4", run: () => { selection.select({ kind: "camera", id: c.resourceId }); shell.go("calibration"); } },
      { label: "Place on robot…", icon: "cube", run: () => { selection.select({ kind: "camera", id: c.resourceId }); shell.go("robot"); } },
      { separator: true },
      { label: "Reset settings", icon: "rotate-clockwise", run: () => cluster.setCamera(c.resourceId, { autoExposure: false, exposureUs: 2200, gain: 4, fps: 60, roi: null }) },
    ]);
  }
</script>

<div class="list">
  {#each groups as g (g.node)}
    {@const node = cluster.node(g.node)}
    <div class="host">
      <span class="hdot" class:warn={node?.health !== "online"}></span>
      {identity.name(g.node, node?.name ?? g.node)}
      <span class="addr">{node?.address}</span>
    </div>
    {#each g.cams as c (c.resourceId)}
      {@const sel = selection.current?.kind === "camera" ? selection.is("camera", c.resourceId) : selection.last.camera?.id === c.resourceId}
      {@const tags = cluster.detectionsFor(c).length}
      <button
        type="button"
        class="row"
        class:sel
        style:--c={colorVar(identity.get(c.resourceId).color)}
        onclick={() => selection.select({ kind: "camera", id: c.resourceId })}
        oncontextmenu={actions(c)}
      >
        <Swatch id={c.resourceId} icon="camera" size={20} />
        <span class="name">
          <b>{identity.name(c.resourceId, c.name)}</b>
          <span class="sub">{c.settings.width}×{c.settings.height} · {c.settings.format}{c.foreign ? ` · ${c.foreign}` : ""}</span>
        </span>
        <span class="num" class:dim={!tags} data-tip="Tags this frame"><Icon name="target" size={11} />{tags}</span>
        <span class="num fps" class:bad={c.stats.fps < c.settings.fps * 0.9}>{c.stats.fps.toFixed(0)}</span>
      </button>
    {/each}
  {/each}
</div>

<style>
  .list {
    flex: 1;
    overflow-y: auto;
    padding: 4px 0;
  }
  .host {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 10px 3px;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--fg-3);
  }
  .addr {
    margin-left: auto;
    font-family: var(--font-code);
    font-weight: 400;
    letter-spacing: 0;
    text-transform: none;
    color: var(--fg-4);
  }
  .hdot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--ok);
  }
  .hdot.warn {
    background: var(--warn);
  }
  .row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 4px 10px;
    text-align: left;
  }
  .row:hover {
    background: var(--s2);
  }
  .row.sel {
    background: color-mix(in oklab, var(--c) 13%, transparent);
  }
  .row.sel::before {
    content: "";
    position: absolute;
    left: 0;
    top: 4px;
    bottom: 4px;
    width: 2px;
    background: var(--c);
  }
  .name {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    line-height: 1.25;
  }
  .name b {
    font-size: 12.5px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub {
    font-size: 10.5px;
    color: var(--fg-3);
  }
  .num {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-family: var(--font-code);
    font-size: 11px;
    color: var(--fg-2);
  }
  .num.dim {
    color: var(--fg-4);
  }
  .fps {
    min-width: 20px;
    justify-content: flex-end;
    color: var(--ok);
  }
  .fps.bad {
    color: var(--warn);
  }
</style>
