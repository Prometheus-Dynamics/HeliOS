<script lang="ts">
  // The IMU, live, as the Hardware screen and the IMU pane show it: the board in 3D (from its own
  // fusion when the node has it, else a filter of the IMU here), the status ring's LEDs on the
  // model and beside it, the accelerometer, gyro and magnetometer over the last seconds, and the
  // numbers. Readings come from the node's live stream (`liveStreams`), pushed as they are read.
  import { liveStreams } from "./live.svelte.ts";
  import type { LiveDevice } from "./feed.ts";
  import ImuView from "./ImuView.svelte";
  import RingView from "./RingView.svelte";
  import Trace from "./Trace.svelte";

  /** compact: the model smaller, for a card under others. */
  let { compact = false, fill = false }: { compact?: boolean; fill?: boolean } = $props();

  $effect(() => liveStreams.watch());

  // The devices are found by the feed, which changes with the stream's batches (`version`).
  const tick = $derived(liveStreams.version);
  const imu = $derived.by((): LiveDevice | null => (void tick, liveStreams.feed.device("imu") ?? null));
  const fusion = $derived.by((): LiveDevice | null => (void tick, liveStreams.feed.device("orientation") ?? null));
  const light = $derived.by((): LiveDevice | null => (void tick, liveStreams.feed.device("light") ?? null));
  const mag = $derived.by((): LiveDevice | null => (void tick, liveStreams.feed.device("magnetometer") ?? null));

  const AXES = ["x", "y", "z"];
  const AXIS_COLORS = ["var(--err)", "var(--ok)", "var(--info)"];
  const ACCEL = $derived(imu ? AXES.map((a) => imu.channel(`acceleration.${a}`)) : []);
  const GYRO = $derived(imu ? AXES.map((a) => imu.channel(`angular_rate.${a}`)) : []);
  const FIELD = $derived(mag ? AXES.map((a) => mag.channel(`magnetic_field.${a}`)) : []);

  /** The newest row of a set of channels, or NaN. */
  const latest = (device: LiveDevice | null, channels: number[]) => channels.map((c) => (device ? device.latest(c) : Number.NaN));
  const accelG = $derived(latest(imu, ACCEL).map((v) => v / 9.80665));
  const gyroDps = $derived(latest(imu, GYRO).map((v) => (v * 180) / Math.PI));
  const field = $derived(latest(mag, FIELD));

  const fmt = (value: number, digits: number) => (Number.isFinite(value) ? (value >= 0 ? " " : "") + value.toFixed(digits) : "–");

  /** The line under the title: the stream's state, in words. */
  const state = $derived.by(() => {
    void tick;
    const status = liveStreams.status;
    if (status.state === "live") return { tone: "ok", text: "Live from the board" };
    if (status.state === "waiting") return { tone: "warn", text: "Connected; waiting for the IMU's first readings" };
    if (status.state === "stale") return { tone: "warn", text: status.reason || "The board's readings have stopped" };
    if (status.state === "unavailable") return { tone: "err", text: status.reason || "The node's streams are not reachable" };
    return { tone: "none", text: "Connecting to the node's streams…" };
  });
</script>

<div class="panel" class:fill class:compact>
  <p class="status {state.tone}" role="status"><span class="dot"></span>{state.text}</p>

  {#if imu}
    <div class="stage">
      <ImuView {imu} {fusion} light={light} {fill} {compact} />
    </div>

    <div class="row">
      {#if light}<RingView light={light} {tick} size={compact ? 56 : 72} />{/if}
      <dl class="axes mono">
        <dt>accel g</dt><dd>{AXES.map((_, i) => fmt(accelG[i], 2)).join(" ")}</dd>
        <dt>gyro °/s</dt><dd>{AXES.map((_, i) => fmt(gyroDps[i], 1)).join(" ")}</dd>
        {#if mag}<dt>field µT</dt><dd>{AXES.map((_, i) => fmt(field[i], 1)).join(" ")}</dd>{/if}
      </dl>
    </div>

    {#if !compact}
      <div class="trace">
        <span class="label">accel</span>
        <Trace device={imu} channels={ACCEL} colors={AXIS_COLORS} {tick} />
      </div>
      <div class="trace">
        <span class="label">gyro</span>
        <Trace device={imu} channels={GYRO} colors={AXIS_COLORS} {tick} />
      </div>
      {#if mag}
        <div class="trace">
          <span class="label">field</span>
          <Trace device={mag} channels={FIELD} colors={AXIS_COLORS} {tick} />
        </div>
      {/if}
      <p class="legend"><i style="--c: var(--err)"></i>x <i style="--c: var(--ok)"></i>y <i style="--c: var(--info)"></i>z · the last 10 s</p>
    {/if}
  {:else}
    <p class="none">The IMU's readings appear here once the node streams them.</p>
  {/if}
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 4px 10px 8px;
    min-width: 0;
  }
  .panel.fill {
    height: 100%;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: 11.5px;
    color: var(--fg-3);
  }
  .dot {
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: #3a414c;
  }
  .status.ok .dot {
    background: var(--ok);
  }
  .status.warn .dot {
    background: var(--warn);
  }
  .status.err .dot {
    background: var(--err);
  }
  .stage {
    min-width: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .axes {
    display: grid;
    grid-template-columns: auto auto;
    gap: 2px 10px;
    margin: 0;
    font-size: 11.5px;
    white-space: nowrap;
  }
  .axes dt {
    color: var(--fg-3);
    font-family: var(--font-ui);
  }
  .axes dd {
    margin: 0;
    color: var(--fg);
  }
  .trace {
    display: grid;
    grid-template-columns: 40px minmax(0, 1fr);
    align-items: center;
    gap: 8px;
  }
  .label {
    font-size: 10.5px;
    color: var(--fg-3);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .legend {
    margin: 0;
    font-size: 10.5px;
    color: var(--fg-3);
  }
  .legend i {
    display: inline-block;
    width: 8px;
    height: 8px;
    margin: 0 3px 0 6px;
    border-radius: 2px;
    background: var(--c);
  }
  .none {
    margin: auto;
    padding: 12px;
    text-align: center;
    font-size: 12px;
    color: var(--fg-3);
  }
  .mono {
    font-family: var(--font-code);
  }
</style>
