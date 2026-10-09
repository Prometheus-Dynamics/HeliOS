<script lang="ts">
  // Every camera control, live. The everyday ones are open; sensor, ISP,
  // region and transport sit in closed "advanced" sections right below.
  import type { Camera, CameraControlInfo, CameraSettings } from "#lib/api/model.js";
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
  import Slider from "#lib/kit/Slider.svelte";
  import Switch from "#lib/kit/Switch.svelte";
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
  // Live mode: the controls come from the camera's Styx camera service. Settings with no camera
  // control behind them (sensor, ISP, transport, which the service plans per client) are hidden;
  // resolution, format, pyramid and region are set per pipeline.
  const live = !!cluster.live;
  const controls = $derived(camera?.controls ?? []);
  const std = (name: string): CameraControlInfo | undefined => controls.find((c) => c.standard === name);
  const range = (c: CameraControlInfo | undefined, min: number, max: number) => ({ min: c?.min ?? min, max: c?.max ?? max });
  const stepOf = (c: CameraControlInfo) => c.step ?? (c.kind === "float" ? Math.max(((c.max ?? 1) - (c.min ?? 0)) / 200, 0.001) : 1);
  const sendControl = (c: CameraControlInfo, value: number | boolean) => camera && cluster.setCamera(camera.resourceId, {}, { [c.standard ?? c.name]: value } as Record<string, number | boolean>);

  const BASE: CameraSettings = { width: 1280, height: 800, fps: 60, format: "GREY", exposureUs: 2200, autoExposure: false, gain: 4, pyramid: true, roi: null };
  const EXTRA: Record<string, number | string | boolean> = {
    brightness: 0,
    contrast: 1,
    sharpness: 1,
    denoise: "fast",
    awb: "auto",
    colourTemp: 5000,
    hflip: false,
    vflip: false,
    rotation: 0,
    analogueGainMax: 16,
    exposureMaxUs: 8000,
    meter: "centre",
    buffers: 4,
    transport: "dmabuf",
    timestamp: "sensor",
    queue: "latest",
    flash: "off",
    sensorMode: "full",
    analogueGain: 4,
    digitalGain: 1,
    blackLevel: 64,
    hdr: false,
    lensShading: true,
    testPattern: "off",
    frameMinUs: 8333,
    frameMaxUs: 33333,
    gamma: 2.2,
    saturation: 1,
  };
  const MODES = [
    { value: "1280x800", label: "1280 × 800" },
    { value: "640x400", label: "640 × 400" },
    { value: "1920x1080", label: "1920 × 1080" },
  ];

  function set(patch: Partial<CameraSettings>) {
    if (camera) cluster.setCamera(camera.resourceId, patch);
  }
  function x<T extends number | string | boolean>(name: string): T {
    return (camera?.extra?.[name] ?? EXTRA[name]) as T;
  }
  function setX(name: string, value: number | string | boolean) {
    if (camera) cluster.setCamera(camera.resourceId, {}, { [name]: value });
  }
  const changed = (name: keyof CameraSettings) => camera && JSON.stringify(camera.settings[name]) !== JSON.stringify(BASE[name]);
  const changedX = (name: string) => camera?.extra?.[name] !== undefined && camera.extra[name] !== EXTRA[name];

  function exportConfig(c: Camera) {
    downloadJson(`${c.resourceId}.camera.json`, { format: "helios.camera", schema_version: 1, camera: c.resourceId, settings: c.settings, extra: c.extra ?? {}, identity: identity.get(c.resourceId) });
  }
  async function importConfig(c: Camera) {
    const doc = await pickJson<{ format?: string; settings?: CameraSettings; extra?: Record<string, number | string | boolean> }>();
    if (doc?.format !== "helios.camera" || !doc.settings) return toasts.error("That file is not a HeliOS camera config");
    cluster.setCamera(c.resourceId, doc.settings, doc.extra);
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
    <IconButton icon="rotate-clockwise" label="Reset all to defaults" size={24} onclick={() => { if (cluster.live) { void cluster.live.resetCamera(camera.resourceId); return; } cluster.setCamera(camera.resourceId, { ...BASE }); camera.extra = {}; toasts.info("Camera reset to defaults"); }} />
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
      {#if live}
      <Prop label="Mode" help="What the camera service captures for its clients. Resolution, format and pyramid are set per pipeline (its camera binding).">
        <span class="mono">{camera.settings.width}×{camera.settings.height} {camera.settings.format}</span>
      </Prop>
      {@const fps = std("fps")}
      <Prop label="Frame rate" help={fps ? "Where the camera cannot change it while streaming, the camera service restarts the capture for every client." : "This camera has no frame rate control."}>
        <Slider value={camera.settings.fps} {...range(fps, 1, 120)} unit="fps" label="Frame rate" disabled={!fps?.writable} onchange={(v) => set({ fps: v })} />
      </Prop>
      {#if camera.controlsError}
        <Prop label="Controls"><span class="warn">{camera.controlsError}</span></Prop>
      {/if}
      {:else}
      <Prop label="Mode" changed={changed("width")} onreset={() => set({ width: BASE.width, height: BASE.height })}>
        <Choice
          label="Resolution"
          value={`${camera.settings.width}x${camera.settings.height}`}
          options={MODES}
          onchange={(v) => {
            const [w, h] = v.split("x").map(Number);
            set({ width: w, height: h, roi: null });
          }}
        />
      </Prop>
      <Prop label="Frame rate" changed={changed("fps")} onreset={() => set({ fps: BASE.fps })}>
        <Slider value={camera.settings.fps} min={1} max={120} unit="fps" label="Frame rate" onchange={(v) => set({ fps: v })} />
      </Prop>
      <Prop label="Format" changed={changed("format")} onreset={() => set({ format: BASE.format })} help="GREY is the sensor's 8-bit luma; tag detection needs nothing else.">
        <Seg label="Pixel format" value={camera.settings.format} options={[{ value: "GREY", label: "GREY" }, { value: "NV12", label: "NV12" }, { value: "MJPEG", label: "MJPEG" }]} onchange={(v) => set({ format: v })} />
      </Prop>
      {/if}
    </Section>

    <Section title="Exposure" key="cam-exposure">
      {@const ae = std("ae")}
      {@const exposure = std("exposure_us")}
      {@const gain = std("gain")}
      <Prop label="Auto exposure" changed={!live && changed("autoExposure")} onreset={live ? undefined : () => set({ autoExposure: BASE.autoExposure })}>
        <Switch checked={camera.settings.autoExposure} label="Auto exposure" disabled={live && !ae?.writable} onchange={(v) => set({ autoExposure: v })} />
      </Prop>
      <Prop label="Exposure" changed={!live && changed("exposureUs")} onreset={live ? undefined : () => set({ exposureUs: BASE.exposureUs })} help="Short exposures stop motion blur on a moving robot.">
        <Slider value={camera.settings.exposureUs} {...(live ? range(exposure, 10, 33000) : { min: 50, max: 16000 })} step={live ? 10 : 50} unit="µs" label="Exposure" disabled={camera.settings.autoExposure || (live && !exposure?.writable)} onchange={(v) => set({ exposureUs: v })} />
      </Prop>
      <Prop label="Gain" changed={!live && changed("gain")} onreset={live ? undefined : () => set({ gain: BASE.gain })}>
        <Slider value={camera.settings.gain} {...(live ? range(gain, 1, 16) : { min: 1, max: 16 })} step={0.1} unit="×" label="Gain" disabled={camera.settings.autoExposure || (live && !gain?.writable)} onchange={(v) => set({ gain: v })} />
      </Prop>
      {#if !live}
      <Prop label="Brightness" changed={changedX("brightness")} onreset={() => setX("brightness", EXTRA.brightness)}>
        <Slider value={x<number>("brightness")} min={-1} max={1} step={0.05} label="Brightness" onchange={(v) => setX("brightness", v)} />
      </Prop>
      <Prop label="Contrast" changed={changedX("contrast")} onreset={() => setX("contrast", EXTRA.contrast)}>
        <Slider value={x<number>("contrast")} min={0} max={3} step={0.05} label="Contrast" onchange={(v) => setX("contrast", v)} />
      </Prop>
      {/if}
    </Section>

    {#if live}
    <Section title="All camera controls" key="cam-device-controls" count={controls.length}>
      {#each controls as c (c.id)}
        <Prop label={c.name} help={c.standard ? `Standard control: ${c.standard}` : undefined} changed={c.writable && c.default !== null && c.value !== null && c.value !== c.default} onreset={c.writable && c.default !== null ? () => sendControl(c, c.default as number | boolean) : undefined}>
          {#if c.kind === "bool"}
            <Switch checked={c.value === true} label={c.name} disabled={!c.writable} onchange={(v) => sendControl(c, v)} />
          {:else if (c.kind === "menu" || c.kind === "int_menu") && c.menu}
            <Choice label={c.name} value={String(c.value ?? 0)} options={c.menu.map((label, i) => ({ value: String(i), label }))} disabled={!c.writable} onchange={(v) => sendControl(c, Number(v))} />
          {:else if (c.kind === "int" || c.kind === "uint" || c.kind === "float") && typeof c.value === "number"}
            {#if c.min !== undefined && c.max !== undefined && c.max > c.min}
              <Slider value={c.value} min={c.min} max={c.max} step={stepOf(c)} label={c.name} disabled={!c.writable} onchange={(v) => sendControl(c, v)} />
            {:else}
              <Num value={c.value} step={stepOf(c)} label={c.name} disabled={!c.writable} onchange={(v) => sendControl(c, v)} />
            {/if}
          {:else}
            <span class="muted">{c.value === null ? "not readable" : String(c.value)}{c.writable ? "" : " · read only"}</span>
          {/if}
        </Prop>
      {:else}
        <Prop label="Controls"><span class="muted">{camera.controlsError ?? "The camera service lists no controls."}</span></Prop>
      {/each}
    </Section>
    {:else}

    <Section title="Sensor" key="cam-sensor" advanced>
      <Prop label="Sensor mode" help="Binning trades resolution for light and speed." changed={changedX("sensorMode")} onreset={() => setX("sensorMode", EXTRA.sensorMode)}>
        <Seg label="Sensor mode" value={x<string>("sensorMode")} options={[{ value: "full", label: "Full" }, { value: "bin2", label: "2×2 bin" }]} onchange={(v) => setX("sensorMode", v)} />
      </Prop>
      <Prop label="Analogue gain" changed={changedX("analogueGain")} onreset={() => setX("analogueGain", EXTRA.analogueGain)}>
        <Slider value={x<number>("analogueGain")} min={1} max={16} step={0.1} unit="×" label="Analogue gain" onchange={(v) => setX("analogueGain", v)} />
      </Prop>
      <Prop label="Digital gain" changed={changedX("digitalGain")} onreset={() => setX("digitalGain", EXTRA.digitalGain)}>
        <Slider value={x<number>("digitalGain")} min={1} max={4} step={0.05} unit="×" label="Digital gain" onchange={(v) => setX("digitalGain", v)} />
      </Prop>
      <Prop label="Black level" changed={changedX("blackLevel")} onreset={() => setX("blackLevel", EXTRA.blackLevel)}>
        <Num value={x<number>("blackLevel")} min={0} max={255} label="Black level" onchange={(v) => setX("blackLevel", v)} />
      </Prop>
      <Prop label="Frame duration" help="Min and max frame time; caps exposure and sets the frame-rate range." changed={changedX("frameMinUs") || changedX("frameMaxUs")} onreset={() => { setX("frameMinUs", EXTRA.frameMinUs); setX("frameMaxUs", EXTRA.frameMaxUs); }}>
        <Num value={x<number>("frameMinUs")} min={1000} max={1000000} step={100} unit="µs" width={86} label="Min frame" onchange={(v) => setX("frameMinUs", v)} />
        <Num value={x<number>("frameMaxUs")} min={1000} max={1000000} step={100} unit="µs" width={86} label="Max frame" onchange={(v) => setX("frameMaxUs", v)} />
      </Prop>
      <Prop label="HDR" changed={changedX("hdr")} onreset={() => setX("hdr", EXTRA.hdr)}>
        <Switch checked={x<boolean>("hdr")} label="HDR" onchange={(v) => setX("hdr", v)} />
      </Prop>
      <Prop label="Lens shading" changed={changedX("lensShading")} onreset={() => setX("lensShading", EXTRA.lensShading)}>
        <Switch checked={x<boolean>("lensShading")} label="Lens shading correction" onchange={(v) => setX("lensShading", v)} />
      </Prop>
      <Prop label="Test pattern" changed={changedX("testPattern")} onreset={() => setX("testPattern", EXTRA.testPattern)}>
        <Choice label="Test pattern" value={x<string>("testPattern")} options={["off", "colour bars", "solid", "gradient", "pn9"]} onchange={(v) => setX("testPattern", v)} />
      </Prop>
    </Section>

    <Section title="Auto-exposure limits" key="cam-ae" advanced>
      <Prop label="Metering" changed={changedX("meter")} onreset={() => setX("meter", EXTRA.meter)}>
        <Seg label="Metering" value={x<string>("meter")} options={[{ value: "centre", label: "Centre" }, { value: "spot", label: "Spot" }, { value: "matrix", label: "Matrix" }, { value: "roi", label: "ROI" }]} onchange={(v) => setX("meter", v)} />
      </Prop>
      <Prop label="Max exposure" changed={changedX("exposureMaxUs")} onreset={() => setX("exposureMaxUs", EXTRA.exposureMaxUs)}>
        <Num value={x<number>("exposureMaxUs")} min={100} max={33000} step={100} unit="µs" width={90} label="Max exposure" onchange={(v) => setX("exposureMaxUs", v)} />
      </Prop>
      <Prop label="Max analogue gain" changed={changedX("analogueGainMax")} onreset={() => setX("analogueGainMax", EXTRA.analogueGainMax)}>
        <Num value={x<number>("analogueGainMax")} min={1} max={16} step={0.5} unit="×" width={80} label="Max analogue gain" onchange={(v) => setX("analogueGainMax", v)} />
      </Prop>
    </Section>

    <Section title="Image processing (ISP)" key="cam-isp" advanced>
      <Prop label="Sharpness" changed={changedX("sharpness")} onreset={() => setX("sharpness", EXTRA.sharpness)}>
        <Slider value={x<number>("sharpness")} min={0} max={4} step={0.1} label="Sharpness" onchange={(v) => setX("sharpness", v)} />
      </Prop>
      <Prop label="Denoise" changed={changedX("denoise")} onreset={() => setX("denoise", EXTRA.denoise)}>
        <Seg label="Denoise" value={x<string>("denoise")} options={[{ value: "off", label: "Off" }, { value: "fast", label: "Fast" }, { value: "hq", label: "HQ" }]} onchange={(v) => setX("denoise", v)} />
      </Prop>
      <Prop label="White balance" changed={changedX("awb")} onreset={() => setX("awb", EXTRA.awb)}>
        <Choice label="White balance" value={x<string>("awb")} options={["auto", "daylight", "cloudy", "tungsten", "fluorescent", "manual"]} onchange={(v) => setX("awb", v)} />
      </Prop>
      <Prop label="Colour temp" changed={changedX("colourTemp")} onreset={() => setX("colourTemp", EXTRA.colourTemp)}>
        <Slider value={x<number>("colourTemp")} min={2500} max={9000} step={100} unit="K" disabled={x("awb") !== "manual"} label="Colour temperature" onchange={(v) => setX("colourTemp", v)} />
      </Prop>
      <Prop label="Flip">
        <span class="row-gap">
          <label class="mini"><Switch checked={x<boolean>("hflip")} label="Horizontal flip" onchange={(v) => setX("hflip", v)} /> H</label>
          <label class="mini"><Switch checked={x<boolean>("vflip")} label="Vertical flip" onchange={(v) => setX("vflip", v)} /> V</label>
        </span>
      </Prop>
      <Prop label="Gamma" changed={changedX("gamma")} onreset={() => setX("gamma", EXTRA.gamma)}>
        <Slider value={x<number>("gamma")} min={1} max={3} step={0.05} label="Gamma" onchange={(v) => setX("gamma", v)} />
      </Prop>
      <Prop label="Saturation" changed={changedX("saturation")} onreset={() => setX("saturation", EXTRA.saturation)}>
        <Slider value={x<number>("saturation")} min={0} max={2} step={0.05} label="Saturation" onchange={(v) => setX("saturation", v)} />
      </Prop>
      <Prop label="Rotation" changed={changedX("rotation")} onreset={() => setX("rotation", 0)}>
        <Seg label="Rotation" value={String(x<number>("rotation"))} options={["0", "90", "180", "270"].map((v) => ({ value: v, label: `${v}°` }))} onchange={(v) => setX("rotation", Number(v))} />
      </Prop>
    </Section>

    <Section title="Region and pyramid" key="cam-roi" advanced>
      <Prop label="Half-size plane" help="The ISP writes a half-size companion plane for free; detectors run on it." changed={changed("pyramid")} onreset={() => set({ pyramid: BASE.pyramid })}>
        <Switch checked={camera.settings.pyramid} label="Half-size plane" onchange={(v) => set({ pyramid: v })} />
      </Prop>
      {#if camera.settings.roi}
        {@const r = camera.settings.roi}
        <Prop label="ROI origin" changed onreset={() => set({ roi: null })}>
          <Num value={r.x} min={0} max={camera.settings.width} step={2} label="ROI x" width={64} onchange={(v) => set({ roi: { ...r, x: v } })} />
          <Num value={r.y} min={0} max={camera.settings.height} step={2} label="ROI y" width={64} onchange={(v) => set({ roi: { ...r, y: v } })} />
        </Prop>
        <Prop label="ROI size">
          <Num value={r.w} min={16} max={camera.settings.width} step={2} label="ROI width" width={64} onchange={(v) => set({ roi: { ...r, w: v } })} />
          <Num value={r.h} min={16} max={camera.settings.height} step={2} label="ROI height" width={64} onchange={(v) => set({ roi: { ...r, h: v } })} />
        </Prop>
      {:else}
        <Prop label="ROI"><span class="muted">Full frame · draw one on the camera view</span></Prop>
      {/if}
    </Section>

    <Section title="Transport" key="cam-transport" advanced>
      <Prop label="Buffers" help="Frames in flight between the camera and consumers." changed={changedX("buffers")} onreset={() => setX("buffers", EXTRA.buffers)}>
        <Num value={x<number>("buffers")} min={2} max={16} label="Buffers" onchange={(v) => setX("buffers", v)} />
      </Prop>
      <Prop label="Delivery" help="dmabuf hands frames over without copying; shm copies once." changed={changedX("transport")} onreset={() => setX("transport", EXTRA.transport)}>
        <Seg label="Delivery" value={x<string>("transport")} options={[{ value: "dmabuf", label: "dmabuf" }, { value: "shm", label: "shm" }, { value: "net", label: "network" }]} onchange={(v) => setX("transport", v)} />
      </Prop>
      <Prop label="Slow consumers" changed={changedX("queue")} onreset={() => setX("queue", EXTRA.queue)}>
        <Seg label="Slow consumers" value={x<string>("queue")} options={[{ value: "latest", label: "Get latest" }, { value: "queue", label: "Queue" }]} onchange={(v) => setX("queue", v)} />
      </Prop>
      <Prop label="Timestamps" changed={changedX("timestamp")} onreset={() => setX("timestamp", EXTRA.timestamp)}>
        <Seg label="Timestamps" value={x<string>("timestamp")} options={[{ value: "sensor", label: "Sensor" }, { value: "kernel", label: "Kernel" }]} onchange={(v) => setX("timestamp", v)} />
      </Prop>
      <Prop label="Flash sync" changed={changedX("flash")} onreset={() => setX("flash", EXTRA.flash)}>
        <Seg label="Flash sync" value={x<string>("flash")} options={[{ value: "off", label: "Off" }, { value: "strobe", label: "Strobe" }, { value: "trigger", label: "Ext trigger" }]} onchange={(v) => setX("flash", v)} />
      </Prop>
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
  .row-gap {
    display: flex;
    gap: 14px;
  }
  .mini {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 11.5px;
    color: var(--fg-2);
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
