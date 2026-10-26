<script lang="ts">
  // A camera's status chain (helios-api `camera_status`): the one component every screen uses to say
  // what is happening and what to do. `why` shows the stage that stops the chain (of the named
  // stages, when `only` is given) with its reason, hint and progress. `chain` lists every stage that
  // applies, in order, and marks where it stops.
  import Meter from "#lib/kit/Meter.svelte";
  import type { CameraStatus } from "#lib/api/types.js";
  import { blockingStage, fraction, visibleStages, STATE_TONE } from "#lib/status/chain.js";

  let {
    status,
    only,
    variant = "chain",
  }: {
    status: CameraStatus | null | undefined;
    only?: readonly string[];
    variant?: "chain" | "why";
  } = $props();

  const stages = $derived(visibleStages(status, only));
  const stop = $derived(only ? (stages.find((s) => s.state === "blocked" || s.state === "waiting") ?? null) : blockingStage(status));
</script>

{#if variant === "why"}
  {#if stop}
    <div class="why" data-state={stop.state} data-stage={stop.stage} role="status">
      <span class="head"><i class="dot {STATE_TONE[stop.state]}"></i><b>{stop.label}</b> <span>{stop.reason}</span></span>
      {#if stop.progress}
        <span class="progress">
          <Meter value={fraction(stop.progress)} color="var(--info)" height={4} />
          <small>{stop.progress.value} of {stop.progress.target} {stop.progress.unit}</small>
        </span>
      {/if}
      {#if stop.hint}<small class="hint">{stop.hint}</small>{/if}
    </div>
  {/if}
{:else if status}
  <div class="chain">
    <p class="summary" data-blocking={status.blocking ?? "none"}>{status.summary}</p>
    <ol>
      {#each stages as s (s.stage)}
        <li data-stage={s.stage} data-state={s.state} class:stop={s.stage === status.blocking}>
          <i class="dot {STATE_TONE[s.state]}"></i>
          <span class="label">{s.label}</span>
          <span class="reason">{s.reason}</span>
          {#if s.progress}
            <span class="progress">
              <Meter value={fraction(s.progress)} color="var(--info)" height={4} />
              <small>{s.progress.value} of {s.progress.target} {s.progress.unit}</small>
            </span>
          {/if}
          {#if s.hint && s.state !== "ok"}<small class="hint">{s.hint}</small>{/if}
        </li>
      {/each}
    </ol>
  </div>
{/if}

<style>
  .why {
    display: grid;
    gap: 4px;
    max-width: 52ch;
    text-align: left;
    color: var(--fg-2);
    font-size: 13px;
    line-height: 1.4;
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: 6px;
    flex-wrap: wrap;
  }
  .head b {
    color: var(--fg);
  }
  .hint {
    color: var(--fg-3);
    font-size: 12px;
  }
  .progress {
    display: grid;
    gap: 3px;
    max-width: 260px;
  }
  .progress small {
    color: var(--fg-3);
    font-size: 11px;
  }
  .chain {
    display: grid;
    gap: 8px;
    text-align: left;
  }
  .summary {
    margin: 0;
    color: var(--fg);
    font-weight: 600;
  }
  ol {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: grid;
    grid-template-columns: 10px minmax(90px, auto) 1fr;
    gap: 2px 8px;
    align-items: baseline;
    font-size: 13px;
    padding: 4px 6px;
    border-radius: var(--r-1);
  }
  li.stop {
    background: var(--err-bg, transparent);
  }
  li .hint,
  li .progress {
    grid-column: 2 / -1;
  }
  .label {
    color: var(--fg);
    font-weight: 600;
  }
  .reason {
    color: var(--fg-2);
  }
  .dot {
    width: 8px;
    height: 8px;
    margin-top: 5px;
    border-radius: 50%;
    background: var(--fg-4);
  }
  .dot.success {
    background: var(--ok);
  }
  .dot.info {
    background: var(--info);
  }
  .dot.error {
    background: var(--err);
  }
</style>
