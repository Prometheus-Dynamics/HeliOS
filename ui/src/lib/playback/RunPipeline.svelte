<script lang="ts">
  // Runs a pipeline on this player: a copy with its camera bindings moved to
  // the player. The live pipeline is not changed, and the result says where the
  // copy's calibration and mount came from.
  import { onMount } from "svelte";
  import Icon from "#lib/components/common/Icon.svelte";
  import { api, errorText } from "#lib/api/client.js";
  import type { Pipeline, PlayerView, PlaybackRun } from "#lib/api/types.js";
  import { toasts } from "#lib/stores/toasts.svelte.js";
  import { contextText } from "./sources.js";

  let { player }: { player: PlayerView } = $props();

  let pipelines = $state<Pipeline[]>([]);
  let loaded = $state(false);
  let loadError = $state<string | null>(null);
  let choice = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let result = $state<{ run: PlaybackRun; source: string } | null>(null);

  async function load() {
    try {
      pipelines = await api.pipelines();
      if (!pipelines.some((p) => p.id === choice)) choice = pipelines[0]?.id ?? "";
      loadError = null;
    } catch (cause) {
      loadError = errorText(cause);
    }
    loaded = true;
  }

  onMount(() => void load());

  async function run() {
    const source = pipelines.find((p) => p.id === choice);
    if (!source) return;
    busy = true;
    error = null;
    result = null;
    try {
      const run = await api.runOnPlayback(player.id, source.id);
      result = { run, source: source.name };
      toasts.success(`${source.name} runs on ${player.name}`);
    } catch (cause) {
      error = errorText(cause);
    } finally {
      busy = false;
    }
  }
</script>

<section class="run" aria-labelledby="run-title">
  <h3 id="run-title">Run a pipeline on this player</h3>
  <p class="hint">Runs a copy of the pipeline on this player. The live pipeline is not changed.</p>

  {#if loadError}
    <p class="err"><Icon name="alert-triangle" size={13} /> {loadError}</p>
  {:else if loaded && pipelines.length === 0}
    <p class="hint">No pipelines yet. Make one on the Pipelines screen, then run it here.</p>
  {:else if pipelines.length}
    <div class="form">
      <select class="select" bind:value={choice} aria-label="Pipeline to run" disabled={busy}>
        {#each pipelines as p (p.id)}
          <option value={p.id}>{p.name} · {p.state}</option>
        {/each}
      </select>
      <button type="button" class="primary" onclick={run} disabled={busy || !choice}>
        <Icon name="player-play" size={13} />{busy ? "Running" : "Run on this player"}
      </button>
    </div>
  {/if}

  {#if error}
    <p class="err"><Icon name="alert-circle" size={13} /> {error}</p>
  {/if}

  {#if result}
    {@const copy = result.run.pipeline}
    <div class="result" role="status">
      <p>
        <b>{copy.name}</b> is <span class="state">{copy.state}</span> on {player.name}. It is a copy of {result.source}.
      </p>
      <ul>
        <li>{contextText("calibration", result.run.context.calibration)}</li>
        <li>{contextText("mount", result.run.context.mount)}</li>
      </ul>
      <p class="hint">The live pipeline "{result.source}" is unchanged.</p>
    </div>
  {/if}
</section>

<style>
  .run {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border: 1px solid var(--line);
    border-radius: var(--r-2);
    background: var(--s1);
    min-width: 0;
  }
  h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
  }
  .hint {
    margin: 0;
    font-size: 12px;
    color: var(--fg-3);
  }
  .form {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    min-width: 0;
  }
  .form .select {
    flex: 1 1 180px;
    min-width: 0;
    width: auto;
    min-height: 40px;
    height: 40px;
  }
  button.primary {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 40px;
    padding: 0 14px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--on-accent);
    background: var(--accent);
    border-radius: var(--r-2);
  }
  button.primary:disabled {
    opacity: 0.5;
  }
  .err {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: 12px;
    color: var(--err);
  }
  .result {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 12px;
    background: var(--inset);
    border-radius: var(--r-2);
    font-size: 12.5px;
    overflow-wrap: anywhere;
  }
  .result p {
    margin: 0;
  }
  .result ul {
    margin: 0;
    padding-left: 18px;
    color: var(--fg-2);
    font-size: 12px;
  }
  .state {
    color: var(--ok);
  }
</style>
