<script lang="ts">
  // The LED ring's settings. Edits go into a draft, and Save writes the whole settings object to
  // the device (the device checks it; its refusal shows here). The ring above mirrors what the
  // device is showing. The device is polled while this is open, so the state stays current.
  import { api, errorText } from "#lib/api/client.js";
  import type { LedSettings, LedsResponse } from "#lib/api/leds.js";
  import { LED_LIMITS, validateLeds } from "#lib/leds/settings.js";
  import type { LedView } from "#lib/leds/states.js";
  import Badge from "#lib/kit/Badge.svelte";
  import Num from "#lib/kit/Num.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import Seg from "#lib/kit/Seg.svelte";
  import Slider from "#lib/kit/Slider.svelte";
  import Switch from "#lib/kit/Switch.svelte";
  import { toasts } from "#lib/stores/toasts.svelte.js";
  import LedPicker from "./LedPicker.svelte";
  import LedRing from "./LedRing.svelte";

  const POLL_MS = 3000;

  let res = $state<LedsResponse | null>(null);
  let saved = $state<LedSettings | null>(null);
  let draft = $state<LedSettings | null>(null);
  let loadError = $state<string | null>(null);
  let saveError = $state<string | null>(null);
  let saving = $state(false);

  const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
  const dirty = $derived(draft !== null && saved !== null && !same(draft, saved));
  const problem = $derived(draft ? validateLeds(draft) : null);
  const changed = (pick: (s: LedSettings) => unknown) => draft !== null && saved !== null && !same(pick(draft), pick(saved));

  const view = $derived<LedView | null>(res ? { state: res.live.state, look: res.live.look, brightness: res.live.brightness, leds: null } : null);
  const liveLabel = $derived.by(() => {
    const now = res;
    if (!now) return "";
    const label = now.states.find((s) => s.name === now.live.state)?.label ?? now.live.state;
    return now.live.recording ? `${label} · recording` : label;
  });

  /** Take a response from the device. An edit in progress is kept unless `force` replaces it. */
  function take(next: LedsResponse, force = false) {
    const keepDraft = !force && dirty;
    res = next;
    saved = structuredClone($state.snapshot(next.settings));
    if (!keepDraft) draft = structuredClone($state.snapshot(next.settings));
    loadError = null;
  }

  async function load() {
    try {
      take(await api.leds());
    } catch (error) {
      loadError = errorText(error);
    }
  }

  $effect(() => {
    void load();
    const timer = window.setInterval(() => void load(), POLL_MS);
    return () => clearInterval(timer);
  });

  async function save() {
    if (!draft || problem) return;
    saving = true;
    saveError = null;
    try {
      take(await api.setLeds($state.snapshot(draft) as LedSettings), true);
      toasts.success("LEDs saved");
    } catch (error) {
      saveError = errorText(error);
    } finally {
      saving = false;
    }
  }

  function revert() {
    if (saved) draft = structuredClone($state.snapshot(saved));
    saveError = null;
  }
</script>

