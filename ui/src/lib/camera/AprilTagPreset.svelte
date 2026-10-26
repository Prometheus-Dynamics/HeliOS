<script lang="ts">
  // The AprilTag preset, for tags on a moving robot. A camera with an exposure ceiling gets auto
  // exposure held to a short exposure, with gain up to its gain cap (the camera keeps the
  // brightness itself). Otherwise: manual exposure of about 1.5 ms, gain raised to keep the
  // brightness. It shows what it will set, applies it in one write, and undo restores the
  // camera's settings from before: each key goes back to its value, or is forgotten when the camera
  // kept no value for it before.
  import type { Camera } from "#lib/api/model.js";
  import Icon from "#lib/components/common/Icon.svelte";
  import { APRILTAG_EXPOSURE_US, aprilTagCapWrites, aprilTagPlan, autoOn, controlFor, controlNamed, disabledReason, EXPOSURE_FALLBACK, formatMs, formatNumber, presetUndoChanges, rangeOr, type PresetBefore } from "./controls.js";
  import { keyOf, writeControls } from "./write.js";

  let { camera }: { camera: Camera } = $props();

  const controls = $derived(camera.controls ?? []);
  const ae = $derived(controlFor(controls, "ae", "ae_enable"));
  const exposure = $derived(controlFor(controls, "exposure_us", "exposure_time_us"));
  const gain = $derived(controlFor(controls, "gain", "gain"));
  const frame = $derived(controlFor(controls, "fps", "frame_rate"));
  const ceiling = $derived(controlNamed(controls, "ae_exposure_max_us"));
  const gainCap = $derived(controlNamed(controls, "ae_gain_max"));

  const aeFree = $derived(!!ae && !disabledReason(ae));
  /** AE held to an exposure ceiling: the camera has the control, and the UI may change it. */
  const capped = $derived(aeFree && !!ceiling && !disabledReason(ceiling));
  const manualUsable = $derived(aeFree && !!exposure && !!gain && !disabledReason(exposure) && !disabledReason(gain));
  const usable = $derived(capped || manualUsable);

  let open = $state(false);
  /** The values the preset replaces, and whether the camera kept each, for undo. */
  let before = $state<PresetBefore | null>(null);

  const exposureRange = $derived(rangeOr(exposure, EXPOSURE_FALLBACK));
  const gainRange = $derived(rangeOr(gain, { min: 1, max: 16 }));
  const exposureNow = $derived(typeof exposure?.value === "number" ? exposure.value : null);
  const gainNow = $derived(typeof gain?.value === "number" ? gain.value : null);
  const plan = $derived(
    manualUsable && !capped ? aprilTagPlan({ exposureUs: exposureNow, gain: gainNow }, { exposure: exposureRange, gain: gainRange, fps: typeof frame?.value === "number" ? frame.value : camera.settings.fps }) : null,
  );
  /** The gain the capped preset allows: the camera's gain cap, or 16 when it lists none. */
  const cappedGainMax = $derived(gainCap?.max ?? 16);

  /** What the preset writes, by key (null when there is nothing to write). */
  const writes = $derived.by((): Record<string, number | boolean> | null => {
    if (capped && ae && ceiling) {
      const gainKey = gainCap && !disabledReason(gainCap) ? keyOf(gainCap) : undefined;
      return aprilTagCapWrites({ ae: keyOf(ae), exposureMax: keyOf(ceiling), gainMax: gainKey }, cappedGainMax);
    }
    if (plan && ae && exposure && gain) return { [keyOf(ae)]: false, [keyOf(exposure)]: plan.exposureUs, [keyOf(gain)]: plan.gain };
    return null;
  });

  function apply() {
    const next = writes;
    if (!next) return;
    before = Object.fromEntries(
      Object.keys(next).map((key) => {
        const control = controls.find((c) => keyOf(c) === key);
        return [key, { kept: control?.persisted ?? false, previous: control?.value ?? null }];
      }),
    );
    writeControls(camera, next);
    open = false;
  }

  function undo() {
    if (!before) return;
    writeControls(camera, presetUndoChanges(before));
    before = null;
  }
</script>

<div class="preset">
  {#if !open}
    <div class="row">
      <button type="button" class="go" disabled={!usable} onclick={() => (open = true)}>
        <Icon name="target" size={13} stroke={1.8} />
        AprilTag preset
      </button>
      {#if before}
        <button type="button" class="undo" onclick={undo}>
          <Icon name="arrow-back-up" size={12} stroke={1.8} />
          Undo
        </button>
      {/if}
    </div>
    <p class="note">
      {#if !usable}
        Needs auto exposure and either an exposure ceiling, or exposure and gain controls the camera lets the UI change.
      {:else if before}
        Applied. Undo puts the camera's settings from before back, with the values it kept and nothing it did not keep.
      {:else if capped}
        Auto exposure held to {formatMs(APRILTAG_EXPOSURE_US)} ms, with gain up to {formatNumber(cappedGainMax)}×. Shows what it sets before it changes anything.
      {:else}
        Short manual exposure for tags on a moving robot. Shows what it sets before it changes anything.
      {/if}
    </p>
  {:else if writes}
    <div class="confirm">
      <p>
        {#if capped}
          Turns <b>auto exposure</b> on, held to a <b>{formatMs(APRILTAG_EXPOSURE_US)} ms</b> ceiling, with gain up to <b>{formatNumber(cappedGainMax)}×</b>. The camera keeps the brightness itself.
        {:else if plan}
          Sets <b>manual</b> exposure to <b>{formatMs(plan.exposureUs)} ms</b> and gain to <b>{formatNumber(plan.gain)}×</b>.
          Now {exposureNow !== null ? `${formatMs(exposureNow)} ms` : "unknown"} at {gainNow !== null ? `${formatNumber(gainNow)}×` : "unknown"}
          ({autoOn(ae) ? "auto" : "manual"}), so the image keeps about the same brightness.
          {#if plan.capped}<span class="warn">Gain is at the camera's limit, so the image will be darker.</span>{/if}
        {/if}
      </p>
      <div class="row">
        <button type="button" class="go" onclick={apply}>Apply</button>
        <button type="button" class="cancel" onclick={() => (open = false)}>Cancel</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .preset {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 6px 10px 8px;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .go,
  .undo,
  .cancel {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: calc(var(--row) - 4px);
    padding: 0 9px;
    font-size: 12px;
    border-radius: var(--r-1);
    border: 1px solid var(--line);
    color: var(--fg-2);
  }
  .go {
    color: var(--fg);
    background: var(--s3);
  }
  .go:hover:not(:disabled),
  .undo:hover,
  .cancel:hover {
    color: var(--fg);
    border-color: var(--line-strong);
  }
  .go:disabled {
    opacity: 0.45;
  }
  .note,
  .confirm p {
    margin: 0;
    font-size: 11.5px;
    line-height: 1.45;
    color: var(--fg-3);
  }
  .confirm p b {
    color: var(--fg-2);
    font-weight: 600;
  }
  .warn {
    color: var(--warn);
  }
</style>
