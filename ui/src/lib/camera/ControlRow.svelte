<script lang="ts">
  // One camera control the device lists, drawn from what the device says about it: a switch for
  // a bool, labelled choices for an enum, a slider (log for wide spans) or number field for a
  // number, text for a read-only value. Shows its unit and value, a reset when it is away from
  // its default (which forgets the value kept for it), a "saved" mark when the value is kept
  // across reboots, and why it is locked.
  import type { Camera, CameraControlInfo } from "#lib/api/model.js";
  import Choice from "#lib/kit/Choice.svelte";
  import Num from "#lib/kit/Num.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import Slider from "#lib/kit/Slider.svelte";
  import Switch from "#lib/kit/Switch.svelte";
  import { disabledReason, enumOptions, friendlyLabel, isChanged, resetTarget, unitOf, valueText } from "./controls.js";
  import { forgetControl, keyOf, writeControls } from "./write.js";

  let {
    camera,
    control,
    label,
    locked,
  }: {
    camera: Camera;
    control: CameraControlInfo;
    /** Overrides the friendly label. */
    label?: string;
    /** A reason the control is locked here, beyond the device's own (e.g. "Auto exposure is on"). */
    locked?: string;
  } = $props();

  const title = $derived(label ?? friendlyLabel(control.name));
  const reason = $derived(control.readOnly ? null : (locked ?? disabledReason(control)));
  const target = $derived(reason ? null : resetTarget(control));
  const unit = $derived(unitOf(control.name));
  const options = $derived(enumOptions(control));
  const text = $derived(valueText(control));
  /** The range the slider spans: the camera's effective range now when it reports one. */
  const low = $derived(control.effectiveMin ?? control.min);
  const high = $derived(control.effectiveMax ?? control.max);
  const wide = $derived(low !== undefined && high !== undefined && low > 0 && high / low >= 100);
  const ranged = $derived(low !== undefined && high !== undefined && high > low);

  function write(value: number | boolean) {
    writeControls(camera, { [keyOf(control)]: value });
  }
</script>

<Prop
  label={title}
  help="{control.name}{control.standard ? ` · standard ${control.standard}` : ''}"
  changed={isChanged(control)}
  onreset={target !== null ? () => forgetControl(camera, control) : undefined}
>
  {#if control.rect !== undefined || control.readOnly || control.value === null}
    <span class="value">{text}</span>
  {:else if typeof control.value === "boolean"}
    <Switch checked={control.value} label={title} disabled={!!reason} onchange={write} />
    <span class="value">{text}</span>
  {:else if options}
    <Choice label={title} value={Number(control.value)} {options} disabled={!!reason} onchange={(v) => write(Number(v))} />
  {:else if ranged && typeof control.value === "number"}
    <Slider value={control.value} min={low} max={high} step={control.step ?? 1} {unit} log={wide} label={title} disabled={!!reason} onchange={write} />
  {:else}
    <Num value={Number(control.value)} step={control.step ?? 1} {unit} label={title} disabled={!!reason} onchange={write} />
  {/if}
  {#if control.persisted}<span class="saved" data-tip="Kept across reboots">saved</span>{/if}
  {#if reason && !control.rect}<span class="why">{reason}</span>{/if}
</Prop>

<style>
  .value {
    font-family: var(--font-code);
    font-size: 11.5px;
    color: var(--fg-2);
    white-space: nowrap;
  }
  .saved {
    font-size: 10px;
    padding: 0 5px;
    border-radius: var(--r-1);
    color: var(--ok);
    background: var(--ok-bg);
  }
  .why {
    flex-basis: 100%;
    font-size: 11px;
    color: var(--fg-3);
  }
</style>
