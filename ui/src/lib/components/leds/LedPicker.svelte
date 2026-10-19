<script lang="ts">
  // Locate the device by its ring, and preview a state on it for a few seconds. Each state has a
  // line of help. The device answers each call with the whole LED state, which goes back up.
  import { api, errorText } from "#lib/api/client.js";
  import type { LedState, LedsResponse } from "#lib/api/leds.js";
  import { STATE_HELP } from "#lib/leds/states.js";
  import Icon from "#lib/components/common/Icon.svelte";
  import { toasts } from "#lib/stores/toasts.svelte.js";

  let { res, onresult }: { res: LedsResponse; onresult: (next: LedsResponse) => void } = $props();

  const LOCATE_SECONDS = 10;
  const PREVIEW_SECONDS = 4;

  let busy = $state<string | null>(null);
  let focus = $state<LedState | null>(null);

  const locating = $derived(res.locate_until_ms !== null);
  const previewing = $derived(res.preview?.state ?? null);
  const helpFor = $derived<LedState | null>(focus ?? previewing);

  async function run(key: string, call: () => Promise<LedsResponse>) {
    busy = key;
    try {
      onresult(await call());
    } catch (error) {
      toasts.error(`LEDs: ${errorText(error)}`);
    } finally {
      busy = null;
    }
  }
</script>

<div class="picker">
  <div class="row">
    <button type="button" class="btn" disabled={busy !== null} onclick={() => run("locate", () => api.locateLeds(LOCATE_SECONDS))}>
      <Icon name="target" size={13} />
      {locating ? "Locating…" : "Locate"}
    </button>
    <span class="hint">Chases the ring for {LOCATE_SECONDS} s, so you can find which device it is.</span>
  </div>

  <div class="states" role="group" aria-label="Preview a state on the ring">
    {#each res.states as s (s.name)}
      <button
        type="button"
        class="chip"
        class:on={previewing === s.name}
        disabled={busy !== null}
        aria-pressed={previewing === s.name}
        onmouseenter={() => (focus = s.name)}
        onfocus={() => (focus = s.name)}
        onclick={() => run(s.name, () => api.previewLeds(s.name, PREVIEW_SECONDS))}
      >
        {s.label}
      </button>
    {/each}
  </div>

  <div class="row">
    {#if previewing}
      <button type="button" class="btn ghost" disabled={busy !== null} onclick={() => run("stop", () => api.previewLeds(null))}>
        <Icon name="player-stop" size={13} />
        Stop preview
      </button>
    {/if}
    <span class="help" aria-live="polite">{helpFor ? STATE_HELP[helpFor] : `Preview a state for ${PREVIEW_SECONDS} s. Pick one to read what it means.`}</span>
  </div>
</div>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 6px 10px 2px;
  }
  .row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px 10px;
  }
  .states {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
  }
  .chip {
    height: 24px;
    padding: 0 9px;
    font-size: 11.5px;
    font-weight: 550;
    color: var(--fg-2);
    background: var(--s2);
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    transition:
      color var(--t-fast),
      background var(--t-fast),
      box-shadow var(--t-fast);
  }
  .chip:hover:not(:disabled) {
    color: var(--fg);
    background: var(--s3);
  }
  .chip.on {
    color: var(--fg);
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent), 0 0 12px -2px var(--accent);
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 11px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--on-accent);
    background: var(--accent);
    border-radius: var(--r-2);
    transition: opacity var(--t-fast);
  }
  .btn.ghost {
    color: var(--fg);
    background: transparent;
    border: 1px solid var(--line-strong);
  }
  .btn:disabled,
  .chip:disabled {
    opacity: 0.5;
  }
  .hint,
  .help {
    font-size: 11px;
    line-height: 1.45;
    color: var(--fg-3);
  }
  .help {
    flex: 1 1 100%;
    min-height: 2.9em;
  }
</style>
