<script lang="ts">
  // White balance: auto, or manual colour temperature and red/blue gains. In auto the colour
  // temperature is the camera's estimate for the latest frame, and the gains are locked.
  import type { Camera, CameraControlInfo } from "#lib/api/model.js";
  import Prop from "#lib/kit/Prop.svelte";
  import Seg from "#lib/kit/Seg.svelte";
  import Slider from "#lib/kit/Slider.svelte";
  import { disabledReason, isChanged, resetTarget } from "./controls.js";
  import { controlNamed, keyOf, writeControls } from "./write.js";

  let { camera }: { camera: Camera } = $props();

  const controls = $derived(camera.controls ?? []);
  const awb = $derived(controlNamed(controls, "awb_enable"));
  const colour = $derived(controlNamed(controls, "colour_temperature"));
  const red = $derived(controlNamed(controls, "red_gain"));
  const blue = $derived(controlNamed(controls, "blue_gain"));
  const auto = $derived(awb?.value === true);

  const numberOr = (value: number | boolean | null | undefined, fallback: number) => (typeof value === "number" ? value : fallback);

  /** Why a white-balance control is locked here, or null when it can be changed. */
  function lockOf(control: CameraControlInfo | undefined, name: string): string | null {
    if (!control) return `This camera has no ${name} control`;
    if (auto) return "Automatic white balance is on";
    return disabledReason(control);
  }
  const colourLock = $derived(lockOf(colour, "colour temperature"));
  const redLock = $derived(lockOf(red, "red gain"));
  const blueLock = $derived(lockOf(blue, "blue gain"));

  function setAuto(value: "auto" | "manual") {
    if (awb && !disabledReason(awb)) writeControls(camera, { [keyOf(awb)]: value === "auto" });
  }
  function set(control: CameraControlInfo | undefined, value: number) {
    if (control) writeControls(camera, { [keyOf(control)]: value });
  }
  function resetOf(control: CameraControlInfo | undefined, lock: string | null): (() => void) | undefined {
    const target = control && !lock ? resetTarget(control) : null;
    return target === null || !control ? undefined : () => set(control, target as number);
  }
</script>

<Prop label="Mode" help="Auto picks the colour balance from the scene. Manual fixes it with the colour temperature and the red and blue gains.">
  <Seg label="White balance" value={auto ? "auto" : "manual"} options={[{ value: "auto", label: "Auto" }, { value: "manual", label: "Manual" }]} onchange={setAuto} />
  {#if !awb}<span class="why">This camera has no white balance control</span>{/if}
</Prop>

{#if colour}
  <Prop
    label={auto ? "Colour temp (estimate)" : "Colour temp"}
    help="colour_temperature. In auto, the camera's estimate for the latest frame; in manual, the colour it corrects to."
    changed={!colourLock && isChanged(colour)}
    onreset={resetOf(colour, colourLock)}
  >
    <Slider
      value={numberOr(colour.value, colour.min ?? 1000)}
      min={colour.min ?? 1000}
      max={colour.max ?? 20000}
      step={colour.step ?? 50}
      unit="K"
      label="Colour temperature"
      disabled={!!colourLock}
      onchange={(v) => set(colour, v)}
    />
    {#if colourLock && auto}<span class="why">{colourLock}</span>{/if}
  </Prop>
{/if}

{#if red}
  <Prop label="Red gain" help="red_gain, relative to green. Used in manual white balance." changed={!redLock && isChanged(red)} onreset={resetOf(red, redLock)}>
    <Slider value={numberOr(red.value, 0)} min={red.min ?? 0} max={red.max ?? 8} step={red.step ?? 0.01} unit="×" label="Red gain" disabled={!!redLock} onchange={(v) => set(red, v)} />
    {#if redLock && auto}<span class="why">{redLock}</span>{/if}
  </Prop>
{/if}

{#if blue}
  <Prop label="Blue gain" help="blue_gain, relative to green. Used in manual white balance." changed={!blueLock && isChanged(blue)} onreset={resetOf(blue, blueLock)}>
    <Slider value={numberOr(blue.value, 0)} min={blue.min ?? 0} max={blue.max ?? 8} step={blue.step ?? 0.01} unit="×" label="Blue gain" disabled={!!blueLock} onchange={(v) => set(blue, v)} />
    {#if blueLock && auto}<span class="why">{blueLock}</span>{/if}
  </Prop>
{/if}

<style>
  .why {
    flex-basis: 100%;
    font-size: 11px;
    color: var(--fg-3);
  }
</style>
