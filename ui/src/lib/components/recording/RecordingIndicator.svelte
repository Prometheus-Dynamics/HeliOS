<script lang="ts">
  // The recorder at a glance in the status bar: whether a recording runs (its
  // elapsed time, size and drops) and the free space it writes to. Opens the
  // Recordings pane.
  import { shell } from "#lib/core/shell.svelte.js";
  import { pane, workspaces } from "#lib/core/workspace.svelte.js";
  import { clockText, dropSummary, elapsedMs, sizeText, spaceLevel, spaceTone, stateLabel } from "#lib/recording/format.js";
  import { recordings } from "#lib/stores/recordings.svelte.js";

  const running = $derived(recordings.active);
  const storage = $derived(recordings.listing?.storage ?? null);
  const level = $derived(storage ? spaceLevel(storage.free_bytes, storage.reserve_bytes) : "unknown");
  const elapsed = $derived(running ? elapsedMs(running, recordings.receivedAt, recordings.now) : 0);
  const drops = $derived(running ? dropSummary(running.frames_written, running.frames_dropped) : null);
  const freeText = $derived(storage ? (storage.free_bytes === null ? "free space unknown" : `${sizeText(storage.free_bytes)} free`) : null);
</script>

<button
  type="button"
  class="rec"
  onclick={() => workspaces.open(shell.active, pane("recordings"))}
  data-tip={running ? `${stateLabel(running.state)} ${running.camera}. Open Recordings` : "Open Recordings"}
>
  <span class="dot" class:live={running !== null && running.state !== "finishing"}></span>
  {#if running}
    <span class="mono strong">{stateLabel(running.state)} {clockText(elapsed)}</span>
    <span class="mono">{sizeText(running.bytes)}</span>
    {#if drops && drops.dropped > 0}<span class="mono t-{drops.tone}">{drops.dropped} dropped</span>{/if}
  {:else if recordings.listingError}
    <span class="t-err">Recorder unreachable</span>
  {:else}
    <span>Not recording</span>
  {/if}
  {#if freeText}<span class="mono free t-{spaceTone(level)}">{freeText}</span>{/if}
</button>

<style>
  .rec {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 100%;
    padding: 0 8px;
    font-size: 11px;
    color: var(--fg-3);
    white-space: nowrap;
  }
  .rec:hover {
    color: var(--fg);
    background: var(--s2);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--fg-3);
    flex-shrink: 0;
  }
  .dot.live {
    background: var(--err);
    animation: pulse 1.6s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
  .mono {
    font-family: var(--font-code);
    font-size: 10.5px;
  }
  .strong {
    color: var(--fg-2);
    font-weight: 600;
  }
  .free {
    padding-left: 2px;
  }
  .t-ok {
    color: var(--ok);
  }
  .t-warn {
    color: var(--warn);
  }
  .t-err {
    color: var(--err);
  }
  .t-neutral {
    color: var(--fg-3);
  }
</style>
