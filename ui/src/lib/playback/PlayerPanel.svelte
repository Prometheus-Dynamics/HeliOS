<script lang="ts">
  // One open player: its frames as a camera (with the detection overlays of its
  // pipelines), the note the recording carries, the transport, and "run a
  // pipeline on it". The keys work while the player has focus.
  import Icon from "#lib/components/common/Icon.svelte";
  import type { PlayerView } from "#lib/api/types.js";
  import { menu } from "#lib/core/menu.svelte.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import IconButton from "#lib/kit/IconButton.svelte";
  import Feed from "#lib/vision/Feed.svelte";
  import { DEFAULT_OVERLAYS, overlayMenu, type Overlays } from "#lib/vision/overlays.js";
  import RunPipeline from "./RunPipeline.svelte";
  import { playback } from "./store.svelte.js";
  import { sourceLabel } from "./sources.js";
  import Transport from "./Transport.svelte";
  import { planAction, shortcutFor, transportKnown } from "./transport.js";

  let { player, onclose }: { player: PlayerView; onclose: () => void } = $props();

  const camera = $derived(cluster.camera(player.resource_id));
  const status = $derived(player.status);
  let overlays = $state<Overlays>({ ...DEFAULT_OVERLAYS });
  let legend = $state(false);

  function setOverlays(patch: Partial<Overlays>) {
    overlays = { ...overlays, ...patch };
  }

  /** The transport's keys, when this player is focused. Typing in a field is never a shortcut. */
  function onkeydown(event: KeyboardEvent) {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    const target = event.target as HTMLElement;
    if (target.closest("input, textarea, select, [contenteditable]")) return;
    // Space on a focused button is that button's own press (play, say), not a second toggle.
    if (event.key === " " && target.closest("button, a, [role=button]")) return;
    const key = shortcutFor(event.key);
    if (!key || player.kind !== "recording" || !status || !transportKnown(status)) return;
    // Handled here: the page's own space shortcut (play the mock robot) must not also fire.
    event.preventDefault();
    event.stopPropagation();
    void playback.apply(player, planAction(key, status));
  }

  let root = $state<HTMLDivElement>();
  $effect(() => {
    if (!root) return;
    root.addEventListener("keydown", onkeydown);
    return () => root?.removeEventListener("keydown", onkeydown);
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div class="player" bind:this={root} tabindex="0" role="application" aria-label="Player: {player.name}. Space plays or pauses; arrows step; J and L seek.">
  <header>
    <div class="id">
      <b>{player.name}</b>
      <span class="src mono" title={sourceLabel(player.source)}>{sourceLabel(player.source)}</span>
    </div>
    <span class="kind">{player.kind === "images" ? "Image folder" : "Recording"}</span>
    <span class="tools">
      <IconButton icon="target" label="Overlays" size={34} onclick={(e) => menu.below(e.currentTarget as Element, overlayMenu(overlays, setOverlays), "end")} />
      {#if player.kind === "recording"}
        <IconButton icon="keyboard" label="Keyboard shortcuts" size={34} active={legend} onclick={() => (legend = !legend)} />
      {/if}
      <IconButton icon="x" label="Close this player" size={34} onclick={onclose} />
    </span>
  </header>

  {#if player.note}
    <p class="note"><Icon name="info-circle" size={14} /><span>{player.note}</span></p>
  {/if}

  <div class="stage">
    {#if camera}
      <Feed {camera} {overlays} />
    {:else}
      <div class="wait"><Icon name="loader-2" size={16} class="spin" />Starting the frame view</div>
    {/if}
  </div>

  {#if player.kind === "recording"}
    {#if status && transportKnown(status)}
      <Transport {player} {status} {legend} />
    {:else}
      <p class="note"><Icon name="alert-triangle" size={14} /><span>The replay controls did not answer{player.status_error ? `: ${player.status_error}` : ""}.</span></p>
    {/if}
  {:else}
    <p class="note"><Icon name="info-circle" size={14} /><span>An image folder plays at its frame rate, so it has no transport to seek or step.</span></p>
  {/if}

  <RunPipeline {player} />
</div>

<style>
  .player {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
    padding: 10px;
    border-radius: var(--r-2);
    outline-offset: -2px;
  }
  header {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px 10px;
    min-width: 0;
  }
  .id {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }
  .id b {
    font-size: 14px;
    font-weight: 650;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .src {
    font-size: 11px;
    color: var(--fg-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }
  .kind {
    font-size: 11px;
    font-weight: 600;
    color: var(--fg-2);
    padding: 2px 7px;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
  }
  .tools {
    display: inline-flex;
    gap: 2px;
  }
  .stage {
    position: relative;
    width: 100%;
    aspect-ratio: 16 / 10;
    max-height: 62vh;
    min-height: 180px;
    border-radius: var(--r-2);
    overflow: hidden;
    background: #050608;
  }
  .stage :global(.feed) {
    position: absolute;
    inset: 0;
  }
  .wait {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: rgba(255, 255, 255, 0.7);
    font-size: 12px;
  }
  .note {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    margin: 0;
    font-size: 12px;
    color: var(--fg-2);
    line-height: 1.45;
  }
  .note :global(svg) {
    margin-top: 2px;
  }
</style>