{#if res}
  <LedRing initial={view} label={liveLabel} />
{/if}

{#if loadError}
  <Prop label="Device"><span class="small dim">{loadError}</span></Prop>
{:else if res && draft}
  {#if !res.device.reachable}
    <Prop label="Device" help="The LEDs are driven by lemnosd; the settings are kept until it is back.">
      <Badge tone="warn" text={res.device.error ?? "Not reachable"} />
    </Prop>
  {/if}
  <Prop label="Ring" help="Off: the ring stays dark." changed={changed((s) => s.enabled)}>
    <Switch bind:checked={draft.enabled} label="Ring enabled" />
  </Prop>
  <Prop label="Brightness" help="Scales the whole ring." changed={changed((s) => s.brightness)}>
    <Slider bind:value={draft.brightness} min={LED_LIMITS.brightness.min} max={LED_LIMITS.brightness.max} step={LED_LIMITS.brightness.step} label="Brightness" />
  </Prop>
  <Prop label="Follow ambient light" help="Dims the ring with the room's light, between the two limits below." changed={changed((s) => s.ambient.enabled)}>
    <span class="ambient">
      <Switch bind:checked={draft.ambient.enabled} label="Follow ambient light" />
      {#if res.live.ambient_lux !== null}<span class="dim mono">{Math.round(res.live.ambient_lux)} lx</span>{/if}
    </span>
  </Prop>
  <Prop label="Ambient minimum" help="The ring's brightness in the dark." changed={changed((s) => s.ambient.min)}>
    <Slider bind:value={draft.ambient.min} min={LED_LIMITS.ambient.min} max={LED_LIMITS.ambient.max} step={LED_LIMITS.ambient.step} disabled={!draft.ambient.enabled} label="Ambient minimum" />
  </Prop>
  <Prop label="Ambient maximum" help="The ring's brightness in bright light. Must not be below the minimum." changed={changed((s) => s.ambient.max)}>
    <Slider bind:value={draft.ambient.max} min={LED_LIMITS.ambient.min} max={LED_LIMITS.ambient.max} step={LED_LIMITS.ambient.step} disabled={!draft.ambient.enabled} label="Ambient maximum" />
  </Prop>
  <Prop label="Forward LED" help="The LED that faces the camera's centre; 0 is the ring's top." changed={changed((s) => s.forward_led)}>
    <Num bind:value={draft.forward_led} min={LED_LIMITS.forward_led.min} max={LED_LIMITS.forward_led.max} step={1} width={64} label="Forward LED" />
  </Prop>
  <Prop label="Direction" help="The way the LED index runs round the ring, seen from the front." changed={changed((s) => s.direction)}>
    <Seg
      label="Direction"
      bind:value={draft.direction}
      options={[
        { value: "cw", label: "Clockwise" },
        { value: "ccw", label: "Counter-clockwise" },
      ]}
    />
  </Prop>
  <Prop label="Field of view" help="Horizontal field of view, used when the camera has no calibration." changed={changed((s) => s.fov_deg)}>
    <Num bind:value={draft.fov_deg} min={LED_LIMITS.fov_deg.min} max={LED_LIMITS.fov_deg.max} step={1} unit="°" width={78} label="Field of view, degrees" />
  </Prop>
  <Prop label="Recording tally" help="A soft red LED at the top while the camera records." changed={changed((s) => s.tally)}>
    <Switch bind:checked={draft.tally} label="Recording tally" />
  </Prop>
  <Prop label="Robot boost" help="+30% while the robot is enabled." changed={changed((s) => s.boost)}>
    <Switch bind:checked={draft.boost} label="Robot-enabled boost" />
  </Prop>

  <div class="savebar">
    {#if problem}
      <span class="note err">{problem}</span>
    {:else if saveError}
      <span class="note err">{saveError}</span>
    {:else if dirty}
      <span class="note">Unsaved changes</span>
    {:else}
      <span class="note">Saved on the device</span>
    {/if}
    <button type="button" class="btn ghost" disabled={!dirty || saving} onclick={revert}>Revert</button>
    <button type="button" class="btn" disabled={!dirty || saving || problem !== null} onclick={save}>{saving ? "Saving…" : "Save"}</button>
  </div>

  <LedPicker {res} onresult={(next) => take(next)} />
{/if}

<style>
  .small {
    font-size: 11px;
  }
  .dim {
    color: var(--fg-3);
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11px;
  }
  .ambient {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .savebar {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 6px;
    margin: 8px 10px 6px;
    padding-top: 8px;
    border-top: 1px solid var(--line);
  }
  .note {
    flex: 1 1 auto;
    font-size: 11.5px;
    color: var(--fg-3);
  }
  .note.err {
    color: var(--err);
  }
  .btn {
    height: 26px;
    padding: 0 12px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--on-accent);
    background: var(--accent);
    border-radius: var(--r-2);
  }
  .btn.ghost {
    color: var(--fg);
    background: transparent;
    border: 1px solid var(--line-strong);
  }
  .btn:disabled {
    opacity: 0.45;
  }
</style>
