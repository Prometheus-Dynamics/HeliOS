<script lang="ts">
  // Auto or manual exposure, with the exposure (ms, on a log scale) and gain. The ranges are the
  // ones the camera takes now (its effective range when it reports one, else one frame at the
  // frame rate within the device's limits). Auto shows the live values, locked. The pane adds EV,
  // the AE state, the AE limits (motion blur, gain), the exposure and metering modes and the
  // metering region; the inspector shows only the basics.
  import type { Camera, CameraControlInfo } from "#lib/api/model.js";
  import Prop from "#lib/kit/Prop.svelte";
  import Seg from "#lib/kit/Seg.svelte";
  import Slider from "#lib/kit/Slider.svelte";
  import Switch from "#lib/kit/Switch.svelte";
  import ControlRow from "./ControlRow.svelte";
  import { clampExposure, disabledReason, EXPOSURE_FALLBACK, formatMs, limitOn, maxExposureUs, rangeOr, rectText } from "./controls.js";
  import { controlNamed, keyOf, writeControls } from "./write.js";

  let { camera, compact = false }: { camera: Camera; compact?: boolean } = $props();

  const controls = $derived(camera.controls ?? []);
  const ae = $derived(controlNamed(controls, "ae_enable"));
  const exposure = $derived(controlNamed(controls, "exposure_time_us"));
  const gain = $derived(controlNamed(controls, "gain"));
  const frame = $derived(controlNamed(controls, "frame_rate"));
  const ev = $derived(controlNamed(controls, "exposure_value"));
  const aeState = $derived(controlNamed(controls, "ae_state"));
  const maxExposureLimit = $derived(controlNamed(controls, "ae_exposure_max_us"));
  const maxGainLimit = $derived(controlNamed(controls, "ae_gain_max"));
  const exposureMode = $derived(controlNamed(controls, "ae_exposure_mode"));
  const meteringMode = $derived(controlNamed(controls, "ae_metering_mode"));
  const meteringRegion = $derived(controlNamed(controls, "ae_metering_region"));

  const auto = $derived(ae?.value === true);
  const fps = $derived(typeof frame?.value === "number" ? frame.value : camera.settings.fps);
  const range = $derived(rangeOr(exposure, EXPOSURE_FALLBACK));
  const gainRange = $derived(rangeOr(gain, { min: 1, max: 16 }));
  const maxUs = $derived(maxExposureUs(range, fps));
  const exposureUs = $derived(typeof exposure?.value === "number" ? exposure.value : null);
  const gainValue = $derived(typeof gain?.value === "number" ? gain.value : null);

  // The AE limits: 0 is no limit. Turning one on takes a sensible value (one frame; the gain's
  // device maximum), and the slider moves it between the device's bounds.
  // A log scale needs a positive floor: the device lists 0 (no limit) as its minimum.
  const exposureCapRange = $derived({ ...rangeOr(maxExposureLimit, EXPOSURE_FALLBACK), min: Math.max(EXPOSURE_FALLBACK.min, maxExposureLimit?.min ?? 0) });
  const gainCapRange = $derived(rangeOr(maxGainLimit, { min: 1, max: 16 }));
  const exposureCapOn = $derived(limitOn(maxExposureLimit?.value));
  const gainCapOn = $derived(limitOn(maxGainLimit?.value));
  const region = $derived(meteringRegion?.rect && !Array.isArray(meteringRegion.rect) ? meteringRegion.rect : null);
  const regionSet = $derived(!!region && region.width > 0 && region.height > 0);

  const modeReason = $derived(ae ? disabledReason(ae) : "This camera has no auto exposure control");
  const exposureLock = $derived(!exposure ? "This camera has no exposure control" : auto ? "Auto exposure is on: the camera sets it" : disabledReason(exposure));
  const gainLock = $derived(!gain ? "This camera has no gain control" : auto ? "Auto exposure is on: the camera sets it" : disabledReason(gain));

  function setMode(value: "auto" | "manual") {
    if (ae && !modeReason) writeControls(camera, { [keyOf(ae)]: value === "auto" });
  }
  function setExposureMs(ms: number) {
    if (exposure) writeControls(camera, { [keyOf(exposure)]: clampExposure(ms * 1000, range, fps) });
  }
  function setGain(value: number) {
    if (gain) writeControls(camera, { [keyOf(gain)]: value });
  }
  function setLimit(control: CameraControlInfo, value: number) {
    writeControls(camera, { [keyOf(control)]: value });
  }
  /** Metering: setting the metering mode (to the one it has) clears the metering region. */
  function clearRegion() {
    if (meteringMode) writeControls(camera, { [keyOf(meteringMode)]: Number(meteringMode.value ?? 0) });
  }
</script>

