<script lang="ts">
  // The recording on the device: while it runs, its state, elapsed time, size,
  // drops and free space, with stop. Otherwise the control to start one from
  // the camera picked in the pane's tab bar.
  import Icon from "#lib/components/common/Icon.svelte";
  import { identity } from "#lib/core/identity.svelte.js";
  import Badge from "#lib/kit/Badge.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { recordings } from "#lib/stores/recordings.svelte.js";
  import { clockText, dropSummary, elapsedMs, sizeText, spaceLevel, spaceText, spaceTone, stateLabel, stateTone } from "#lib/recording/format.js";
  import type { Snippet } from "svelte";

  let { camera }: { camera: string | undefined } = $props();

  const running = $derived(recordings.active);
  const elapsed = $derived(running ? elapsedMs(running, recordings.receivedAt, recordings.now) : 0);
  const drops = $derived(running ? dropSummary(running.frames_written, running.frames_dropped) : null);
  const level = $derived(running ? spaceLevel(running.free_bytes, running.reserve_bytes) : "unknown");
  const busy = $derived(recordings.busy);
  const cameraName = (id: string) => identity.name(id, cluster.camera(id)?.name ?? id);
</script>

{#snippet stat(label: string, children: Snippet)}
  <div class="stat">
    <span class="k">{label}</span>
    <span class="v">{@render children()}</span>
  </div>
{/snippet}

{#if running}
  <div class="now">
    <div class="head">
      <span class="cam">{cameraName(running.camera)}</span>
      <Badge tone={stateTone(running.state)} text={stateLabel(running.state)} />
      <span class="grow"></span>
      <button
        type="button"
        class="btn danger"
        onclick={() => recordings.stop(running.id)}
        disabled={running.state === "finishing" || busy === `stop:${running.id}`}
      >
        <Icon name="player-stop" size={13} />{running.state === "finishing" ? "Finishing" : "Stop"}
      </button>
    </div>
    <div class="clock mono" aria-label="Elapsed time">{clockText(elapsed)}</div>
    <div class="stats">
      {#snippet size()}<span class="mono">{sizeText(running.bytes)}</span>{/snippet}
      {@render stat("Size", size)}
      {#snippet frames()}<span class="mono">{running.frames_written} frames</span>{/snippet}
      {@render stat("Written", frames)}
      {#snippet dropped()}{#if drops}<Badge tone={drops.tone} text={drops.text} />{/if}{/snippet}
      {@render stat("Drops", dropped)}
      {#snippet free()}<Badge tone={spaceTone(level)} text={spaceText(running.free_bytes, running.reserve_bytes)} />{/snippet}
      {@render stat("Storage", free)}
    </div>
    <p class="meta mono">{running.format} · {running.frames} · {running.max_fps ? `up to ${running.max_fps} fps` : "camera rate"}{running.file ? ` · ${running.file}` : ""}</p>
    {#if running.error}<p class="err">{running.error}</p>{/if}
  </div>
{:else}
  <div class="idle">
    <div class="idle-text">
      <b>Not recording</b>
      <span>{camera ? `Record ${cameraName(camera)}'s frames to this device.` : "Pick a camera in the tab bar to record it."}</span>
    </div>
    <button type="button" class="btn" onclick={() => camera && recordings.start(camera)} disabled={!camera || busy === "start" || !recordings.listing}>
      <Icon name="player-record" size={13} />Start recording
    </button>
  </div>
{/if}

<style>
  .now,
  .idle {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 4px 10px 10px;
  }
  .idle {
    flex-direction: row;
    align-items: center;
    gap: 12px;
  }
  .idle-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }
  .idle-text b {
    font-size: 13px;
    font-weight: 650;
  }
  .idle-text span {
    font-size: 11.5px;
    color: var(--fg-3);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .cam {
    font-size: 13px;
    font-weight: 650;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .grow {
    flex: 1;
  }
  .clock {
    font-size: 30px;
    font-weight: 600;
    letter-spacing: -0.01em;
    line-height: 1.1;
    font-variant-numeric: tabular-nums;
  }
  .stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 8px 16px;
  }
  .stat {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .k {
    font-size: 10.5px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--fg-3);
  }
  .v {
    font-size: 12px;
    display: flex;
    align-items: center;
    min-width: 0;
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11.5px;
  }
  .meta {
    margin: 0;
    font-size: 10.5px;
    color: var(--fg-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .err {
    margin: 0;
    font-size: 11.5px;
    color: var(--err);
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 12px;
    font-size: 12px;
    font-weight: 600;
    color: var(--on-accent);
    background: var(--accent);
    border-radius: var(--r-2);
    white-space: nowrap;
  }
  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .btn.danger {
    background: var(--err);
  }
</style>
