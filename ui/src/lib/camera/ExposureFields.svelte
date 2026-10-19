<script lang="ts">
  // Auto or manual exposure, with the exposure (ms, on a log scale, clamped to one frame at the
  // current frame rate) and gain. Auto shows the live values, locked. The pane adds EV, the AE
  // state and a device's max-exposure limit (motion blur); the inspector shows only the basics.
  import type { Camera } from "#lib/api/model.js";
  import Prop from "#lib/kit/Prop.svelte";
  import Seg from "#lib/kit/Seg.svelte";
  import Slider from "#lib/kit/Slider.svelte";
  import ControlRow from "./ControlRow.svelte";
  import { clampExposure, disabledReason, EXPOSURE_FALLBACK, formatMs, isMaxExposureName, maxExposureUs } from "./controls.js";
  import { controlNamed, keyOf, writeControls } from "./write.js";

  let { camera, compact = false }: { camera: Camera; compact?: boolean } = $props();

  const controls = $derived(camera.controls ?? []);
  const ae = $derived(controlNamed(controls, "ae_enable"));
  const exposure = $derived(controlNamed(controls, "exposure_time_us"));
  const gain = $derived(controlNamed(controls, "gain"));
  const frame = $derived(controlNamed(controls, "frame_rate"));
  const ev = $derived(controlNamed(controls, "exposure_value"));
  const aeState = $derived(controlNamed(controls, "ae_state"));
  const maxLimit = $derived(controls.find((c) => isMaxExposureName(c.name)));

  const auto = $derived(ae?.value === true);
  const fps = $derived(typeof frame?.value === "number" ? frame.value : camera.settings.fps);
  const range = $derived({ min: exposure?.min ?? EXPOSURE_FALLBACK.min, max: exposure?.max ?? EXPOSURE_FALLBACK.max });
  const maxUs = $derived(maxExposureUs(range, fps));
  const exposureUs = $derived(typeof exposure?.value === "number" ? exposure.value : null);
  const gainValue = $derived(typeof gain?.value === "number" ? gain.value : null);

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
</script>

<Prop label="Mode" help="Auto: the camera sets exposure and gain every frame. Manual: you fix them. Short manual exposures stop motion blur on a moving robot.">
  <Seg label="Exposure mode" value={auto ? "auto" : "manual"} options={[{ value: "auto", label: "Auto" }, { value: "manual", label: "Manual" }]} onchange={setMode} />
  {#if modeReason && !ae}<span class="why">{modeReason}</span>{/if}
</Prop>

{#if exposure}
  <Prop label={auto ? "Exposure (live)" : "Exposure"} help="Shutter time. The longest it can be is one frame at the current frame rate.">
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
    <Slider value={gainValue ?? 1} min={gain.min ?? 1} max={gain.max ?? 16} step={gain.step ?? 0.01} unit="×" label="Gain" disabled={!!gainLock} onchange={setGain} />
  </Prop>
{/if}

{#if !compact}
  {#if aeState}
    <ControlRow {camera} control={aeState} />
  {/if}
  {#if ev}
    <ControlRow {camera} control={ev} locked={auto ? undefined : "Only used by auto exposure"} />
  {/if}
  {#if maxLimit}
    <ControlRow {camera} control={maxLimit} label="Max exposure (motion blur limit)" locked={auto ? undefined : "Only used by auto exposure"} />
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
</style>
