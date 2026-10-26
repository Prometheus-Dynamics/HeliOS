<script lang="ts">
  import { visibleStages } from "#lib/status/chain.js";
  import StatusChain from "#lib/components/status/StatusChain.svelte";
  // The 3D scene of a live map: a pipeline running Eidos's live mapper and dense mapper. Field view:
  // the field's tags, the robot and camera on the field, the map's points, path, keyframes and the
  // dense mesh. Camera view: the same scene from the camera's own pose, with its lens. Layers toggle
  // one by one. The mapper's pose still jumps and drifts without tags, so this is for watching.
  import Icon from "#lib/components/common/Icon.svelte";
  import IconButton from "#lib/kit/IconButton.svelte";
  import Picker from "#lib/kit/Picker.svelte";
  import Seg from "#lib/kit/Seg.svelte";
  import { identity } from "#lib/core/identity.svelte.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { field } from "#lib/stores/field.svelte.js";
  import { workspaces } from "#lib/core/workspace.svelte.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "#lib/workspace/panes.js";
  import { MapScene, ALL_LAYERS, type MeshShading, type SceneLayers, type SceneMode } from "#lib/three/map-scene.js";
  import type { MapTrackingState } from "#lib/vision/map.js";
  import { fieldOfView } from "#lib/vision/pose.js";
  import Empty from "./Empty.svelte";
  import { follow } from "./follow.svelte";

  let { pane, ws }: PaneProps = $props();

  const liveMaps = $derived(cluster.workloads.filter((w) => cluster.isLiveMap(w)));
  const target = follow(() => pane, () => ws, "workload", () => liveMaps[0]?.id);
  // The selected workload when it runs the live mapper, else the first live map.
  const followed = $derived(target.id ? cluster.workload(target.id) : undefined);
  const w = $derived(followed && cluster.isLiveMap(followed) ? followed : liveMaps[0]);
  const view = $derived(w && cluster.isLiveMap(w) ? cluster.mapFor(w) : null);
  const dense = $derived(w ? cluster.denseFor(w) : null);
  const posesView = $derived(w ? cluster.poseViewFor(w) : null);
  const camera = $derived(w ? cluster.cameraOf(w) : undefined);
  const intrinsics = $derived(camera ? cluster.intrinsicsFor(camera) : null);

  const mode = $derived((pane.props?.mode as SceneMode) ?? "field");
  const shading = $derived((pane.props?.shading as MeshShading) ?? "grey");
  const layers: SceneLayers = $derived({ ...ALL_LAYERS, ...((pane.props?.layers as Partial<SceneLayers>) ?? {}) });

  const pose = $derived(view?.pose ?? null);
  const snapshot = $derived(view?.snapshot ?? null);
  const estimate = $derived(pose?.valid ? pose.estimate : null);
  const sigma = $derived(estimate?.sigma ?? null);
  /** Aligned: the map is in the field's frame, so the field's tags and the robot are drawn. */
  const aligned = $derived(Boolean(snapshot?.aligned ?? pose?.alignment.aligned));
  const fieldPose = $derived(posesView?.field?.valid ? posesView.field : null);
  /** The camera in the scene's frame: the field's pose from the tags once aligned, else the mapper's. */
  const cameraPose = $derived(aligned && fieldPose?.cameraInField ? fieldPose.cameraInField : (estimate?.reference_from_camera ?? null));
  const robotPose = $derived(aligned ? (fieldPose?.robotInField ?? null) : null);
  const lens = $derived(intrinsics ? fieldOfView(intrinsics) : null);
  const mesh = $derived(dense?.mesh ?? null);
  const fieldCount = $derived(field.layout.tags.length);
  const placedCount = $derived(snapshot ? snapshot.tags.filter((t) => !(aligned && field.layout.tags.some((f) => f.id === t.id))).length : 0);

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

  $effect(() =>
    scene?.update({
      snapshot,
      camera: cameraPose,
      robot: robotPose,
      fieldTags: field.layout.tags,
      aligned,
      mesh,
      lens,
      mode,
      shading,
      layers,
    }),
  );

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

  const STATE_TONE: Record<MapTrackingState, string> = {
    tracking: "ok",
    relocalized: "ok",
    coasting: "warn",
    bootstrapping: "info",
    lost: "err",
  };
  const fixed = (value: number | undefined, digits = 2) => (value === undefined || !Number.isFinite(value) ? "–" : value.toFixed(digits));
  const LAYER_CHIPS: [keyof SceneLayers, string][] = [
    ["points", "Points"],
    ["path", "Path"],
    ["keyframes", "Keyframes"],
    ["mesh", "Mesh"],
    ["fieldTags", "Field tags"],
    ["mapTags", "Map tags"],
    ["camera", "Camera"],
    ["robot", "Robot"],
    ["floor", "Floor"],
  ];

  function setProps(patch: Record<string, unknown>) {
    workspaces.setProps(ws, pane.id, patch);
  }

  function toggleLayer(key: keyof SceneLayers) {
    setProps({ layers: { ...layers, [key]: !layers[key] } });
  }

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
  <span class="sep"></span>
  <Seg
    label="View"
    value={mode}
    options={[
      { value: "field", label: "Field" },
      { value: "camera", label: "Camera" },
    ]}
    onchange={(v) => setProps({ mode: v })}
  />
  <span class="sep"></span>
  <IconButton icon="focus-centered" label="Fit the content in view" size={24} onclick={() => scene?.fit()} disabled={mode !== "field"} />
  <IconButton icon="home" label="Reset to the default view" size={24} onclick={() => scene?.resetView()} disabled={mode !== "field"} />
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
    <div class="chips" role="group" aria-label="Layers">
      {#each LAYER_CHIPS as [key, label] (key)}
        <button type="button" class="chip" class:on={layers[key]} aria-pressed={layers[key]} onclick={() => toggleLayer(key)}>{label}</button>
      {/each}
      {#if layers.mesh}
        <span class="sep"></span>
        <Seg
          label="Mesh shading"
          value={shading}
          options={[
            { value: "grey", label: "Grey" },
            { value: "normal", label: "Normals" },
          ]}
          onchange={(v) => setProps({ shading: v })}
        />
      {/if}
    </div>

    <div class="stage" bind:this={host}>
      {#if failed}
        <div class="fallback">WebGL is not available in this browser, so the scene cannot be drawn.</div>
      {:else if !view}
        <div class="waiting">
          {#if visibleStages(camera?.status, ["calibration", "map"]).some((s) => s.state !== "ok")}
            <StatusChain status={camera?.status} only={["calibration", "map"]} variant="why" />
          {:else}
            Waiting for the first pose from the mapper…
          {/if}
        </div>
      {/if}
      {#if mode === "camera" && !cameraPose}
        <div class="note centre">The camera view stands at the camera's pose, which the mapper has not found yet.</div>
      {/if}
      {#if !aligned && view}
        <div class="note">Not aligned to the field yet: the field's tags and the robot are hidden, and the map is in its own frame.</div>
      {/if}
      <div class="legend" aria-label="Legend">
        {#if aligned}<span class="key"><i class="sw field"></i>field tags ({fieldCount})</span>{/if}
        <span class="key"><i class="sw placed"></i>map tags ({placedCount})</span>
        <span class="key"><i class="sw cloud"></i>points</span>
        <span class="key"><i class="sw path"></i>path and keyframes</span>
        {#if mesh}<span class="key"><i class="sw mesh"></i>mesh ({count(mesh.triangles)} triangles)</span>{/if}
        <span class="key"><i class="sw cam"></i>camera</span>
        {#if robotPose}<span class="key"><i class="sw robot"></i>robot</span>{/if}
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
        <dd>{aligned ? "aligned" : "own frame"}</dd>
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
      <div class="cell">
        <dt>Dense mesh</dt>
        <dd>{mesh ? `${count(mesh.vertices)} vertices` : "–"}</dd>
        <small>{mesh ? `${count(mesh.triangles)} triangles · ${mesh.keyframes} keyframes · ${fixed(mesh.voxelM, 2)} m voxels` : "waiting for a mesh"}</small>
      </div>
      <div class="cell sigma" data-tip="The largest 1σ of the pose's position and rotation, from the mapper's covariance. Checked against ground truth over many runs: it covers the real error and errs on the wide side.">
        <dt>Pose σ</dt>
        <dd>
          {#if sigma}
            ±{fixed(Math.max(...sigma.position_m), 2)} m · ±{fixed(Math.max(...sigma.rotation_deg), 1)}°
          {:else}
            <span class="muted">–</span>
          {/if}
        </dd>
        <small>largest axis, 1σ</small>
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
    overflow: auto;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
  }
  .chip {
    padding: 2px 9px;
    font-size: 11.5px;
    color: var(--fg-3);
    background: var(--s1);
    border: 1px solid var(--line);
    border-radius: var(--r-pill, 999px);
    cursor: pointer;
  }
  .chip.on {
    color: var(--fg);
    border-color: color-mix(in srgb, var(--accent) 55%, var(--line));
    background: color-mix(in srgb, var(--accent) 14%, var(--s1));
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
  .note {
    position: absolute;
    top: 8px;
    left: 8px;
    right: 8px;
    max-width: 52ch;
    padding: 5px 9px;
    font-size: 11.5px;
    color: var(--fg-2);
    background: color-mix(in srgb, var(--s0) 85%, transparent);
    border: 1px solid var(--line);
    border-radius: var(--r-2);
    pointer-events: none;
  }
  .note.centre {
    top: 50%;
    left: 50%;
    right: auto;
    transform: translate(-50%, -50%);
    text-align: center;
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
  .sw.mesh {
    background: linear-gradient(135deg, #8a909c, #e7e9ef);
  }
  .sw.cam {
    background: var(--accent);
    border-radius: 50%;
    width: 8px;
    height: 8px;
  }
  .sw.robot {
    background: transparent;
    border: 1.5px solid var(--info, #60a5fa);
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
