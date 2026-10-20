<script lang="ts">
  // Picks what to replay: a stored recording from the list, or a typed path (an
  // absolute path to a .mcap file or an image folder). Open starts the player.
  import { onMount } from "svelte";
  import Icon from "#lib/components/common/Icon.svelte";
  import { errorText, api } from "#lib/api/client.js";
  import type { PlayerView, PlaybackSource, StoredRecording } from "#lib/api/types.js";
  import { bytes, duration, timeAgo } from "#lib/format.js";
  import { playback } from "./store.svelte.js";
  import { newestFirst, parsePath, recordingSource } from "./sources.js";

  let { onopened, oncancel }: { onopened: (view: PlayerView) => void; oncancel?: () => void } = $props();

  let recordings = $state<StoredRecording[]>([]);
  let loading = $state(true);
  let listError = $state<string | null>(null);
  let picked = $state<string | null>(null);
  let typed = $state("");
  let fps = $state(30);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const isPath = $derived(typed.trim() !== "");

  async function load() {
    loading = true;
    try {
      const listing = await api.recordings();
      recordings = newestFirst(listing.stored);
      listError = listing.storage_error;
    } catch (cause) {
      listError = errorText(cause);
    }
    loading = false;
  }

  onMount(() => void load());

  function pick(id: string) {
    picked = id;
    typed = "";
  }

  function typing(value: string) {
    typed = value;
    picked = null;
  }

  async function open() {
    error = null;
    let source: PlaybackSource;
    if (isPath) {
      const parsed = parsePath(typed);
      if (!parsed.ok) {
        error = parsed.error;
        return;
      }
      source = parsed.source;
    } else if (picked) {
      source = recordingSource(picked);
    } else {
      error = "Pick a recording, or type the path of a .mcap file or an image folder.";
      return;
    }
    busy = true;
    try {
      // The frame rate only means something for an image folder; a recording ignores it.
      const view = await playback.open(isPath && fps ? { source, fps } : { source });
      typed = "";
      picked = null;
      onopened(view);
    } catch (cause) {
      error = errorText(cause);
    } finally {
      busy = false;
    }
  }

  function when(stored: StoredRecording): string {
    return timeAgo(stored.modified_at_ms);
  }
</script>

<section class="source" aria-label="Open a recording">
  <header>
    <h3>Open a recording</h3>
    {#if oncancel}<button type="button" class="ghost" onclick={oncancel}>Cancel</button>{/if}
  </header>

  <div class="list-head">
    <span>Stored recordings</span>
    <button type="button" class="ghost mini" onclick={load} disabled={loading} aria-label="Refresh recordings">
      <Icon name="refresh" size={12} />{loading ? "Loading" : "Refresh"}
    </button>
  </div>

  {#if listError}
    <p class="err"><Icon name="alert-triangle" size={13} /> {listError}</p>
  {:else if !loading && recordings.length === 0}
    <p class="hint">No stored recordings. Type the path of a .mcap file or an image folder below.</p>
  {:else}
    <ul class="recordings" aria-label="Stored recordings">
      {#each recordings as stored (stored.id)}
        {@const on = picked === stored.id}
        <li>
          <button type="button" class="rec" class:on aria-pressed={on} onclick={() => pick(stored.id)}>
            <span class="rec-id mono">{stored.id}</span>
            <span class="rec-meta">
              {#if stored.summary}
                {duration(stored.summary.duration_ms / 1000)} · {stored.summary.camera}
              {:else}
                unfinished
              {/if}
              · {bytes(stored.bytes)} · {when(stored)}
            </span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <label class="field">
    <span>Or a path</span>
    <input class="input mono" type="text" placeholder="/absolute/path/to/recording.mcap or an image folder" value={typed} oninput={(e) => typing(e.currentTarget.value)} autocomplete="off" spellcheck="false" />
  </label>

  {#if isPath}
    <label class="field narrow">
      <span>Frames per second (image folders)</span>
      <input class="input" type="number" min="1" step="1" bind:value={fps} />
    </label>
  {/if}

  {#if error}
    <p class="err"><Icon name="alert-circle" size={13} /> {error}</p>
  {/if}

  <div class="actions">
    <button type="button" class="primary" onclick={open} disabled={busy || (!isPath && !picked)}>
      <Icon name="player-play" size={13} />{busy ? "Opening" : "Open"}
    </button>
  </div>
</section>

<style>
  .source {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border: 1px solid var(--line);
    border-radius: var(--r-2);
    background: var(--s1);
    min-width: 0;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
  }
  .list-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 11.5px;
    color: var(--fg-3);
  }
  .recordings {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
    max-height: 240px;
    overflow-y: auto;
  }
  .rec {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    width: 100%;
    min-height: 44px;
    padding: 6px 10px;
    text-align: left;
    border: 1px solid var(--line);
    border-radius: var(--r-2);
    background: var(--inset);
    color: var(--fg);
  }
  .rec:hover {
    border-color: var(--line-strong);
  }
  .rec.on {
    border-color: var(--accent-ring);
    box-shadow: 0 0 0 2px var(--accent-tint);
  }
  .rec-id {
    font-size: 12px;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rec-meta {
    font-size: 11px;
    color: var(--fg-3);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 11.5px;
    color: var(--fg-3);
  }
  .field.narrow {
    max-width: 240px;
  }
  .input {
    min-height: 40px;
    height: 40px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
  button.primary,
  button.ghost {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 40px;
    padding: 0 14px;
    font-size: 12.5px;
    font-weight: 600;
    border-radius: var(--r-2);
  }
  button.primary {
    color: var(--on-accent);
    background: var(--accent);
  }
  button.ghost {
    color: var(--fg);
    border: 1px solid var(--line-strong);
  }
  button.mini {
    height: 32px;
    padding: 0 10px;
    font-size: 11.5px;
    font-weight: 500;
  }
  button:disabled {
    opacity: 0.5;
  }
  .hint {
    margin: 0;
    font-size: 12px;
    color: var(--fg-3);
  }
  .err {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: 12px;
    color: var(--err);
  }
</style>
