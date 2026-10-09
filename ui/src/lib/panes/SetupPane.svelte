<script lang="ts">
  // The setup checklist: what a new robot still needs, each step one click
  // from the screen that does it.
  import Icon from "#lib/components/common/Icon.svelte";
  import { guide } from "#lib/core/guide.svelte.js";
  import { shell } from "#lib/core/shell.svelte.js";
  import { pane, workspaces } from "#lib/core/workspace.svelte.js";

  const steps = $derived(guide.checklist);
  const done = $derived(steps.filter((s) => s.done).length);
  const TARGET: Record<string, () => void> = {
    name: () => shell.go("cameras"),
    place: () => shell.go("robot"),
    calibrate: () => shell.go("calibration"),
    field: () => {
      shell.go("robot");
      workspaces.open("robot", pane("field"));
    },
    "robot-code": () => shell.go("pipelines"),
  };
</script>

<div class="setup">
  <div class="head">
    <span class="bar"><span style:width="{(done / steps.length) * 100}%"></span></span>
    <span class="count">{done}/{steps.length}</span>
  </div>
  {#each steps as s, i (s.id)}
    <div class="step" class:done={s.done}>
      <span class="n">{#if s.done}<Icon name="check" size={11} stroke={3} />{:else}{i + 1}{/if}</span>
      <span class="t"><b>{s.title}</b><i>{s.detail}</i></span>
      <button type="button" onclick={TARGET[s.id]}>{s.done ? "Review" : "Open"}<Icon name="chevron-right" size={12} /></button>
    </div>
  {/each}
</div>

<style>
  .setup {
    flex: 1;
    overflow-y: auto;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--line);
  }
  .bar {
    flex: 1;
    height: 4px;
    background: var(--line);
    border-radius: 2px;
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--ok);
  }
  .count {
    font-family: var(--font-code);
    font-size: 11px;
    color: var(--fg-2);
  }
  .step {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 7px 10px;
    border-bottom: 1px solid color-mix(in oklab, var(--line) 60%, transparent);
  }
  .n {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    margin-top: 1px;
    font-size: 10.5px;
    font-weight: 700;
    color: var(--fg-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
    flex-shrink: 0;
  }
  .done .n {
    color: var(--s0);
    background: var(--ok);
    border-color: var(--ok);
  }
  .t {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .t b {
    font-size: 12.5px;
    font-weight: 600;
  }
  .done .t b {
    color: var(--fg-2);
  }
  .t i {
    font-style: normal;
    font-size: 11px;
    color: var(--fg-3);
    line-height: 1.4;
  }
  button {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 22px;
    padding: 0 6px 0 8px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--fg-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
    flex-shrink: 0;
  }
  button:hover {
    color: var(--fg);
    background: var(--s3);
  }
  .step:not(.done) button {
    color: var(--on-accent);
    background: var(--accent);
    border-color: var(--accent);
  }
</style>
