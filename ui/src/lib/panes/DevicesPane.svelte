<script lang="ts">
  // Every device in the robot, one row each, live: health, link, CPU (per
  // core), memory, temperature, clock and recovery. Click to inspect.
  import Icon from "#lib/components/common/Icon.svelte";
  import { colorVar, identity } from "#lib/core/identity.svelte.js";
  import { menu } from "#lib/core/menu.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { LINK_ICON } from "#lib/devices.js";
  import { nodeIcon } from "#lib/present.js";
  import Swatch from "#lib/kit/Swatch.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { system } from "#lib/stores/system.svelte.js";
  import type { ClusterNode } from "#lib/api/model.js";

  const HEALTH = { online: "var(--ok)", degraded: "var(--warn)", offline: "var(--err)" };

  function actions(n: ClusterNode) {
    return menu.context(() => [
      { heading: identity.name(n.id, n.name) },
      { label: "Reboot", icon: "power", run: () => cluster.reboot(n.id) },
      { label: "Safe mode", icon: "lifebuoy", hint: "Stop applications; keep the agent and control plane", run: () => cluster.safeMode(n.id) },
      { label: "Switch boot slot", icon: "versions", disabled: n.slots.length < 2, run: () => cluster.switchSlot(n.id) },
      { separator: true },
      ...n.services.map((s) => ({ label: `Restart ${s.name}`, icon: "refresh" as const, run: () => cluster.restartService(n.id, s.name) })),
    ]);
  }
  const heat = (t: number) => (t > 70 ? "var(--err)" : t > 60 ? "var(--warn)" : "var(--fg-2)");
</script>

<div class="table">
  <div class="head">
    <span></span><span>Device</span><span>Link</span><span>CPU</span><span>Memory</span><span class="r">Temp</span><span class="r">Clock</span><span>Recovery</span>
  </div>
  {#each cluster.nodes as n (n.id)}
    {@const hw = system.of(n.id)}
    {@const sel = selection.last.device?.id === n.id}
    <div
      class="row"
      class:sel
      class:off={n.health === "offline"}
      style:--c={colorVar(identity.get(n.id).color)}
      role="button"
      tabindex="0"
      onclick={() => selection.select({ kind: "device", id: n.id })}
      onkeydown={(e) => e.key === "Enter" && selection.select({ kind: "device", id: n.id })}
      oncontextmenu={actions(n)}
    >
      <span class="h" style:background={HEALTH[n.health]} data-tip={n.health}></span>
      <span class="name">
        <Swatch id={n.id} icon={nodeIcon(n.kind)} size={20} />
        <span class="nm">
          <b>{identity.name(n.id, n.name)}</b>
          <i>{n.model}</i>
        </span>
      </span>
      <span class="link" data-tip="{n.link} · {n.address}"><Icon name={LINK_ICON[n.link]} size={12} /><span>{n.address}</span></span>
      <span class="cpu">
        {#if hw}
          <span class="cores">{#each hw.cores as c, i (i)}<span class="core" data-tip="core {i}: {(c * 100).toFixed(0)}%"><span style:height="{Math.max(4, c * 100)}%" class:hot={c > 0.8}></span></span>{/each}</span>
        {/if}
        <span class="mono">{(n.cpu * 100).toFixed(0)}%</span>
      </span>
      <span class="mem">
        <span class="bar"><span style:width="{(n.memMiB / n.memTotalMiB) * 100}%"></span></span>
        <span class="mono">{n.memMiB < 1 ? `${(n.memMiB * 1024).toFixed(0)}K` : n.memMiB < 100 ? `${n.memMiB.toFixed(1)}M` : `${(n.memMiB / 1024).toFixed(1)}G`}</span>
      </span>
      <span class="r mono" style:color={heat(n.tempC)}>{n.tempC.toFixed(0)}°</span>
      <span class="r mono" class:warn={n.clock.offsetUs > 1000} data-tip="{n.clock.source} offset">{n.clock.offsetUs >= 1000 ? `${(n.clock.offsetUs / 1000).toFixed(1)}ms` : `${n.clock.offsetUs}µs`}</span>
      <span class="rec">{#each ["R1", "R2", "R3", "R4"] as r (r)}<i class:on={n.recovery.includes(r as never)}>{r}</i>{/each}</span>
    </div>
  {/each}
</div>

<style>
  .table {
    flex: 1;
    overflow: auto;
    font-size: 12px;
  }
  .head,
  .row {
    display: grid;
    grid-template-columns: 8px minmax(150px, 1.6fr) minmax(90px, 0.9fr) minmax(90px, 1fr) minmax(80px, 0.8fr) 36px 50px 96px;
    align-items: center;
    gap: 10px;
    padding: 0 10px;
  }
  .head {
    position: sticky;
    top: 0;
    z-index: 1;
    height: 24px;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--fg-3);
    background: var(--s1);
    border-bottom: 1px solid var(--line);
  }
  .row {
    position: relative;
    height: 36px;
    border-bottom: 1px solid var(--line);
  }
  .row:hover {
    background: var(--s2);
  }
  .row.sel {
    background: color-mix(in oklab, var(--c) 12%, transparent);
  }
  .row.sel::before {
    content: "";
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 2px;
    background: var(--c);
  }
  .row.off {
    opacity: 0.5;
  }
  .h {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .nm {
    display: flex;
    flex-direction: column;
    min-width: 0;
    line-height: 1.25;
  }
  .nm b {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .nm i {
    font-style: normal;
    font-size: 10.5px;
    color: var(--fg-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .link {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--fg-3);
    font-family: var(--font-code);
    font-size: 11px;
    white-space: nowrap;
    overflow: hidden;
  }
  .cpu,
  .mem {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .cores {
    display: flex;
    gap: 2px;
    height: 16px;
    flex: 1;
  }
  .core {
    flex: 1;
    max-width: 10px;
    display: flex;
    align-items: flex-end;
    background: var(--line);
    border-radius: 1.5px;
    overflow: hidden;
  }
  .core span {
    width: 100%;
    background: var(--ok);
  }
  .core span.hot {
    background: var(--warn);
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
    background: var(--info);
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11px;
    min-width: 30px;
    text-align: right;
  }
  .r {
    text-align: right;
  }
  .warn {
    color: var(--warn);
  }
  .rec {
    display: flex;
    gap: 2px;
  }
  .rec i {
    font-style: normal;
    font-family: var(--font-code);
    font-size: 9.5px;
    padding: 1px 3px;
    border-radius: 2px;
    color: var(--fg-4);
    border: 1px solid var(--line);
  }
  .rec i.on {
    color: var(--ok);
    border-color: color-mix(in oklab, var(--ok) 35%, transparent);
    background: var(--ok-bg);
  }
</style>
