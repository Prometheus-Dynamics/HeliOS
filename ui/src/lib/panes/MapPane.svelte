<script lang="ts">
  // The live map: a pipeline running Eidos's live mapper, shown in 3D (point cloud, camera
  // path, keyframes, tags, the current camera) with its state and timings. The mapper's pose
  // still jumps and drifts without tags, so this is for watching and testing, not for robot code.
  import Icon from "#lib/components/common/Icon.svelte";
  import Picker from "#lib/kit/Picker.svelte";
  import { identity } from "#lib/core/identity.svelte.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { field } from "#lib/stores/field.svelte.js";
  import { workspaces } from "#lib/core/workspace.svelte.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "#lib/workspace/panes.js";
  import { MapScene } from "#lib/three/map-scene.js";
  import type { MapTrackingState } from "#lib/vision/map.js";
  import Empty from "./Empty.svelte";
  import { follow } from "./follow.svelte";

  let { pane, ws }: PaneProps = $props();

  const liveMaps = $derived(cluster.workloads.filter((w) => cluster.isLiveMap(w)));
  const target = follow(() => pane, () => ws, "workload", () => liveMaps[0]?.id);
  // The selected workload when it runs the live mapper, else the first live map.
  const followed = $derived(target.id ? cluster.workload(target.id) : undefined);
  const w = $derived(followed && cluster.isLiveMap(followed) ? followed : liveMaps[0]);
  const view = $derived(w && cluster.isLiveMap(w) ? cluster.mapFor(w) : null);
  const fieldIds = $derived(new Set(field.layout.tags.map((t) => t.id)));

  /** The p50 frame and map milliseconds over the last second, refreshed a few times a second. */
  let p50 = $state<{ frameMs: number; mapMs: number } | null>(null);
  $effect(() => {
    const workload = w;
    if (!workload) return;
    const read = () => (p50 = cluster.mapTimingP50(workload));
    read();
    const timer = window.setInterval(read, 250);
    return () => clearInterval(timer);
  });

  // The stage exists only while a live map runs, so the scene follows the element it is drawn into.
  let host = $state<HTMLDivElement | undefined>();
  let scene = $state<MapScene | null>(null);
  let failed = $state(false);

  $effect(() => {
    const element = host;
    if (!element) return;
    let created: MapScene;
    try {
      created = new MapScene(element);
    } catch {
      failed = true;
      return;
    }
    scene = created;
    return () => {
      created.dispose();
      scene = null;
    };
  });

  $effect(() => scene?.update({ snapshot: view?.snapshot ?? null, pose: view?.pose ?? null, fieldIds }));

  const pose = $derived(view?.pose ?? null);
  const snapshot = $derived(view?.snapshot ?? null);
  const estimate = $derived(pose?.valid ? pose.estimate : null);
  const sigma = $derived(estimate?.sigma ?? null);
  const fieldCount = $derived(snapshot ? snapshot.tags.filter((t) => fieldIds.has(t.id)).length : 0);
  const placedCount = $derived(snapshot ? snapshot.tags.length - fieldCount : 0);

  const STATE_TONE: Record<MapTrackingState, string> = {
    tracking: "ok",
    relocalized: "ok",
    coasting: "warn",
    bootstrapping: "info",
    lost: "err",
  };
  const fixed = (value: number | undefined, digits = 2) => (value === undefined || !Number.isFinite(value) ? "–" : value.toFixed(digits));
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
  {#if w && snapshot}
    <span class="sep"></span>
    <span class="count">{snapshot.points.count.toLocaleString()} points · {snapshot.trajectory.length} poses · {snapshot.keyframes.length} keyframes</span>
  {/if}
  {#if target.pinned}
    <button type="button" class="reset" onclick={() => workspaces.setProps(ws, pane.id, { workload: undefined })}>Follow selection</button>
  {/if}
</PaneBar>

{#if !liveMaps.length}
  <Empty icon="route" text="No live map runs here. Deploy the Live map template to build one from the camera." />
{:else if !w || w.state !== "running"}
  <Empty icon="route" text="This live map is not running." />
{:else}
  <div class="map">
    <div class="stage" bind:this={host}>
      {#if failed}
        <div class="fallback">WebGL is not available in this browser, so the map cannot be drawn.</div>
      {:else if !view}
        <div class="waiting">Waiting for the first pose from the mapper…</div>
      {/if}
      <div class="legend" aria-label="Legend">
        <span class="key"><i class="sw field"></i>field tags ({fieldCount})</span>
        <span class="key"><i class="sw placed"></i>placed tags ({placedCount})</span>
        <span class="key"><i class="sw cloud"></i>points</span>
        <span class="key"><i class="sw path"></i>path and keyframes</span>
        <span class="key"><i class="sw cam"></i>current camera</span>
      </div>
    </div>

    <dl class="status" aria-label="Map status">
      <div class="cell state">
        <dt>State</dt>
        <dd>
          {#if pose}
            <span class="pill {STATE_TONE[pose.state]}">{pose.state}</span>
          {:else}
            <span class="muted">no pose yet</span>
          {/if}
        </dd>
        <small title={pose?.reason ?? ""}>{pose?.reason || "–"}</small>
      </div>
      <div class="cell">
        <dt>Alignment</dt>
        <dd>{pose?.alignment.aligned ? "aligned" : "own frame"}</dd>
        <small>{pose?.alignment.matched_tags ?? 0} matched · {fixed(pose?.alignment.residual_m, 3)} m residual</small>
      </div>
      <div class="cell">
        <dt>Calibration</dt>
        <dd>{pose?.calibration.estimated ? "estimated" : "given"}</dd>
        <small>{fixed(pose?.calibration.rms_px, 2)} px rms · {pose?.calibration.views ?? 0} views</small>
      </div>
      <div class="cell">
        <dt>Timing p50 (1 s)</dt>
        <dd>{p50 ? `${fixed(p50.frameMs, 2)} / ${fixed(p50.mapMs, 2)} ms` : "–"}</dd>
        <small>frame / map</small>
      </div>
      <div class="cell sigma" data-tip="The mapper's own covariance. Eidos warns it is optimistic until the camera is calibrated.">
        <dt>Pose σ</dt>
        <dd>
          {#if sigma}
            ±{fixed(sigma.position_m[0], 2)} m · ±{fixed(sigma.rotation_deg[0], 1)}°
          {:else}
            <span class="muted">–</span>
          {/if}
        </dd>
        <small class="warn-text">model σ (not yet calibrated)</small>
      </div>
    </dl>
  </div>
{/if}

<style>
  .map {
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
    min-height: 260px;
    border-radius: var(--r-2);
    overflow: hidden;
    border: 1px solid var(--line);
    background: var(--inset, var(--bg));
  }
  .stage :global(.map-canvas) {
    display: block;
    width: 100%;
    height: 100%;
    outline: none;
  }
  .stage :global(.map-labels) {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .stage :global(.map-tag) {
    padding: 0 5px;
    border-radius: 5px;
    font-family: var(--font-code);
    font-size: 10.5px;
    font-weight: 600;
    line-height: 16px;
    color: var(--fg-2);
    background: color-mix(in srgb, var(--s0) 80%, transparent);
    white-space: nowrap;
  }
  .stage :global(.map-tag.field) {
    color: var(--accent-fg, var(--fg));
    background: color-mix(in srgb, var(--accent) 22%, var(--s0));
  }
  .fallback,
  .waiting {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 24px;
    text-align: center;
    color: var(--fg-3);
    font-size: 12.5px;
    pointer-events: none;
  }
  .legend {
    position: absolute;
    left: 10px;
    bottom: 10px;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
    max-width: calc(100% - 20px);
    padding: 6px 9px;
    border-radius: var(--r-2);
    background: color-mix(in srgb, var(--s0) 78%, transparent);
    border: 1px solid var(--line);
    font-size: 11px;
    color: var(--fg-3);
    pointer-events: none;
  }
  .key {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
  }
  .sw {
    width: 10px;
    height: 10px;
    border-radius: 2px;
    display: inline-block;
  }
  .sw.field {
    background: color-mix(in srgb, var(--accent) 55%, transparent);
    border: 1px solid var(--accent);
  }
  .sw.placed {
    background: color-mix(in srgb, var(--fg-2) 25%, transparent);
    border: 1px solid var(--fg-2);
  }
  .sw.cloud {
    background: linear-gradient(135deg, #5a5f6a, #d7dae2);
    border-radius: 50%;
  }
  .sw.path {
    height: 2px;
    background: var(--fg-3);
    border: 0;
  }
  .sw.cam {
    background: var(--accent);
    border-radius: 50%;
    width: 8px;
    height: 8px;
  }
  .status {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
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
  .cell .warn-text {
    color: var(--warn);
  }
  .pill {
    display: inline-block;
    padding: 1px 8px;
    border-radius: var(--r-pill, 999px);
    font-size: 12px;
    font-weight: 600;
    color: var(--on-accent, #0b0d11);
    background: var(--fg-3);
  }
  .pill.ok {
    background: var(--ok);
  }
  .pill.warn {
    background: var(--warn);
  }
  .pill.err {
    background: var(--err);
  }
  .pill.info {
    background: var(--info);
  }
  .muted {
    color: var(--fg-4);
  }
  .sep {
    width: 1px;
    height: 16px;
    background: var(--line);
    margin: 0 4px;
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
