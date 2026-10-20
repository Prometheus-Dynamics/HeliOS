<script lang="ts">
  // Playback: a recording or an image folder replayed as a camera. Open one,
  // then play, step and seek it, and run a pipeline on it. Its frames and
  // overlays are the same view a live camera has.
  import Icon from "#lib/components/common/Icon.svelte";
  import IconButton from "#lib/kit/IconButton.svelte";
  import { workspaces } from "#lib/core/workspace.svelte.js";
  import PlayerPanel from "#lib/playback/PlayerPanel.svelte";
  import SourcePanel from "#lib/playback/SourcePanel.svelte";
  import { playback } from "#lib/playback/store.svelte.js";
  import { sourceLabel } from "#lib/playback/sources.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "#lib/workspace/panes.js";
  import Empty from "./Empty.svelte";

  let { pane, ws }: PaneProps = $props();

  // One poll loop for every Playback pane that is up.
  $effect(() => playback.acquire());

  let opening = $state(false);
  const chosen = $derived((pane.props?.player as string | undefined) ?? undefined);
  const player = $derived(playback.players.find((p) => p.id === chosen) ?? playback.players[0]);

  /** This pane remembers the player it shows, so a layout reload keeps it. */
  function show(id: string) {
    workspaces.setProps(ws, pane.id, { player: id });
    opening = false;
  }
</script>

<PaneBar>
  <IconButton icon="plus" label="Open a recording or image folder" active={opening} size={24} onclick={() => (opening = !opening)} />
</PaneBar>

<div class="body">
  {#if playback.mock}
    <Empty icon="player-play" text="Playback needs a device. The simulated robot has no recordings to replay." />
  {:else if !playback.loaded}
    <p class="status"><Icon name="loader-2" size={14} class="spin" />Loading the players</p>
  {:else}
    {#if playback.error}
      <p class="status err"><Icon name="alert-triangle" size={14} />{playback.error}</p>
    {/if}

    {#if opening || playback.players.length === 0}
      <SourcePanel onopened={(view) => show(view.id)} oncancel={playback.players.length ? () => (opening = false) : undefined} />
    {/if}

    {#if playback.players.length > 1}
      <nav class="players" aria-label="Open players">
        {#each playback.players as p (p.id)}
          <button type="button" class="chip" class:on={player?.id === p.id} aria-current={player?.id === p.id ? "true" : undefined} onclick={() => show(p.id)} title={sourceLabel(p.source)}>
            {p.name}
          </button>
        {/each}
      </nav>
    {/if}

    {#if player}
      {#key player.id}
        <PlayerPanel {player} onclose={() => playback.close(player.id)} />
      {/key}
    {/if}
  {/if}
</div>

<style>
  .body {
    display: flex;
    flex-direction: column;
    gap: 12px;
    flex: 1;
    min-height: 0;
    min-width: 0;
    padding: 10px;
    overflow-y: auto;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    font-size: 12px;
    color: var(--fg-3);
  }
  .status.err {
    color: var(--err);
  }
  .players {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    min-height: 40px;
    padding: 0 12px;
    font-size: 12px;
    font-weight: 500;
    color: var(--fg-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-2);
  }
  .chip.on {
    color: var(--fg);
    background: var(--s3);
    border-color: var(--accent-ring);
  }
</style>
