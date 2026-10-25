<script lang="ts">
  // A camera's own settings: the sensor and its image (capture, exposure, white balance,
  // flicker, advanced), then its lens and where it sits on the robot. Every pipeline on the
  // camera shares them. What runs on the camera and how a pipeline is tuned (its resolution,
  // detection, stages) are the pipeline's settings, one click away. Everything comes from the
  // camera service's control list, so the mock and a device show the same controls.
  import type { Camera } from "#lib/api/model.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import Badge from "#lib/kit/Badge.svelte";
  import IconButton from "#lib/kit/IconButton.svelte";
  import IdentityEditor from "#lib/kit/IdentityEditor.svelte";
  import Num from "#lib/kit/Num.svelte";
  import Picker from "#lib/kit/Picker.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import Section from "#lib/kit/Section.svelte";
  import AprilTagPreset from "#lib/camera/AprilTagPreset.svelte";
  import ControlRow from "#lib/camera/ControlRow.svelte";
  import ExposureFields from "#lib/camera/ExposureFields.svelte";
  import FrameRateField from "#lib/camera/FrameRateField.svelte";
  import WhiteBalanceFields from "#lib/camera/WhiteBalanceFields.svelte";
  import { friendlyLabel, groupOf, isChanged, resetTarget } from "#lib/camera/controls.js";
  import type { CameraControlInfo } from "#lib/api/model.js";
  import { capabilitiesOf } from "#lib/core/defaults.js";
  import { pane as paneRef, workspaces } from "#lib/core/workspace.svelte.js";
  import { keyOf, writeControls } from "#lib/camera/write.js";
  import { identity } from "#lib/core/identity.svelte.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { calibration } from "#lib/stores/calibration.svelte.js";
  import { robot } from "#lib/stores/robot.svelte.js";
  import { guide } from "#lib/core/guide.svelte.js";
  import { shell } from "#lib/core/shell.svelte.js";
  import { toasts } from "#lib/stores/toasts.svelte.js";
  import { downloadJson, pickJson } from "#lib/files.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "#lib/workspace/panes.js";
  import Empty from "./Empty.svelte";
  import { follow } from "./follow.svelte";

  let { pane, ws }: PaneProps = $props();
  const target = follow(() => pane, () => ws, "camera", () => cluster.cameras[0]?.resourceId);
  const camera = $derived(target.id ? cluster.camera(target.id) : undefined);
  const node = $derived(camera ? cluster.node(camera.nodeId) : undefined);
  const active = $derived(camera ? guide.active(camera) : undefined);
  const cal = $derived(camera ? calibration.saved[calibration.key(camera)] : undefined);
  const place = $derived(camera ? robot.placement(camera.resourceId) : null);

  const controls = $derived(camera?.controls ?? []);
  let query = $state("");
  let changedOnly = $state(false);
  const q = $derived(query.trim().toLowerCase());
  /** A control row the search and the "changed only" filter let through. */
  const shown = (c: CameraControlInfo) => (!q || friendlyLabel(c.name).toLowerCase().includes(q) || c.name.toLowerCase().includes(q)) && (!changedOnly || isChanged(c));
  /** A section of composite fields (exposure, white balance) shows while the search names it. */
  const named = (...words: string[]) => !q || words.some((w) => w.includes(q) || q.includes(w));
  const inGroup = (group: ReturnType<typeof groupOf>) => controls.filter((c) => groupOf(c) === group && shown(c));
  const flicker = $derived(inGroup("flicker"));
  /** Capture controls beyond the frame rate (its own field): the sensor's rotation. */
  const capture = $derived(inGroup("capture").filter((c) => c.name !== "frame_rate"));
  const advanced = $derived(inGroup("advanced"));
  const runs = $derived(active ? capabilitiesOf(active.graph) : []);
  const filtering = $derived(Boolean(q) || changedOnly);

  function openJobs() {
    shell.go("cameras");
    workspaces.open("cameras", paneRef("camera-jobs"));
  }

  function resetAll(c: Camera) {
    if (cluster.live) {
      void cluster.live.resetCamera(c.resourceId);
      return;
    }
    // As the device does: each control that has a default goes back to it, forgetting its value.
    const changes: Record<string, null> = {};
    for (const control of c.controls ?? []) if (resetTarget(control) !== null) changes[keyOf(control)] = null;
    writeControls(c, changes);
    toasts.info("Camera reset to defaults");
  }

  function exportConfig(c: Camera) {
    const values = Object.fromEntries(controls.filter((x) => x.writable && x.value !== null).map((x) => [keyOf(x), x.value]));
    downloadJson(`${c.resourceId}.camera.json`, { format: "helios.camera", schema_version: 2, camera: c.resourceId, controls: values, identity: identity.get(c.resourceId) });
  }
  async function importConfig(c: Camera) {
    const doc = await pickJson<{ format?: string; controls?: Record<string, number | boolean> }>();
    if (doc?.format !== "helios.camera" || !doc.controls) return toasts.error("That file is not a HeliOS camera config");
    const writable = new Set((c.controls ?? []).filter((x) => x.writable).map((x) => keyOf(x)));
    const changes = Object.fromEntries(Object.entries(doc.controls).filter(([key, value]) => writable.has(key) && (typeof value === "number" || typeof value === "boolean")));
    writeControls(c, changes);
    toasts.success(`Applied settings to ${c.name}`);
  }
