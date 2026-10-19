<script lang="ts">
  // Frame rate: presets (30, 60, 90, 120, those the camera allows) or a custom number, clamped
  // to the camera's range. Lowering the rate also cuts a manual exposure that no longer fits a
  // frame, in the same write.
  import type { Camera } from "#lib/api/model.js";
  import Num from "#lib/kit/Num.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import Seg from "#lib/kit/Seg.svelte";
  import { clamp, clampExposure, disabledReason, EXPOSURE_FALLBACK, frameRateChoice, frameRatePresetsIn } from "./controls.js";
  import { controlNamed, keyOf, writeControls } from "./write.js";

  let { camera, help }: { camera: Camera; help?: string } = $props();

  const controls = $derived(camera.controls ?? []);
  const frame = $derived(controlNamed(controls, "frame_rate"));
  const range = $derived({ min: frame?.min ?? 1, max: frame?.max ?? 120 });
  const fps = $derived(typeof frame?.value === "number" ? frame.value : camera.settings.fps);
  const presets = $derived(frameRatePresetsIn(range));
  const lock = $derived(!frame ? "This camera has no frame rate control" : disabledReason(frame));
  const options = $derived([...presets.map((p) => ({ value: String(p), label: String(p) })), { value: "custom", label: "Custom" }]);

  function setFps(value: number) {
    if (!frame || lock) return;
    const next = clamp(value, range.min, range.max);
    const changes: Record<string, number | boolean> = { [keyOf(frame)]: next };
    const exposure = controlNamed(controls, "exposure_time_us");
    const ae = controlNamed(controls, "ae_enable");
    if (exposure && ae?.value !== true && typeof exposure.value === "number") {
      const bounds = { min: exposure.min ?? EXPOSURE_FALLBACK.min, max: exposure.max ?? EXPOSURE_FALLBACK.max };
      const cut = clampExposure(exposure.value, bounds, next);
      if (cut !== exposure.value) changes[keyOf(exposure)] = cut;
    }
    writeControls(camera, changes);
  }
</script>

<Prop label="Frame rate" help={help ?? "How many frames a second the camera delivers. Some rates restart the capture."}>
  <span class="fps" inert={!!lock}>
    {#if presets.length > 0}
      <Seg label="Frame rate preset" value={frameRateChoice(fps)} {options} onchange={(v) => v !== "custom" && setFps(Number(v))} />
    {/if}
    <Num value={fps} min={range.min} max={range.max} step={1} unit="fps" width={86} label="Frame rate" disabled={!!lock} onchange={setFps} />
  </span>
  {#if lock}<span class="why">{lock}</span>{/if}
</Prop>

<style>
  .fps {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: 6px;
    max-width: 100%;
  }
  .why {
    flex-basis: 100%;
    font-size: 11px;
    color: var(--fg-3);
  }
</style>
