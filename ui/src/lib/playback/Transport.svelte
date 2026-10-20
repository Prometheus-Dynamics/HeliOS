<script lang="ts">
  // A recording's transport: play or pause, step and seek, a scrubber, the
  // speed, loop, and the readouts. Every control sends replay_* changes to the
  // player's camera; the keys are the same actions (see PlayerPanel).
  import Icon from "#lib/components/common/Icon.svelte";
  import IconButton from "#lib/kit/IconButton.svelte";
  import type { PlayerView, PlaybackStatus } from "#lib/api/types.js";
  import { playback } from "./store.svelte.js";
  import {
    canStepForward,
    fractionOf,
    frameReadout,
    loopChange,
    planAction,
    planPlay,
    playAction,
    previousFrame,
    seekMsChange,
    SEEK_STEP_MS,
    SHORTCUTS,
    speedChange,
    speedOptions,
    stepChange,
    timeReadout,
    type ReplayChange,
  } from "./transport.js";

  let { player, status, legend = false }: { player: PlayerView; status: PlaybackStatus; legend?: boolean } = $props();

  const action = $derived(playAction(status));
  const back = $derived(previousFrame(status) !== null);
  const forward = $derived(canStepForward(status));
  const seekable = $derived(status.duration_ms > 0);

  // The scrubber: while dragging it shows the drag and sends seeks at most every SCRUB_MS.
  const SCRUB_MS = 120;
  let drag = $state<number | null>(null);
  let sentAt = 0;
  let trailing: ReturnType<typeof setTimeout> | undefined;
  $effect(() => () => clearTimeout(trailing));

  function scrub(ms: number) {
    drag = ms;
    const wait = SCRUB_MS - (performance.now() - sentAt);
    clearTimeout(trailing);
    if (wait <= 0) {
      sentAt = performance.now();
      void playback.apply(player, [seekMsChange(ms)]);
    } else {
      trailing = setTimeout(() => scrub(drag ?? ms), wait);
    }
  }

  function scrubEnd(ms: number) {
    clearTimeout(trailing);
    drag = null;
    void playback.apply(player, [seekMsChange(ms)]);
  }

  const shown = $derived(drag ?? status.position_ms);
  const fraction = $derived(fractionOf(shown, status.duration_ms));

  function run(changes: ReplayChange[]) {
    void playback.apply(player, changes);
  }

  const playIcon = $derived(action === "pause" ? "player-pause" : action === "replay" ? "rotate-clockwise" : "player-play");
  const playLabel = $derived(action === "pause" ? "Pause" : action === "replay" ? "Play from the start" : "Play");
</script>

<div class="transport" aria-label="Transport">
  <div class="row controls">
    <IconButton icon="player-skip-back" label="Step back one frame" size={40} disabled={!back} onclick={() => run(planAction("step-back", status))} shortcut="←" />
    <IconButton icon="rewind-backward-5" label="Back 5 seconds" size={40} disabled={status.position_ms <= 0} onclick={() => run(planAction("seek-back", status))} shortcut="J" />
    <IconButton icon={playIcon} label={playLabel} size={40} tone="accent" onclick={() => run(planPlay(status))} shortcut="Space" />
    <IconButton icon="rewind-forward-5" label="Forward 5 seconds" size={40} disabled={status.position_ms >= status.duration_ms} onclick={() => run(planAction("seek-forward", status))} shortcut="L" />
    <IconButton icon="player-track-next" label="Step forward one frame" size={40} disabled={!forward} onclick={() => run([stepChange()])} shortcut="→" />
    <span class="sep" aria-hidden="true"></span>
    <IconButton icon="repeat" label="Loop" size={40} active={status.loop} onclick={() => run([loopChange(!status.loop)])} />
    <label class="speed">
      <span class="sr">Speed</span>
      <select class="select" value={String(status.speed ?? 1)} onchange={(e) => run([speedChange(Number(e.currentTarget.value))])} aria-label="Playback speed">
        {#each speedOptions(status.speed) as option (option.value)}
          <option value={String(option.value)}>{option.label}</option>
        {/each}
      </select>
    </label>
  </div>

  <div class="scrub-row">
    <input
      class="scrub"
      type="range"
      min="0"
      max={Math.max(0, status.duration_ms)}
      step="1"
      value={shown}
      disabled={!seekable}
      aria-label="Position"
      aria-valuetext={timeReadout({ position_ms: shown, duration_ms: status.duration_ms })}
      style:--fill="{fraction * 100}%"
      oninput={(e) => scrub(Number(e.currentTarget.value))}
      onchange={(e) => scrubEnd(Number(e.currentTarget.value))}
    />
  </div>

  <div class="readouts mono">
    <span class="time" data-tip="Position / duration">{timeReadout({ position_ms: shown, duration_ms: status.duration_ms })}</span>
    <span class="frame" data-tip="Frame / frame count (0-based)">frame {frameReadout(status)}</span>
    <span class="loop-state">{status.loop ? "looping" : ""}</span>
  </div>

  {#if legend}
    <dl class="legend">
      {#each SHORTCUTS as s (s.action)}
        <div>
          <dt>{#each s.keys as k, i (k)}{#if i}<span class="or">or</span>{/if}<kbd>{k}</kbd>{/each}</dt>
          <dd>{s.action}</dd>
        </div>
      {/each}
      <p class="hint"><Icon name="keyboard" size={12} /> J and L move {SEEK_STEP_MS / 1000} seconds. Keys work while the player has focus.</p>
    </dl>
  {/if}
</div>

<style>
  .transport {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    min-width: 0;
  }
  .sep {
    width: 1px;
    height: 24px;
    margin: 0 4px;
    background: var(--line-strong);
  }
  .speed {
    display: inline-flex;
    align-items: center;
  }
  .speed .select {
    width: auto;
    min-width: 110px;
    height: 40px;
    min-height: 40px;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
  .scrub-row {
    padding: 4px 0;
  }
  .scrub {
    width: 100%;
    height: 32px;
    margin: 0;
    background: transparent;
    accent-color: var(--accent);
    cursor: pointer;
  }
  .scrub:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .readouts {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 14px;
    font-size: 12px;
    color: var(--fg-2);
  }
  .time {
    color: var(--fg);
  }
  .loop-state {
    color: var(--accent-text);
  }
  .legend {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
    gap: 6px 14px;
    margin: 4px 0 0;
    padding: 10px 12px;
    background: var(--inset);
    border: 1px solid var(--line);
    border-radius: var(--r-2);
    font-size: 12px;
  }
  .legend > div {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .legend dt {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 4px;
    align-items: center;
    min-width: 96px;
  }
  .legend dd {
    margin: 0;
    color: var(--fg-2);
  }
  .or {
    color: var(--fg-3);
    font-size: 11px;
  }
  .legend .hint {
    grid-column: 1 / -1;
    margin: 0;
    display: flex;
    align-items: center;
    gap: 6px;
  }
</style>
