<script lang="ts">
  // The AprilTag preset: manual exposure of about 1.5 ms, gain raised to keep the brightness.
  // It shows what it will set, applies it in one write, and can be undone to the values before.
  import type { Camera } from "#lib/api/model.js";
  import Icon from "#lib/components/common/Icon.svelte";
  import { aprilTagPlan, disabledReason, EXPOSURE_FALLBACK, formatMs, formatNumber } from "./controls.js";
  import { controlNamed, keyOf, writeControls } from "./write.js";

  let { camera }: { camera: Camera } = $props();

  const controls = $derived(camera.controls ?? []);
  const ae = $derived(controlNamed(controls, "ae_enable"));
  const exposure = $derived(controlNamed(controls, "exposure_time_us"));
  const gain = $derived(controlNamed(controls, "gain"));
  const frame = $derived(controlNamed(controls, "frame_rate"));
  const usable = $derived(!!ae && !!exposure && !!gain && !disabledReason(ae) && !disabledReason(exposure) && !disabledReason(gain));

  let open = $state(false);
  /** The values before the preset was applied, for undo. */
  let before = $state<{ auto: boolean; exposureUs: number | null; gain: number | null } | null>(null);

  const exposureRange = $derived({ min: exposure?.min ?? EXPOSURE_FALLBACK.min, max: exposure?.max ?? EXPOSURE_FALLBACK.max });
  const gainRange = $derived({ min: gain?.min ?? 1, max: gain?.max ?? 16 });
  const exposureNow = $derived(typeof exposure?.value === "number" ? exposure.value : null);
  const gainNow = $derived(typeof gain?.value === "number" ? gain.value : null);
  const plan = $derived(
    usable ? aprilTagPlan({ exposureUs: exposureNow, gain: gainNow }, { exposure: exposureRange, gain: gainRange, fps: typeof frame?.value === "number" ? frame.value : camera.settings.fps }) : null,
  );

  function apply() {
    if (!ae || !exposure || !gain || !plan) return;
    before = { auto: ae.value === true, exposureUs: exposureNow, gain: gainNow };
    writeControls(camera, { [keyOf(ae)]: false, [keyOf(exposure)]: plan.exposureUs, [keyOf(gain)]: plan.gain });
    open = false;
  }

  function undo() {
    if (!ae || !exposure || !gain || !before) return;
    const changes: Record<string, number | boolean> = {};
    if (before.exposureUs !== null) changes[keyOf(exposure)] = before.exposureUs;
    if (before.gain !== null) changes[keyOf(gain)] = before.gain;
    changes[keyOf(ae)] = before.auto;
    writeControls(camera, changes);
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
        Needs auto exposure, exposure and gain controls the camera lets the UI change.
      {:else if before}
        Applied. Undo puts back {before.auto ? "auto exposure" : `${formatMs(before.exposureUs ?? 0)} ms at ${formatNumber(before.gain ?? 0)}×`}.
      {:else}
        Short manual exposure for tags on a moving robot. Shows what it sets before it changes anything.
      {/if}
    </p>
  {:else if plan}
    <div class="confirm">
      <p>
        Sets <b>manual</b> exposure to <b>{formatMs(plan.exposureUs)} ms</b> and gain to <b>{formatNumber(plan.gain)}×</b>.
        Now {exposureNow !== null ? `${formatMs(exposureNow)} ms` : "unknown"} at {gainNow !== null ? `${formatNumber(gainNow)}×` : "unknown"}
        ({ae?.value ? "auto" : "manual"}), so the image keeps about the same brightness.
        {#if plan.capped}<span class="warn">Gain is at the camera's limit, so the image will be darker.</span>{/if}
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
