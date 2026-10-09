<script lang="ts">
  // Every process on the robot (or one device): sort, filter, pause, kill,
  // pin to a core. Per-core load across the top.
  import Icon from "#lib/components/common/Icon.svelte";
  import { colorVar, identity } from "#lib/core/identity.svelte.js";
  import { menu } from "#lib/core/menu.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { workspaces } from "#lib/core/workspace.svelte.js";
  import IconButton from "#lib/kit/IconButton.svelte";
  import Picker from "#lib/kit/Picker.svelte";
  import Seg from "#lib/kit/Seg.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { system, type Proc } from "#lib/stores/system.svelte.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "#lib/workspace/panes.js";
  import { follow } from "./follow.svelte";

  let { pane, ws }: PaneProps = $props();
  const target = follow(() => pane, () => ws, "device", () => cluster.nodes[0]?.id);
  const scope = $derived((pane.props?.scope as "device" | "all") ?? "device");
  const node = $derived(target.id ? cluster.node(target.id) : undefined);
  const hw = $derived(node ? system.of(node.id) : undefined);
  let sortKey = $state<"cpu" | "mem" | "name" | "pid">("cpu");
  let query = $state("");
  let showKernel = $state(false);

  const procs = $derived.by(() => {
    const all = scope === "all" ? system.allProcs : (hw?.procs ?? []);
    const q = query.toLowerCase();
    const list = all.filter((p) => (showKernel || p.owner.kind !== "kernel") && (!q || p.name.toLowerCase().includes(q)));
    const cmp: Record<typeof sortKey, (a: Proc, b: Proc) => number> = {
      cpu: (a, b) => b.cpu - a.cpu,
      mem: (a, b) => b.memMiB - a.memMiB,
      name: (a, b) => a.name.localeCompare(b.name),
      pid: (a, b) => a.pid - b.pid,
    };
    return [...list].sort(cmp[sortKey]);
  });

  const OWNER_ICON = { service: "settings", workload: "schema", kernel: "cpu", user: "device-laptop" } as const;

  function actions(p: Proc) {
    const cores = system.of(p.nodeId)?.cores ?? [];
    return [
      { heading: `${p.name} · ${p.pid}` },
      p.state === "stopped"
        ? { label: "Resume", icon: "player-play" as const, run: () => system.setState(p.nodeId, p.pid, "running") }
        : { label: "Pause", icon: "player-pause" as const, run: () => system.setState(p.nodeId, p.pid, "stopped") },
      { label: "Terminate", icon: "x" as const, hint: "SIGTERM: ask it to exit", run: () => system.kill(p.nodeId, p.pid, "TERM") },
      { label: "Kill", icon: "skull" as const, danger: true, hint: "SIGKILL", run: () => system.kill(p.nodeId, p.pid, "KILL") },
      { separator: true as const },
      { label: "Pin to core", icon: "cpu" as const, items: [{ label: "Any core", checked: p.core === null, run: () => system.pin(p.nodeId, p.pid, null) }, ...cores.map((_, i) => ({ label: `Core ${i}`, checked: p.core === i, run: () => system.pin(p.nodeId, p.pid, i) }))] },
      { label: "Priority", icon: "gauge" as const, items: [-10, -5, 0, 5, 10].map((v) => ({ label: `nice ${v}`, checked: p.nice === v, run: () => system.renice(p.nodeId, p.pid, v) })) },
      ...(p.owner.kind === "workload" ? [{ separator: true as const }, { label: "Open pipeline", icon: "schema" as const, run: () => selection.select({ kind: "workload", id: p.owner.id }) }] : []),
    ];
  }
</script>

