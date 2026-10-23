<script lang="ts">
  // The IMU's orientation in 3D, live: the board's case with its axes (x red, y green, z blue).
  // With the board's own fusion (the orientation device, calibrated) it shows that; otherwise a
  // filter of the accelerometer and gyro here (attitude.ts), whose heading drifts slowly. "Zero
  // heading" makes the current heading straight ahead either way.
  import { onMount } from "svelte";
  import CaseModel from "./CaseModel.svelte";
  import { FusedOrientation, Orientation, caseOrientation, euler, type Quat } from "./attitude.ts";
  import type { LiveDevice } from "./feed.ts";
  import { ringFrame, type RingFrame } from "./ring.ts";

  let {
    imu,
    fusion = null,
    light = null,
    fill = false,
    compact = false,
  }: {
    imu: LiveDevice;
    /** The board's orientation device, when the node has one. */
    fusion?: LiveDevice | null;
    /** The status ring, for the model's LEDs. */
    light?: LiveDevice | null;
    /** As tall as its container. */
    fill?: boolean;
    /** A smaller model, the readout in a row under it, no notes. */
    compact?: boolean;
  } = $props();

  const NAMES = ["acceleration.x", "acceleration.y", "acceleration.z", "angular_rate.x", "angular_rate.y", "angular_rate.z"];
  const indices = $derived(NAMES.map((name) => imu.channel(name)));
  const fusedChannels = $derived(fusion ? FusedOrientation.channelsOf(fusion.channels.map((c) => c.name)) : null);

  let orientation: Orientation | null = null;
  let orientationOf: LiveDevice | null = null;
  let board: FusedOrientation | null = null;
  let boardOf: LiveDevice | null = null;
  /** What the view shows now: the board's fusion once it streams. */
  let source = $state<"board" | "here">("here");
  let disturbed = $state(false);
  let confidence = $state<number | null>(null);
  const current = () => (source === "board" ? board : orientation);
  let ringCache: RingFrame | null = null;
  const ring = () => (light ? (ringCache = ringFrame(light, ringCache)) : null);
  /** The case model didn't load (no WebGL, no model): said instead. */
  let plain = $state(false);
  let angles = $state({ roll: 0, pitch: 0, yaw: 0 });
  let accelG = $state(0);
  let rateDps = $state(0);
  /** Until the filter has a sample: level. The model reads the filter's own attitude each frame. */
  const LEVEL: Quat = [1, 0, 0, 0];

  onMount(() => {
    let frame = 0;
    const draw = () => {
      if (indices.every((i) => i >= 0)) {
        if (!orientation || orientationOf !== imu) {
          orientation = new Orientation(indices);
          orientationOf = imu;
        }
        const tilt = orientation;
        tilt.update(imu);
        if (fusion && fusedChannels && fusion.count > 0) {
          if (boardOf !== fusion || !board) {
            board = new FusedOrientation(fusedChannels);
            boardOf = fusion;
          }
          board.update(fusion);
          source = "board";
          disturbed = board.disturbed;
          confidence = board.confidence;
        } else source = "here";
        // Shown and read out in the case's frame (lens out of z), not the IMU's.
        angles = euler(caseOrientation(current()?.q ?? tilt.q));
        if (imu.count) {
          const at = imu.index(imu.count - 1);
          const [ax, ay, az, gx, gy, gz] = indices.map((c) => imu.v[c][at]);
          accelG = Math.hypot(ax, ay, az) / 9.80665;
          rateDps = (Math.hypot(gx, gy, gz) * 180) / Math.PI;
        }
      }
      frame = requestAnimationFrame(draw);
    };
    frame = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(frame);
  });

  const sourceNote = $derived(
    source === "board"
      ? `Fused on the board${confidence !== null ? ` (IMU confidence ${Math.round(confidence * 100)}%)` : ""}; ${disturbed ? "heading from the gyro (the magnetometer isn't trusted now)" : "heading from the magnetometer"}.`
      : "Tilt from gravity; heading from the gyro alone, so it drifts slowly.",
  );
  const fmt = (value: number, digits = 1) => (Number.isFinite(value) ? value.toFixed(digits) : "–");
</script>

