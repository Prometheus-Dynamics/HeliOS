<script lang="ts">
  // A camera's settings, grouped the way a person thinks about them: capture, exposure, white
  // balance, flicker, then advanced. Everything comes from the camera service's control list,
  // so the mock and a device show the same controls. The fields are shared with the inspector.
  import type { Camera } from "#lib/api/model.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import Badge from "#lib/kit/Badge.svelte";
  import Choice from "#lib/kit/Choice.svelte";
  import IconButton from "#lib/kit/IconButton.svelte";
  import IdentityEditor from "#lib/kit/IdentityEditor.svelte";
  import Num from "#lib/kit/Num.svelte";
  import Picker from "#lib/kit/Picker.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import Section from "#lib/kit/Section.svelte";
  import Seg from "#lib/kit/Seg.svelte";
  import Switch from "#lib/kit/Switch.svelte";
  import AprilTagPreset from "#lib/camera/AprilTagPreset.svelte";
  import ControlRow from "#lib/camera/ControlRow.svelte";
  import ExposureFields from "#lib/camera/ExposureFields.svelte";
  import FrameRateField from "#lib/camera/FrameRateField.svelte";
  import WhiteBalanceFields from "#lib/camera/WhiteBalanceFields.svelte";
  import { groupOf, resetTarget } from "#lib/camera/controls.js";
  import { keyOf, writeControls } from "#lib/camera/write.js";
  import { identity } from "#lib/core/identity.svelte.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { calibration } from "#lib/stores/calibration.svelte.js";
  import { robot } from "#lib/stores/robot.svelte.js";
  import { DICTIONARIES, guide } from "#lib/core/guide.svelte.js";
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
  const inGroup = (group: ReturnType<typeof groupOf>) => controls.filter((c) => groupOf(c) === group);
  const flicker = $derived(inGroup("flicker"));
  const advanced = $derived(inGroup("advanced"));
  const crops = $derived(inGroup("crop"));
  let showCrop = $state(false);

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

    <Section title="Detection" key="cam-detect">
      <Prop label="Job" help="What runs on this camera. Each choice is a pipeline you can open and edit.">
        {#if camera.foreign}
          <span class="muted">Run by {camera.foreign}</span>
        {:else}
          <Seg label="Job" value={guide.job(camera)} options={[{ value: "apriltag", label: "AprilTag" }, { value: "aruco", label: "ArUco" }, { value: "view", label: "Video only" }]} onchange={(v) => guide.setJob(camera, v)} />
        {/if}
      </Prop>
      {#if !camera.foreign && guide.job(camera) !== "view"}
        {@const job = guide.job(camera) as "apriltag" | "aruco"}
        <Prop label="Tag family" help="The kind of tags to look for. A family other than the tags' own finds nothing at all.">
          <Choice label="Tag family" value={guide.dictionary(camera)} options={DICTIONARIES[job]} onchange={(v) => guide.setDictionary(camera, v)} />
        </Prop>
      {/if}
      {#if active}
        <Prop label="Pipeline">
          <button type="button" class="link" onclick={() => { selection.select({ kind: "workload", id: active.id }); shell.go("pipelines"); }}>{identity.name(active.id, active.name)} · r{active.revision} · {active.perf.tickP50Ms.toFixed(2)} ms</button>
        </Prop>
        <Prop label="NetworkTables"><span class="mono">{guide.table(camera) ?? "—"}</span></Prop>
      {/if}
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

    <Section title="Capture" key="cam-capture">
      <Prop label="Sensor mode" help="The sensor mode the camera streams. Read-only until the device lists modes to choose from.">
        <span class="mono">{camera.mode ?? "not reported"}</span>
      </Prop>
      <Prop label="Pipeline output" help="Resolution, format, pyramid and region are set per pipeline (its camera binding), not on the camera.">
        <span class="mono">{camera.settings.width}×{camera.settings.height} {camera.settings.format}</span>
      </Prop>
      <FrameRateField {camera} />
      {#if camera.controlsError}
        <Prop label="Controls"><span class="warn">{camera.controlsError}</span></Prop>
      {:else if controls.length === 0}
        <Prop label="Controls"><span class="muted">The camera service lists no controls.</span></Prop>
      {/if}
    </Section>

    <Section title="Exposure" key="cam-exposure">
      <ExposureFields {camera} />
      <AprilTagPreset {camera} />
    </Section>

    <Section title="White balance" key="cam-white">
      <WhiteBalanceFields {camera} />
    </Section>

    <Section title="Flicker" key="cam-flicker" open={flicker.length > 0}>
      {#each flicker as c (c.id)}
        <ControlRow {camera} control={c} />
      {:else}
        <Prop label="Flicker"><span class="muted">This camera has no flicker control.</span></Prop>
      {/each}
    </Section>

    <Section title="Advanced" key="cam-advanced" open={false} count={advanced.length}>
      {#each advanced as c (c.id)}
        <ControlRow {camera} control={c} />
      {:else}
        <Prop label="Controls"><span class="muted">No other controls.</span></Prop>
      {/each}
      {#if crops.length > 0}
        <Prop label="Crop regions" help="Output and region crops, in frame pixels. Set per pipeline; shown here for reference.">
          <Switch checked={showCrop} label="Show crop regions" onchange={(v) => (showCrop = v)} />
        </Prop>
        {#if showCrop}
          {#each crops as c (c.id)}
            <ControlRow {camera} control={c} locked="Set per pipeline; not editable here" />
          {/each}
        {/if}
      {/if}
    </Section>

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
