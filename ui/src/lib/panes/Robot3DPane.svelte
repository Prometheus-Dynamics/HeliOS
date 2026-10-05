<script lang="ts">
  // The robot in 3D: cameras with their fields of view, tags they see right
  // now, trails, and your CAD model. Pick mode attaches the selected camera
  // to the part you click.
  import type * as THREE from "three";
  import { colorVar, identity } from "$lib/core/identity.svelte";
  import { menu } from "$lib/core/menu.svelte";
  import { prefs } from "$lib/core/prefs.svelte";
  import { selection } from "$lib/core/selection.svelte";
  import { workspaces } from "$lib/core/workspace.svelte";
  import { pickFile } from "$lib/files";
  import IconButton from "$lib/kit/IconButton.svelte";
  import Seg from "$lib/kit/Seg.svelte";
  import { cluster } from "$lib/stores/cluster.svelte";
  import { robot } from "$lib/stores/robot.svelte";
  import { toasts } from "$lib/stores/toasts.svelte";
  import { cssVarColor } from "$lib/three/colors";
  import { mountsFor, placeTag, type PlacedTag } from "$lib/three/mounts";
  import Scene3D from "$lib/three/Scene3D.svelte";
  import type { ViewPreset } from "$lib/three/scene";
  import PaneBar from "$lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "$lib/workspace/panes";

  let { pane, ws }: PaneProps = $props();
  const opts = $derived({ frustums: true, trails: true, grid: true, labels: true, ...((pane.props?.view as object) ?? {}) });
  let view = $state<ViewPreset>("free");
  let scene = $state<ReturnType<typeof Scene3D>>();

  const cams = $derived(cluster.cameras.map((c) => ({ ...c, name: identity.name(c.resourceId, c.name) })));
  // Re-resolve colours when identities or the theme change.
  const mounts = $derived.by(() => {
    void prefs.theme;
    return mountsFor(cams, robot.placements, (id) => cssVarColor(colorVar(identity.get(id).color)));
  });
  const tags = $derived.by(() => {
    const out: PlacedTag[] = [];
    for (const m of mounts) for (const pose of cluster.posesFor(m.camera)) out.push(placeTag(m, pose));
    return out;
  });
  const selected = $derived(selection.last.camera?.id ?? null);
  const pickingName = $derived(robot.picking ? identity.name(robot.picking, cluster.camera(robot.picking)?.name ?? "") : "");

  function onpick(h: { point: THREE.Vector3; normal: THREE.Vector3; part: string }) {
    const id = robot.picking;
    if (!id) return;
    // Face the camera out along the surface, a little off it.
    const n = h.normal;
    const yaw = (Math.atan2(n.y, n.x) * 180) / Math.PI;
    const pitch = Math.max(-30, Math.min(60, (Math.asin(Math.max(-1, Math.min(1, n.z))) * 180) / Math.PI)) || 15;
    const p = h.point.clone().addScaledVector(n, 0.02);
    robot.set(id, { pos: [+p.x.toFixed(3), +p.y.toFixed(3), +p.z.toFixed(3)], yaw: +yaw.toFixed(1), pitch: Math.abs(n.z) > 0.9 ? 15 : +pitch.toFixed(1), part: h.part });
    robot.picking = null;
    toasts.success(`${identity.name(id, cluster.camera(id)?.name ?? id)} mounted on “${h.part}”. Fine-tune it in Mounts.`);
  }

  async function upload() {
    const f = await pickFile(".glb,.gltf,.stl,.obj,.ply,.3mf,.step,.stp");
    if (f) robot.loadModel(f);
  }

  function setView(patch: Record<string, boolean>) {
    workspaces.setProps(ws, pane.id, { view: { ...opts, ...patch } });
  }
</script>

<PaneBar>
  <Seg label="View" value={view} options={[{ value: "free", label: "Orbit" }, { value: "top", label: "Top" }, { value: "behind", label: "Driver" }]} onchange={(v) => { view = v; scene?.goTo(v); }} />
  <span class="sep"></span>
  <IconButton
    icon="click"
    label={robot.picking ? "Cancel placing" : "Place the selected camera by clicking the robot"}
    text={robot.picking ? "Click a part…" : "Place camera"}
    active={Boolean(robot.picking)}
    size={24}
    disabled={!selected}
    onclick={() => (robot.picking = robot.picking ? null : selected)}
  />
  <IconButton icon="upload" label="Load robot CAD (GLB, glTF, STL, OBJ, PLY, 3MF)" size={24} onclick={upload} />
  <IconButton
    icon="eye"
    label="Show"
    size={24}
    onclick={(e) =>
      menu.below(e.currentTarget as Element, [
        { heading: "Show" },
        { label: "Fields of view", checked: opts.frustums, run: () => setView({ frustums: !opts.frustums }) },
        { label: "Tag trails", checked: opts.trails, run: () => setView({ trails: !opts.trails }) },
        { label: "Ground grid", checked: opts.grid, run: () => setView({ grid: !opts.grid }) },
        { label: "Labels", checked: opts.labels, run: () => setView({ labels: !opts.labels }) },
      ], "end")}
  />
</PaneBar>

<div class="wrap">
  {#key prefs.theme}
    <Scene3D
      bind:this={scene}
      {mounts}
      {tags}
      frame={cluster.frame}
      {selected}
      options={opts}
      onselect={(id) => selection.select({ kind: "camera", id })}
      onusermove={() => (view = "free")}
      model={robot.model}
      modelOpts={robot.info}
      picking={Boolean(robot.picking)}
      {onpick}
    />
  {/key}
  {#if robot.picking}
    <div class="banner">Placing <b>{pickingName}</b>: click the part it mounts on. <button type="button" onclick={() => (robot.picking = null)}>Cancel</button></div>
  {/if}
  <div class="legend">
    {#each mounts as m (m.camera.resourceId)}
      <button type="button" class:on={selected === m.camera.resourceId} style:--c={m.color} onclick={() => selection.select({ kind: "camera", id: m.camera.resourceId })}>
        <i></i>{m.camera.name}<span>{tags.filter((t) => t.cameraId === m.camera.resourceId).length}</span>
      </button>
    {/each}
  </div>
</div>

<style>
  .wrap {
    position: relative;
    flex: 1;
    min-height: 0;
  }
  .banner {
    position: absolute;
    top: 8px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 6px 5px 12px;
    font-size: 12px;
    color: var(--on-accent);
    background: var(--accent);
    border-radius: var(--r-1);
    box-shadow: var(--shadow);
  }
  .banner button {
    padding: 2px 9px;
    font-size: 11.5px;
    font-weight: 600;
    color: inherit;
    background: rgba(0, 0, 0, 0.18);
    border-radius: var(--r-1);
  }
  .legend {
    position: absolute;
    left: 8px;
    bottom: 8px;
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .legend button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 22px;
    padding: 0 8px;
    font-size: 11.5px;
    color: var(--fg-2);
    background: color-mix(in oklab, var(--s1) 85%, transparent);
    border: 1px solid var(--line);
    border-radius: var(--r-1);
    backdrop-filter: blur(6px);
  }
  .legend button.on {
    color: var(--fg);
    border-color: var(--c);
  }
  .legend i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--c);
  }
  .legend span {
    font-family: var(--font-code);
    font-size: 10.5px;
    color: var(--fg-3);
  }
</style>