<div class="imu" class:fill class:compact>
  <div class="stage" role="img" aria-label="The board's orientation in 3D" title="Drag to look around it, scroll to zoom, double-click to reset the view. {sourceNote}">
    {#if !plain}
      <CaseModel orientation={() => caseOrientation(current()?.q ?? LEVEL)} {ring} onfail={() => (plain = true)} />
    {:else}
      <p class="unavailable">The 3D model needs WebGL, which this browser does not give. The readings are still live.</p>
    {/if}
  </div>
  <div class="readout">
    <dl>
      <div><dt>Roll</dt><dd>{fmt(angles.roll)}°</dd></div>
      <div><dt>Pitch</dt><dd>{fmt(angles.pitch)}°</dd></div>
      <div><dt>Heading</dt><dd>{fmt(angles.yaw)}°</dd></div>
      <div><dt>|a|</dt><dd>{fmt(accelG, 2)} g</dd></div>
      <div><dt>|ω|</dt><dd>{fmt(rateDps)} °/s</dd></div>
    </dl>
    <button type="button" class="btn zero" title="Zero heading: make the current heading straight ahead" onclick={() => current()?.zero()}>{compact ? "Zero" : "Zero heading"}</button>
    {#if !compact}
      <p class="note">
        {#if source === "board"}
          Fused on the board{confidence !== null ? ` (IMU confidence ${Math.round(confidence * 100)}%)` : ""};
          {disturbed ? "heading from the gyro: the magnetometer isn't trusted now (calibrate it, or a magnet is near)" : "heading from the magnetometer"}.
        {:else}
          Tilt from gravity; heading from the gyro alone, so it drifts slowly.
        {/if}
      </p>
    {/if}
  </div>
</div>

<style>
  .imu {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 12px;
    align-items: center;
    container-type: inline-size;
  }
  @media (max-width: 560px) {
    .imu {
      grid-template-columns: 1fr;
    }
  }
  /* In a narrow card: the model across, the readout in a row under it. */
  @container (max-width: 520px) {
    .imu,
    .imu.fill {
      grid-template-columns: 1fr;
      grid-template-rows: minmax(0, 1fr) auto;
    }
    .readout {
      min-width: 0;
    }
    dl {
      grid-template-columns: repeat(5, auto);
      grid-auto-flow: column;
      grid-template-rows: auto auto;
      justify-content: space-between;
    }
    dd {
      text-align: left;
    }
    .note {
      display: none;
    }
    .imu.fill .stage.stage {
      min-height: 160px;
    }
  }
  .imu.compact {
    grid-template-columns: 1fr;
    gap: 6px;
  }
  .imu.compact .stage {
    height: clamp(96px, 14vh, 140px);
  }
  .imu.compact dl {
    grid-template-columns: repeat(3, auto);
    grid-auto-flow: column;
    grid-template-rows: auto auto;
    gap: 0 14px;
    flex: 1;
  }
  .imu.compact dl > div:nth-child(n + 4) {
    display: none;
  }
  .imu.compact .readout {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .imu.fill {
    height: 100%;
    align-items: stretch;
  }
  .imu.fill .readout {
    align-self: center;
  }
  .stage {
    height: 230px;
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    border-radius: var(--r-md);
    background: var(--inset);
  }
  .imu.fill .stage {
    height: auto;
    min-height: 230px;
  }
  .btn {
    align-self: flex-start;
    height: 22px;
    padding: 0 8px;
    font-size: 11.5px;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
  }
  .btn:hover {
    background: var(--s3);
  }
  .unavailable {
    margin: auto;
    max-width: 22ch;
    padding: 10px;
    text-align: center;
    font-size: 12px;
    color: var(--fg-3);
  }
  .readout {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 11rem;
  }
  dl {
    display: grid;
    grid-template-columns: auto auto;
    gap: 2px 14px;
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
  }
  dl div {
    display: contents;
  }
  dt {
    color: var(--fg-3);
  }
  dd {
    text-align: right;
    font-weight: 600;
    font-family: var(--font-code);
    color: var(--fg);
  }
  .note {
    font-size: 11px;
    color: var(--fg-3);
    max-width: 14rem;
  }
</style>
