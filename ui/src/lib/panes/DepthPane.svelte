<script lang="ts">
  import StatusChain from "#lib/components/status/StatusChain.svelte";
  // The dense mapper's depth of a live map, large: the latest keyframe's inverse depth in a colour
  // map (yellow is near, purple far, transparent is unknown), with its frame and age, and the
  // mapper's work per tick. Depth comes at most twice a second, after a keyframe.
  import Picker from "#lib/kit/Picker.svelte";
  import { identity } from "#lib/core/identity.svelte.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { workspaces } from "#lib/core/workspace.svelte.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "#lib/workspace/panes.js";
  import { depthRgba } from "#lib/vision/dense.js";
  import Empty from "./Empty.svelte";
  import { follow } from "./follow.svelte";

  let { pane, ws }: PaneProps = $props();

  const liveMaps = $derived(cluster.workloads.filter((w) => cluster.isLiveMap(w)));
  const target = follow(() => pane, () => ws, "workload", () => liveMaps[0]?.id);
  const followed = $derived(target.id ? cluster.workload(target.id) : undefined);
  const w = $derived(followed && cluster.isLiveMap(followed) ? followed : liveMaps[0]);
  const dense = $derived(w ? cluster.denseFor(w) : null);
  const depth = $derived(dense?.depth ?? null);
  const timing = $derived(dense?.timing ?? null);
  /** The mapper's current frame, for how far behind the depth is. */
  const liveFrame = $derived(w && cluster.isLiveMap(w) ? cluster.mapFor(w)?.pose?.frame ?? null : null);
  const camera = $derived(w ? cluster.cameraOf(w) : undefined);

  let canvas = $state<HTMLCanvasElement>();
  $effect(() => {
    const d = depth;
    const target = canvas;
    if (!d || !target) return;
    target.width = d.width;
    target.height = d.height;
    target.getContext("2d")?.putImageData(new ImageData(depthRgba(d), d.width, d.height), 0, 0);
  });

  /** The clock the age is read against, refreshed a few times a second. */
  let now = $state(performance.now());
  $effect(() => {
    const timer = window.setInterval(() => (now = performance.now()), 250);
    return () => clearInterval(timer);
  });
  const ageS = $derived(dense?.depthAt ? Math.max(0, (now - dense.depthAt) / 1000) : null);
  const lag = $derived(depth && liveFrame !== null ? Math.max(0, liveFrame - depth.frame) : null);

  const fixed = (value: number | undefined, digits = 1) => (value === undefined || !Number.isFinite(value) ? "–" : value.toFixed(digits));
  const count = (n: number) => n.toLocaleString();
</script>

