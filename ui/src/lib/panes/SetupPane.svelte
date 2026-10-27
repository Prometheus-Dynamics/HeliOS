<script lang="ts">
  // The setup checklist: each camera's status chain as its steps (name, camera, pipeline, tags,
  // calibration, placement, then the map, depth and features it runs), and what the robot still
  // needs (the field, robot code). The step a camera is at is open, with the chain's reason, hint
  // and progress; its button opens the pane that does it, with the camera selected.
  import Icon from "#lib/components/common/Icon.svelte";
  import { guide, type CameraStep } from "#lib/core/guide.svelte.js";
  import { colorVar, identity } from "#lib/core/identity.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { shell } from "#lib/core/shell.svelte.js";
  import Meter from "#lib/kit/Meter.svelte";
  import Swatch from "#lib/kit/Swatch.svelte";
  import { fraction } from "#lib/status/chain.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import type { Camera } from "#lib/api/model.js";
  import Empty from "./Empty.svelte";

  const cams = $derived(guide.nativeCameras);
  const robotSteps = $derived(guide.checklist.filter((s) => s.id === "field" || s.id === "robot-code"));
  const all = $derived([...cams.flatMap((c) => guide.cameraSteps(c).map((s) => s.state === "ok")), ...robotSteps.map((s) => s.done)]);
  const done = $derived(all.filter(Boolean).length);
  const ROBOT_TARGET: Record<string, { screen: string; pane: string }> = { field: { screen: "setup", pane: "field" }, "robot-code": { screen: "device", pane: "robot" } };

  function go(c: Camera | null, target: { screen: string; pane: string }) {
    if (c) selection.select({ kind: "camera", id: c.resourceId });
    shell.open(target.screen, target.pane);
  }
  const ICON: Record<CameraStep["state"], "circle-check" | "loader-2" | "circle-dashed" | "alert-circle"> = { ok: "circle-check", waiting: "loader-2", todo: "circle-dashed", blocked: "alert-circle" };
</script>