</script>

<PaneBar>
  <Picker
    label="Camera"
    icon="camera"
    value={camera?.resourceId}
    options={cluster.cameras.map((c) => ({ id: c.resourceId, name: identity.name(c.resourceId, c.name) }))}
    onpick={(id) => (target.pinned ? target.pin(id) : selection.select({ kind: "camera", id }))}
    pinned={target.pinned}
    ontogglepin={() => target.toggle()}
  />
  {#if camera}
    <span class="sep"></span>
    <IconButton icon="file-import" label="Import camera settings" size={24} onclick={() => importConfig(camera)} />
    <IconButton icon="file-export" label="Export camera settings" size={24} onclick={() => exportConfig(camera)} />
    <IconButton icon="rotate-clockwise" label="Reset all controls to defaults" size={24} onclick={() => resetAll(camera)} />
  {/if}
</PaneBar>

{#if camera}
  <div class="scroll">
    <IdentityEditor id={camera.resourceId} fallbackName={camera.name} fallbackIcon="camera" sub="{camera.sensor} · {node?.name ?? camera.nodeId}" />
    <div class="stats">
      <span><b>{camera.stats.fps.toFixed(0)}</b> fps</span>
      <span><b>{camera.stats.latencyMs.toFixed(1)}</b> ms</span>
      <span><b>{camera.stats.dropped}</b> dropped</span>
      <span><b>{camera.stats.cpuMsPerFrame.toFixed(2)}</b> ms cpu</span>
      {#if camera.foreign}<Badge tone="warn" text="via {camera.foreign}" />{/if}
    </div>

    <div class="runs">
      <span class="k">Runs</span>
      {#if camera.foreign}
        <span class="muted">Run by {camera.foreign}</span>
      {:else if active}
        <button type="button" class="link" onclick={() => selection.select({ kind: "workload", id: active.id })}>{identity.name(active.id, active.name)}</button>
        <span class="muted">{runs.map((c) => c.replace("_", " ")).join(" · ")}</span>
      {:else}
        <span class="muted">Video only</span>
      {/if}
      <span class="grow"></span>
      {#if !camera.foreign}<button type="button" class="link" onclick={openJobs}>Change</button>{/if}
    </div>
    <div class="tools">
      <input class="search" placeholder="Search camera controls" bind:value={query} aria-label="Search camera controls" />
      <button type="button" class="filter" class:on={changedOnly} aria-pressed={changedOnly} onclick={() => (changedOnly = !changedOnly)}>Changed only</button>
    </div>

    {#if !filtering || capture.length > 0 || named("sensor", "mode", "frame rate", "fps", "capture")}
    <Section title="Sensor" key="cam-capture">
      {#if !filtering || named("sensor", "mode")}
        <Prop label="Sensor mode" help="The sensor mode the camera streams. Read-only until the device lists modes to choose from. Each pipeline asks for its own size in its settings.">
          <span class="mono">{camera.mode ?? `${camera.settings.width}×${camera.settings.height} ${camera.settings.format}`}</span>
        </Prop>
      {/if}
      {#if !filtering || named("frame rate", "fps")}<FrameRateField {camera} />{/if}
      {#each capture as c (c.id)}
        <ControlRow {camera} control={c} />
      {/each}
      {#if camera.controlsError}
        <Prop label="Controls"><span class="warn">{camera.controlsError}</span></Prop>
      {:else if controls.length === 0}
        <Prop label="Controls"><span class="muted">The camera service lists no controls.</span></Prop>
      {/if}
    </Section>
    {/if}

    {#if !changedOnly && named("exposure", "gain", "auto exposure", "ae", "brightness", "apriltag")}
      <Section title="Exposure" key="cam-exposure">
        <ExposureFields {camera} />
        <AprilTagPreset {camera} />
      </Section>
    {/if}

    {#if !changedOnly && named("white balance", "awb", "colour", "color", "temperature")}
      <Section title="White balance" key="cam-white">
        <WhiteBalanceFields {camera} />
      </Section>
    {/if}

    {#if !filtering || flicker.length > 0}
      <Section title="Flicker" key="cam-flicker" open={flicker.length > 0}>
        {#each flicker as c (c.id)}
          <ControlRow {camera} control={c} />
        {:else}
          <Prop label="Flicker"><span class="muted">This camera has no flicker control.</span></Prop>
        {/each}
      </Section>
    {/if}

    {#if !filtering || named("calibration", "lens", "position", "rotation", "mount", "placement")}
      <Section title="Lens and placement" key="cam-lens">
        <Prop label="Calibration" help="Lens intrinsics at the current resolution.">
          {#if cal}
            <span class="mono">{cal.rms.toFixed(3)} px RMS · fx {cal.fx.toFixed(0)}</span>
          {:else}
            <span class="warn">none at {camera.settings.width}×{camera.settings.height}</span>
          {/if}
          <button type="button" class="link" onclick={() => shell.go("calibration")}>{cal ? "Redo" : "Calibrate"}</button>
        </Prop>
        {#if place}
          <Prop label="Position (cm)" help="Robot frame: x forward, y left, z up from the floor at the chassis centre." changed={robot.isCustom(camera.resourceId)} onreset={() => robot.reset(camera.resourceId)}>
            {#each [0, 1, 2] as i (i)}
              <Num value={Math.round(place.pos[i] * 1000) / 10} step={0.5} width={62} label={["x", "y", "z"][i]} onchange={(v) => { const pos = [...place.pos] as [number, number, number]; pos[i] = v / 100; robot.set(camera.resourceId, { pos }); }} />
            {/each}
          </Prop>
          <Prop label="Rotation (°)" help="Roll, pitch (up), yaw (left).">
            <Num value={place.roll} min={-180} max={180} step={0.5} width={62} label="Roll" onchange={(v) => robot.set(camera.resourceId, { roll: v })} />
            <Num value={place.pitch} min={-90} max={90} step={0.5} width={62} label="Pitch" onchange={(v) => robot.set(camera.resourceId, { pitch: v })} />
            <Num value={place.yaw} min={-180} max={180} step={0.5} width={62} label="Yaw" onchange={(v) => robot.set(camera.resourceId, { yaw: v })} />
          </Prop>
        {/if}
      </Section>
    {/if}

    {#if !filtering || advanced.length > 0}
      <Section title="Advanced" key="cam-advanced" open={filtering} count={advanced.length}>
        {#each advanced as c (c.id)}
          <ControlRow {camera} control={c} />
        {:else}
          <Prop label="Controls"><span class="muted">No other controls.</span></Prop>
        {/each}
      </Section>
    {/if}

    <Section title="Device" key="cam-device" open={false}>
      <Prop label="Resource"><span class="mono">{camera.resourceId}</span></Prop>
      <Prop label="Sensor"><span>{camera.sensor}</span></Prop>
      <Prop label="Backend"><span>{camera.backend}</span></Prop>
      <Prop label="Mount"><span>{camera.mount}</span></Prop>
      <Prop label="Host"><span>{node?.name} · {node?.address}</span></Prop>
    </Section>
  </div>
{:else}
  <Empty icon="camera" text="Select a camera to see its controls." />
{/if}

<style>
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .stats {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 12px;
    padding: 0 10px 8px 48px;
    font-size: 11.5px;
    color: var(--fg-3);
    border-bottom: 1px solid var(--line);
  }
  .stats b {
    color: var(--fg);
    font-family: var(--font-code);
    font-weight: 600;
  }
  .runs {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px 8px;
    padding: 6px 10px;
    font-size: 11.5px;
    border-bottom: 1px solid var(--line);
  }
  .runs .k {
    color: var(--fg-3);
  }
  .grow {
    flex: 1;
  }
  .tools {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 6px 10px;
  }
  .search {
    flex: 1;
    min-width: 120px;
    height: 24px;
    padding: 0 8px;
    font: inherit;
    font-size: 12px;
    color: var(--fg);
    background: var(--inset);
    border: 1px solid var(--line);
    border-radius: var(--r-1);
    outline: none;
  }
  .search:focus {
    border-color: var(--accent-ring);
  }
  .filter {
    height: 24px;
    padding: 0 8px;
    font-size: 11.5px;
    color: var(--fg-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
    white-space: nowrap;
  }
  .filter.on {
    color: var(--accent-fg);
    border-color: var(--accent-ring);
    background: color-mix(in oklab, var(--accent) 12%, transparent);
  }
  .link {
    font-size: 11.5px;
    color: var(--accent-fg);
    white-space: nowrap;
  }
  .link:hover {
    text-decoration: underline;
  }
  .warn {
    font-size: 11.5px;
    color: var(--warn);
  }
  .muted {
    font-size: 11.5px;
    color: var(--fg-3);
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11px;
    color: var(--fg-2);
  }
</style>