<PaneBar>
  <Picker
    label="Live map"
    icon="route"
    value={w?.id}
    options={liveMaps.map((x) => ({ id: x.id, name: identity.name(x.id, x.name) }))}
    onpick={(id) => target.pin(id)}
    pinned={target.pinned}
    ontogglepin={() => target.toggle()}
  />
  {#if camera}<span class="count">{camera.name}</span>{/if}
  {#if target.pinned}
    <button type="button" class="reset" onclick={() => workspaces.setProps(ws, pane.id, { workload: undefined })}>Follow selection</button>
  {/if}
</PaneBar>

{#if !liveMaps.length}
  <Empty icon="scan" text="No live map runs here. Deploy the Localization, 3D map and depth template to see depth." />
{:else if !w || w.state !== "running"}
  <Empty icon="scan" text="This live map is not running." />
{:else}
  <div class="depth-pane">
    <div class="stage">
      {#if depth}
        <canvas bind:this={canvas} class="image" aria-label="Depth of the latest keyframe, colour-mapped"></canvas>
        <div class="hud tl">
          <b>frame {count(depth.frame)}</b>
          <span>{ageS === null ? "–" : `${fixed(ageS, 1)} s old`}</span>
          {#if lag !== null}<span>{lag === 0 ? "live" : `${lag} frames behind`}</span>{/if}
        </div>
      {:else}
        <div class="waiting">
          <b>No depth yet</b>
          <StatusChain status={camera?.status} only={["calibration", "map", "depth"]} variant="why" />
          <span>The dense mapper makes depth once the camera is calibrated and it has a posed keyframe. {timing ? `It is ${timing.state}.` : ""}</span>
        </div>
      {/if}
      <div class="scale" aria-label="Colour scale: far to near">
        <span>far</span>
        <i class="bar"></i>
        <span>near</span>
        <span class="unknown"><i></i>unknown</span>
      </div>
    </div>

    <dl class="stats" aria-label="Dense mapper">
      <div class="cell">
        <dt>Mapper</dt>
        <dd>{timing?.state ?? "–"}</dd>
        <small>{timing ? `${count(timing.keyframes)} keyframes · ${count(timing.submaps)} submaps` : "no tick yet"}</small>
      </div>
      <div class="cell">
        <dt>Keyframe rate</dt>
        <dd>{timing ? `${fixed(timing.keyframeRate, 2)} /frame` : "–"}</dd>
        <small>{timing ? `every ${timing.spacing} frames · ${count(timing.pending)} pending` : ""}</small>
      </div>
      <div class="cell">
        <dt>Work per keyframe</dt>
        <dd>{timing ? `${fixed(timing.costMs, 1)} ms` : "–"}</dd>
        <small>{timing ? `budget ${fixed(timing.budgetMs, 1)} ms` : ""}</small>
      </div>
      <div class="cell">
        <dt>Sweep · fill · integrate</dt>
        <dd>{timing ? `${fixed(timing.sweepMs, 1)} · ${fixed(timing.fillMs, 1)} · ${fixed(timing.integrateMs, 1)} ms` : "–"}</dd>
        <small>{timing ? `mesh ${fixed(timing.meshMs, 1)} ms` : ""}</small>
      </div>
      <div class="cell">
        <dt>Depth</dt>
        <dd>{depth ? `${depth.width}×${depth.height}` : "–"}</dd>
        <small>{depth ? "inverse depth, 8 bits, half the sweep" : "waiting for a keyframe"}</small>
      </div>
    </dl>
  </div>
{/if}

<style>
  .depth-pane {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    min-width: 0;
    gap: 8px;
    padding: 8px 10px 10px;
  }
  .stage {
    position: relative;
    flex: 1;
    min-height: 240px;
    display: grid;
    place-items: center;
    border-radius: var(--r-2);
    overflow: hidden;
    border: 1px solid var(--line);
    background: repeating-conic-gradient(color-mix(in srgb, var(--s1) 70%, transparent) 0 25%, transparent 0 50%) 0 0 / 18px 18px;
  }
  .image {
    width: 100%;
    height: 100%;
    object-fit: contain;
    image-rendering: auto;
  }
  .waiting {
    display: grid;
    gap: 4px;
    padding: 24px;
    max-width: 48ch;
    text-align: center;
    color: var(--fg-3);
    font-size: 12.5px;
  }
  .waiting b {
    color: var(--fg-2);
    font-size: 13px;
  }
  .hud {
    position: absolute;
    top: 8px;
    left: 8px;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
    padding: 4px 9px;
    font-family: var(--font-code);
    font-size: 11.5px;
    color: #e8eaf0;
    background: rgba(0, 0, 0, 0.6);
    border-radius: var(--r-1);
    pointer-events: none;
  }
  .scale {
    position: absolute;
    left: 8px;
    right: 8px;
    bottom: 8px;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
    color: #e8eaf0;
    pointer-events: none;
  }
  .scale span {
    text-shadow: 0 0 3px rgba(0, 0, 0, 0.8);
  }
  .bar {
    flex: 1;
    max-width: 320px;
    height: 8px;
    border-radius: 4px;
    background: linear-gradient(90deg, #440154, #3b528b, #21918c, #5ec962, #fde725);
  }
  .unknown {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-left: auto;
  }
  .unknown i {
    width: 10px;
    height: 10px;
    border: 1px dashed #e8eaf0;
    border-radius: 2px;
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: 6px;
    margin: 0;
  }
  .cell {
    display: grid;
    gap: 2px;
    padding: 8px 10px;
    border-radius: var(--r-2);
    background: var(--s1);
    border: 1px solid var(--line);
    min-width: 0;
  }
  .cell dt {
    font-size: 10.5px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--fg-3);
  }
  .cell dd {
    margin: 0;
    font-family: var(--font-code);
    font-size: 13px;
    color: var(--fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cell small {
    font-size: 11px;
    color: var(--fg-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    font-size: 11.5px;
    color: var(--fg-3);
    font-family: var(--font-code);
  }
  .reset {
    margin-left: auto;
    font-size: 11.5px;
    color: var(--fg-3);
    background: none;
    border: 0;
    cursor: pointer;
  }
  .reset:hover {
    color: var(--fg);
  }
</style>