<div class="setup">
  <div class="head">
    <span class="bar"><span style:width="{all.length ? (done / all.length) * 100 : 0}%"></span></span>
    <span class="count">{done}/{all.length}</span>
  </div>

  {#each cams as c (c.resourceId)}
    {@const steps = guide.cameraSteps(c)}
    {@const next = steps.find((s) => s.state !== "ok")}
    {@const sel = selection.last.camera?.id === c.resourceId}
    <section class="cam" class:sel style:--c={colorVar(identity.get(c.resourceId).color)} data-camera={c.resourceId}>
      <button type="button" class="cam-head" onclick={() => selection.select({ kind: "camera", id: c.resourceId })}>
        <Swatch id={c.resourceId} icon="camera" size={20} />
        <span class="nm"><b>{identity.name(c.resourceId, c.name)}</b><i>{next ? `Next: ${next.title.toLowerCase()}` : c.status?.summary ?? "Ready"}</i></span>
        <span class="dots" aria-label="{steps.filter((s) => s.state === 'ok').length} of {steps.length} steps done">
          {#each steps as s (s.id)}<i class="d {s.state}" title={s.title}></i>{/each}
        </span>
      </button>
      {#each steps as s, i (s.id)}
        {@const current = s === next}
        <div class="step {s.state}" class:current data-step={s.id} data-state={s.state}>
          <span class="n">{#if s.state === "ok"}<Icon name="check" size={11} stroke={3} />{:else}{i + 1}{/if}</span>
          <span class="t">
            <b>{s.title}</b>
            {#if current || s.state === "blocked" || s.state === "waiting"}
              <i>{s.detail}</i>
              {#if s.progress}
                <span class="progress"><Meter value={fraction(s.progress)} color="var(--info)" height={4} /><small>{s.progress.value} of {s.progress.target} {s.progress.unit}</small></span>
              {/if}
              {#if s.hint}<small class="hint">{s.hint}</small>{/if}
            {/if}
          </span>
          {#if current || s.state === "blocked"}
            <button type="button" class="open" class:primary={current} onclick={() => go(c, s.target)}>Open<Icon name="chevron-right" size={12} /></button>
          {:else if s.state === "ok"}
            <button type="button" class="open ghost" onclick={() => go(c, s.target)}>Review</button>
          {:else}
            <Icon name={ICON[s.state]} size={13} class="state-icon" />
          {/if}
        </div>
      {/each}
    </section>
  {:else}
    <Empty icon="camera" text="No camera yet. Plug one in: it appears here with its checklist." />
  {/each}

  <section class="robot">
    <div class="cam-head static"><Icon name="robot" size={16} /><span class="nm"><b>Robot</b><i>The field and robot code, once per robot.</i></span></div>
    {#each robotSteps as s (s.id)}
      <div class="step {s.done ? 'ok' : 'todo'}" class:current={!s.done} data-step={s.id}>
        <span class="n">{#if s.done}<Icon name="check" size={11} stroke={3} />{:else}<Icon name={s.icon} size={11} />{/if}</span>
        <span class="t"><b>{s.title}</b><i>{s.detail}</i></span>
        <button type="button" class="open" class:primary={!s.done} class:ghost={s.done} onclick={() => go(null, ROBOT_TARGET[s.id])}>{s.done ? "Review" : "Open"}<Icon name="chevron-right" size={12} /></button>
      </div>
    {/each}
  </section>
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
  section {
    position: relative;
    border-bottom: 1px solid var(--line);
    padding-bottom: 4px;
  }
  .cam.sel::before {
    content: "";
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 3px;
    background: var(--c);
  }
  .cam-head {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 10px 4px;
    text-align: left;
  }
  .cam-head.static {
    color: var(--fg-3);
  }
  .nm {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    line-height: 1.25;
  }
  .nm b {
    font-size: 13px;
    font-weight: 650;
    color: var(--fg);
  }
  .nm i {
    font-style: normal;
    font-size: 11px;
    color: var(--fg-3);
  }
  .dots {
    display: flex;
    gap: 3px;
  }
  .d {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--line-strong);
  }
  .d.ok {
    background: var(--ok);
  }
  .d.waiting {
    background: var(--info);
  }
  .d.blocked {
    background: var(--err);
  }
  .step {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 4px 10px 4px 12px;
  }
  .step.current {
    padding: 7px 10px 7px 12px;
    background: var(--accent-tint);
  }
  .step.blocked {
    background: var(--err-bg);
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
  .ok .n {
    color: var(--s0);
    background: var(--ok);
    border-color: var(--ok);
  }
  .blocked .n {
    color: #fff;
    background: var(--err);
    border-color: var(--err);
  }
  .waiting .n {
    color: var(--info);
    border-color: var(--info);
  }
  .t {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .t b {
    font-size: 12.5px;
    font-weight: 600;
    line-height: 18px;
  }
  .ok .t b,
  .todo:not(.current) .t b {
    color: var(--fg-2);
    font-weight: 500;
  }
  .t i {
    font-style: normal;
    font-size: 11.5px;
    color: var(--fg-2);
    line-height: 1.4;
  }
  .hint {
    font-size: 11px;
    color: var(--fg-3);
    line-height: 1.4;
  }
  .progress {
    display: grid;
    gap: 3px;
    max-width: 220px;
  }
  .progress small {
    font-size: 11px;
    color: var(--fg-3);
  }
  .open {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 22px;
    margin-top: 1px;
    padding: 0 6px 0 8px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--fg-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
    flex-shrink: 0;
  }
  .open:hover {
    color: var(--fg);
    background: var(--s3);
  }
  .open.primary {
    color: var(--on-accent);
    background: var(--accent);
    border-color: var(--accent);
  }
  .open.ghost {
    color: var(--fg-3);
    border-color: transparent;
  }
  .step :global(.state-icon) {
    margin-top: 3px;
    color: var(--fg-4);
  }
</style>