<Prop label="Mode" help="Auto: the camera sets exposure and gain every frame. Manual: you fix them. Short manual exposures stop motion blur on a moving robot.">
  <Seg label="Exposure mode" value={auto ? "auto" : "manual"} options={[{ value: "auto", label: "Auto" }, { value: "manual", label: "Manual" }]} onchange={setMode} />
  {#if modeReason && !ae}<span class="why">{modeReason}</span>{/if}
</Prop>

{#if exposure}
  <Prop label={auto ? "Exposure (live)" : "Exposure"} help="Shutter time. The longest it can be is one frame at the current frame rate, or the max exposure limit below.">
    <div class="field" inert={!!exposureLock}>
      <Slider
        value={(exposureUs ?? range.min) / 1000}
        min={range.min / 1000}
        max={maxUs / 1000}
        step={0.01}
        unit="ms"
        log
        label="Exposure"
        disabled={!!exposureLock}
        onchange={setExposureMs}
      />
      <span class="hint">max {formatMs(maxUs)} ms at {Number(fps.toFixed(1))} fps</span>
    </div>
    {#if exposureLock && auto}<span class="why">{exposureLock}</span>{/if}
  </Prop>
{/if}

{#if gain}
  <Prop label={auto ? "Gain (live)" : "Gain"} help="Brightens the image; more gain also adds noise.">
    <Slider value={gainValue ?? 1} min={gainRange.min} max={gainRange.max} step={gain.step ?? 0.01} unit="×" label="Gain" disabled={!!gainLock} onchange={setGain} />
  </Prop>
{/if}

{#if !compact}
  {#if maxExposureLimit}
    <Prop
      label="Max exposure"
      help="Motion blur limit (ae_exposure_max_us): auto exposure never exposes longer than this, and a manual exposure is clamped to it. Off: no limit."
    >
      <Switch checked={exposureCapOn} label="Limit exposure" disabled={!!disabledReason(maxExposureLimit)} onchange={(on) => setLimit(maxExposureLimit, on ? Math.round(maxUs) : 0)} />
      {#if exposureCapOn}
        <Slider
          value={(maxExposureLimit?.value as number) / 1000}
          min={exposureCapRange.min / 1000}
          max={exposureCapRange.max / 1000}
          step={0.01}
          unit="ms"
          log
          label="Max exposure"
          disabled={!!disabledReason(maxExposureLimit)}
          onchange={(ms) => setLimit(maxExposureLimit, clampExposure(ms * 1000, exposureCapRange, null))}
        />
      {/if}
      <span class="hint">{exposureCapOn ? `at most ${formatMs((maxExposureLimit?.value as number) ?? 0)} ms` : "no limit"}</span>
    </Prop>
  {/if}
  {#if maxGainLimit}
    <Prop label="Max gain" help="ae_gain_max: the most total gain (analogue and digital) auto exposure uses. Off: no limit.">
      <Switch checked={gainCapOn} label="Limit gain" disabled={!!disabledReason(maxGainLimit)} onchange={(on) => setLimit(maxGainLimit, on ? gainCapRange.max : 0)} />
      {#if gainCapOn}
        <Slider
          value={(maxGainLimit?.value as number) ?? gainCapRange.max}
          min={Math.max(1, gainCapRange.min)}
          max={gainCapRange.max}
          step={0.01}
          unit="×"
          label="Max gain"
          disabled={!!disabledReason(maxGainLimit)}
          onchange={(g) => setLimit(maxGainLimit, g)}
        />
      {/if}
    </Prop>
  {/if}
  {#if aeState}
    <ControlRow {camera} control={aeState} />
  {/if}
  {#if ev}
    <ControlRow {camera} control={ev} locked={auto ? undefined : "Only used by auto exposure"} />
  {/if}
  {#if exposureMode}
    <ControlRow {camera} control={exposureMode} locked={auto ? undefined : "Only used by auto exposure"} />
  {/if}
  {#if meteringMode}
    <ControlRow {camera} control={meteringMode} locked={auto ? undefined : "Only used by auto exposure"} />
  {/if}
  {#if meteringRegion}
    <Prop label="Metering region" help="The part of the frame auto exposure meters. Drawn with the camera view's region tool (Meter exposure here); a metering mode clears it.">
      <span class="value">{rectText(meteringRegion.rect)}</span>
      {#if regionSet && meteringMode}
        <button type="button" class="clear" onclick={clearRegion}>Clear metering region</button>
      {/if}
    </Prop>
  {/if}
{/if}

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: 3px;
    width: 100%;
    min-width: 0;
  }
  .hint {
    font-size: 10.5px;
    color: var(--fg-3);
    font-family: var(--font-code);
  }
  .why {
    flex-basis: 100%;
    font-size: 11px;
    color: var(--fg-3);
  }
  .value {
    font-family: var(--font-code);
    font-size: 11.5px;
    color: var(--fg-2);
    white-space: nowrap;
  }
  .clear {
    font-size: 11.5px;
    color: var(--accent-fg);
    white-space: nowrap;
  }
  .clear:hover {
    text-decoration: underline;
  }
</style>