<PaneBar>
  {#if scope === "device"}
    <Picker
      label="Device"
      icon="cpu"
      value={node?.id}
      options={cluster.nodes.map((n) => ({ id: n.id, name: identity.name(n.id, n.name) }))}
      onpick={(id) => (target.pinned ? target.pin(id) : selection.select({ kind: "device", id }))}
      pinned={target.pinned}
      ontogglepin={() => target.toggle()}
    />
  {/if}
  <Seg label="Scope" value={scope} options={[{ value: "device", label: "Device" }, { value: "all", label: "Whole robot" }]} onchange={(v) => workspaces.setProps(ws, pane.id, { scope: v })} />
  <input class="q" placeholder="Filter…" bind:value={query} aria-label="Filter processes" />
  <IconButton icon="cpu" label={showKernel ? "Hide kernel threads" : "Show kernel threads"} active={showKernel} size={24} onclick={() => (showKernel = !showKernel)} />
</PaneBar>

{#if scope === "device" && hw && node}
  <div class="cores">
    {#each hw.cores as c, i (i)}
      {@const hist = hw.coreHistory[i]}
      <div class="core">
        <div class="ch">
          <span>core {i}</span>
          <b class:hot={c > 0.8}>{(c * 100).toFixed(0)}%</b>
          <i>{hw.freqMHz[i]} MHz</i>
        </div>
        <svg viewBox="0 0 40 20" preserveAspectRatio="none">
          <path d="M0,20 {hist.map((v, k) => `L${k},${20 - v * 19}`).join(' ')} L{hist.length - 1},20Z" class="area" />
          <path d={hist.map((v, k) => `${k ? "L" : "M"}${k},${20 - v * 19}`).join(" ")} class="ln" />
        </svg>
        <div class="pins">
          {#each hw.procs.filter((p) => p.core === i) as p (p.pid)}<span data-tip="{p.name} pinned here">{p.name.replace("runner:", "")}</span>{/each}
        </div>
      </div>
    {/each}
  </div>
{/if}

<div class="table">
  <div class="head">
    {#each [["pid", "PID"], ["name", "Process"]] as [k, l] (k)}
      <button type="button" class:on={sortKey === k} onclick={() => (sortKey = k as typeof sortKey)}>{l}</button>
    {/each}
    {#if scope === "all"}<span>Device</span>{/if}
    <span>State</span>
    <button type="button" class="r" class:on={sortKey === "cpu"} onclick={() => (sortKey = "cpu")}>CPU</button>
    <button type="button" class="r" class:on={sortKey === "mem"} onclick={() => (sortKey = "mem")}>Mem</button>
    <span class="r">Thr</span><span class="r">Core</span><span class="r">Nice</span><span></span>
  </div>
  {#each procs as p (p.nodeId + p.pid)}
    {@const sel = selection.is("process", `${p.nodeId}:${p.pid}`)}
    <div
      class="row"
      class:all={scope === "all"}
      class:sel
      class:stopped={p.state === "stopped"}
      role="button"
      tabindex="0"
      onclick={() => selection.select({ kind: "process", id: `${p.nodeId}:${p.pid}` })}
      onkeydown={(e) => e.key === "Enter" && selection.select({ kind: "process", id: `${p.nodeId}:${p.pid}` })}
      oncontextmenu={menu.context(() => actions(p))}
    >
      <span class="mono dim">{p.pid}</span>
      <span class="nm"><Icon name={OWNER_ICON[p.owner.kind]} size={12} /><b>{p.name}</b></span>
      {#if scope === "all"}<span class="dev" style:color={colorVar(identity.get(p.nodeId).color)}>{identity.name(p.nodeId, cluster.node(p.nodeId)?.name ?? p.nodeId)}</span>{/if}
      <span class="state s-{p.state}">{p.state}</span>
      <span class="r cpu">
        <span class="bar"><span style:width="{Math.min(100, p.cpu * 100)}%"></span></span>
        <span class="mono">{(p.cpu * 100).toFixed(p.cpu < 0.1 ? 1 : 0)}</span>
      </span>
      <span class="r mono">{p.memMiB < 1 ? (p.memMiB * 1024).toFixed(0) + "K" : p.memMiB.toFixed(1)}</span>
      <span class="r mono dim">{p.threads}</span>
      <span class="r mono" class:dim={p.core === null}>{p.core ?? "·"}</span>
      <span class="r mono dim">{p.nice}</span>
      <span class="acts">
        <IconButton icon={p.state === "stopped" ? "player-play" : "player-pause"} label={p.state === "stopped" ? "Resume" : "Pause"} size={20} onclick={(e) => { e.stopPropagation(); system.setState(p.nodeId, p.pid, p.state === "stopped" ? "running" : "stopped"); }} />
        <IconButton icon="x" label="Terminate" tone="danger" size={20} onclick={(e) => { e.stopPropagation(); system.kill(p.nodeId, p.pid); }} />
      </span>
    </div>
  {/each}
</div>

<style>
  .q {
    width: 110px;
    height: 22px;
    padding: 0 7px;
    font: inherit;
    font-size: 11.5px;
    color: var(--fg);
    background: var(--inset);
    border: 1px solid var(--line);
    border-radius: var(--r-1);
    outline: none;
  }
  .cores {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(110px, 1fr));
    gap: 1px;
    background: var(--line);
    border-bottom: 1px solid var(--line);
    flex-shrink: 0;
  }
  .core {
    padding: 5px 8px 4px;
    background: var(--s1);
  }
  .ch {
    display: flex;
    align-items: baseline;
    gap: 6px;
    font-size: 10.5px;
    color: var(--fg-3);
  }
  .ch b {
    font-family: var(--font-code);
    font-size: 12px;
    color: var(--fg);
  }
  .ch b.hot {
    color: var(--warn);
  }
  .ch i {
    margin-left: auto;
    font-style: normal;
    font-family: var(--font-code);
    font-size: 10px;
    color: var(--fg-4);
  }
  .core svg {
    display: block;
    width: 100%;
    height: 22px;
    margin-top: 2px;
  }
  .area {
    fill: color-mix(in oklab, var(--ok) 18%, transparent);
  }
  .ln {
    fill: none;
    stroke: var(--ok);
    stroke-width: 0.8;
    vector-effect: non-scaling-stroke;
  }
  .pins {
    display: flex;
    gap: 3px;
    min-height: 15px;
    margin-top: 2px;
    overflow: hidden;
  }
  .pins span {
    font-size: 9.5px;
    padding: 0 4px;
    border-radius: 2px;
    color: var(--info);
    background: var(--info-bg);
    white-space: nowrap;
  }
  .table {
    flex: 1;
    overflow: auto;
    font-size: 12px;
  }
  .head,
  .row {
    display: grid;
    grid-template-columns: 50px minmax(140px, 2fr) 64px 90px 54px 34px 38px 38px 48px;
    align-items: center;
    gap: 8px;
    padding: 0 6px 0 10px;
  }
  .head:has(+ .row.all),
  .row.all {
    grid-template-columns: 50px minmax(140px, 2fr) 90px 64px 90px 54px 34px 38px 38px 48px;
  }
  .head {
    position: sticky;
    top: 0;
    height: 24px;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--fg-3);
    background: var(--s1);
    border-bottom: 1px solid var(--line);
    z-index: 1;
  }
  .head button {
    text-align: left;
    font: inherit;
    color: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
  }
  .head button.r,
  .r {
    text-align: right;
    justify-content: flex-end;
  }
  .head button.on {
    color: var(--accent-fg);
  }
  .row {
    height: 26px;
    border-bottom: 1px solid color-mix(in oklab, var(--line) 60%, transparent);
  }
  .row:hover {
    background: var(--s2);
  }
  .row.sel {
    background: var(--accent-tint);
  }
  .row.stopped {
    opacity: 0.6;
  }
  .nm {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    color: var(--fg-3);
  }
  .nm b {
    color: var(--fg);
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dev {
    font-size: 11.5px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .state {
    font-size: 11px;
    color: var(--fg-3);
  }
  .s-running {
    color: var(--ok);
  }
  .s-stopped {
    color: var(--warn);
  }
  .cpu {
    display: flex;
    align-items: center;
    gap: 6px;
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
  .mono {
    font-family: var(--font-code);
    font-size: 11px;
  }
  .dim {
    color: var(--fg-3);
  }
  .acts {
    display: flex;
    justify-content: flex-end;
    opacity: 0;
  }
  .row:hover .acts,
  .row.sel .acts {
    opacity: 1;
  }
</style>
