<script lang="ts">
  // Whatever is selected, anywhere: its identity, live facts, settings and
  // actions. Everyday things first; deeper sections are closed below.
  import { CATALOG_BY_ID } from "#lib/api/catalog.js";
  import Icon from "#lib/components/common/Icon.svelte";
  import { duration } from "#lib/format.js";
  import { colorVar, identity } from "#lib/core/identity.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { shell } from "#lib/core/shell.svelte.js";
  import { drafts } from "#lib/graph/drafts.svelte.js";
  import { CATEGORY_COLORS, CATEGORY_ICONS, portColor, portTypeLabel } from "#lib/graph/graph.js";
  import Badge from "#lib/kit/Badge.svelte";
  import Choice from "#lib/kit/Choice.svelte";
  import IconButton from "#lib/kit/IconButton.svelte";
  import IdentityEditor from "#lib/kit/IdentityEditor.svelte";
  import Meter from "#lib/kit/Meter.svelte";
  import ParamControl from "#lib/kit/ParamControl.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import Section from "#lib/kit/Section.svelte";
  import Slider from "#lib/kit/Slider.svelte";
  import Spark from "#lib/kit/Spark.svelte";
  import Switch from "#lib/kit/Switch.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { system } from "#lib/stores/system.svelte.js";
  import { field } from "#lib/stores/field.svelte.js";
  import Num from "#lib/kit/Num.svelte";
  import PaneBar from "#lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "#lib/workspace/panes.js";
  import Empty from "./Empty.svelte";

  let {}: PaneProps = $props();

  const s = $derived(selection.current);
  const STATE_TONE = { running: "ok", starting: "info", quarantined: "err", stopped: "neutral" } as const;
  const HEALTH_TONE = { online: "ok", degraded: "warn", offline: "err" } as const;
</script>

<PaneBar>
  {#if s}
    <span class="kind">{s.kind.replace("-", " ")}</span>
    <IconButton icon="x" label="Clear selection" size={22} onclick={() => selection.select(null)} />
  {/if}
</PaneBar>

<div class="scroll">
  {#if s?.kind === "camera"}
    {@const c = cluster.camera(s.id)}
    {#if c}
      {@const uses = cluster.workloads.filter((w) => w.bindings.camera === c.resourceId)}
      <IdentityEditor id={c.resourceId} fallbackName={c.name} fallbackIcon="camera" sub="{c.sensor} · {cluster.node(c.nodeId)?.name}" />
      <Section title="Quick controls" key="insp-cam">
        <Prop label="Auto exposure"><Switch checked={c.settings.autoExposure} onchange={(v) => cluster.setCamera(c.resourceId, { autoExposure: v })} /></Prop>
        <Prop label="Exposure"><Slider value={c.settings.exposureUs} min={50} max={16000} step={50} unit="µs" disabled={c.settings.autoExposure} onchange={(v) => cluster.setCamera(c.resourceId, { exposureUs: v })} /></Prop>
        <Prop label="Gain"><Slider value={c.settings.gain} min={1} max={16} step={0.1} unit="×" disabled={c.settings.autoExposure} onchange={(v) => cluster.setCamera(c.resourceId, { gain: v })} /></Prop>
        <Prop label="Frame rate"><Slider value={c.settings.fps} min={1} max={120} unit="fps" onchange={(v) => cluster.setCamera(c.resourceId, { fps: v })} /></Prop>
        <div class="link"><button type="button" onclick={() => shell.go("cameras")}>All camera controls <Icon name="arrow-right" size={12} /></button></div>
      </Section>
      <Section title="Used by" key="insp-cam-uses" count={uses.length}>
        {#each uses as w (w.id)}
          <button type="button" class="ref" style:--c={colorVar(identity.get(w.id).color)} onclick={() => selection.select({ kind: "workload", id: w.id })}>
            <span class="d"></span>{identity.name(w.id, w.name)}<Badge tone={STATE_TONE[w.state]} text={w.state} />
          </button>
        {:else}
          <div class="none">No pipeline reads this camera.</div>
        {/each}
      </Section>
    {/if}
  {:else if s?.kind === "workload"}
    {@const w = cluster.workload(s.id)}
    {#if w}
      <IdentityEditor id={w.id} fallbackName={w.name} fallbackIcon="schema" sub="revision {w.revision} · {cluster.node(w.nodeId)?.name}" />
      <div class="actions">
        <Badge tone={STATE_TONE[w.state]} text={w.state} />
        <span class="grow"></span>
        {#if w.state === "running"}
          <IconButton icon="player-stop" label="Stop" text="Stop" size={24} onclick={() => cluster.stopWorkload(w.id)} />
        {:else}
          <IconButton icon="player-play" label="Start" text="Start" size={24} tone="ok" onclick={() => cluster.restartWorkload(w.id)} />
        {/if}
        <IconButton icon="refresh" label="Restart" size={24} onclick={() => cluster.restartWorkload(w.id)} />
        <IconButton icon="history" label="Roll back" size={24} disabled={!cluster.canRollBack(w.id)} onclick={() => drafts.get(w.id).rollBack()} />
      </div>
      <Section title="Inputs and placement" key="insp-wl-bind">
        <Prop label="Camera">
          <Choice value={w.bindings.camera ?? ""} options={cluster.cameras.map((c) => ({ value: c.resourceId, label: identity.name(c.resourceId, c.name) }))} onchange={(v) => cluster.bindInput(w.id, "camera", v)} />
        </Prop>
        <Prop label="Runs on" help="Moving a pipeline keeps its camera: frames travel over the network if the camera is elsewhere.">
          <Choice value={w.nodeId} options={cluster.nodes.filter((n) => n.kind === "raze" || n.id === w.nodeId).map((n) => ({ value: n.id, label: identity.name(n.id, n.name) }))} onchange={(v) => (w.nodeId = v)} />
        </Prop>
      </Section>
      <Section title="Performance" key="insp-wl-perf">
        <Prop label="Tick p50 / p99"><span class="mono">{w.perf.tickP50Ms.toFixed(2)} / {w.perf.tickP99Ms.toFixed(2)} ms</span></Prop>
        <Prop label="Trend"><Spark values={w.perf.tickHistory} width={120} height={18} max={1.2} color={colorVar(identity.get(w.id).color)} /></Prop>
        <Prop label="Memory"><span class="mono">{w.perf.memMiB.toFixed(1)} MiB</span></Prop>
        <Prop label="Restarts"><span class="mono">{w.restarts}</span></Prop>
      </Section>
      <Section title="Outputs" key="insp-wl-out" count={w.outputs.length} advanced>
        {#each w.outputs as o (o)}<Prop label="Stream"><span class="mono">{o}</span></Prop>{/each}
        <Prop label="Plugins"><span class="mono">{w.graph.requires.map((r) => r.id).join(", ")}</span></Prop>
      </Section>
    {/if}
  {:else if s?.kind === "graph-node" && s.parent}
    {@const d = drafts.get(s.parent)}
    {@const n = d.nodes.find((x) => x.id === s.id)}
    {@const t = n ? CATALOG_BY_ID[n.data.type] : undefined}
    {#if n}
      {@const w = d.workload}
      <div class="node-head" style:--cat={t ? CATEGORY_COLORS[t.category] : "var(--fg-3)"}>
        <span class="cat"><Icon name={t ? CATEGORY_ICONS[t.category] : "alert-triangle"} size={16} stroke={2} /></span>
        <div class="grow">
          <input class="label" value={n.data.label} aria-label="Node label" onchange={(e) => d.setLabel(n.id, (e.currentTarget as HTMLInputElement).value)} />
          <span class="sub">{t ? `${t.category} · ${t.plugin}` : `unknown type ${n.data.type}`} · <span class="mono">{n.id}</span></span>
        </div>
        <IconButton icon="trash" label="Delete node" tone="danger" size={24} onclick={() => { d.remove([n.id]); selection.select({ kind: "workload", id: d.workloadId }); }} />
      </div>
      {#if t?.summary}<p class="summary">{t.summary}</p>{/if}
      {#if w?.state === "running" && w.perf.nodeMs[n.id] !== undefined}
        <div class="timing">
          <Icon name="stopwatch" size={12} />
          <b class="mono">{w.perf.nodeMs[n.id] >= 0.1 ? `${w.perf.nodeMs[n.id].toFixed(2)} ms` : `${(w.perf.nodeMs[n.id] * 1000).toFixed(0)} µs`}</b>
          <Meter value={w.perf.nodeMs[n.id] / Math.max(w.perf.tickP50Ms, 0.001)} color={t ? CATEGORY_COLORS[t.category] : undefined} />
          <span>{Math.round((w.perf.nodeMs[n.id] / Math.max(w.perf.tickP50Ms, 0.001)) * 100)}%</span>
        </div>
      {/if}
      {#if t}
        <Section title="Parameters" key="insp-node-params" count={t.params.length}>
          {#each t.params as p (p.name)}
            {@const v = n.data.params[p.name] ?? p.default}
            <Prop label={p.name} help={p.help} changed={v !== p.default} onreset={() => d.setParam(n.id, p.name, p.default)}>
              <ParamControl param={p} value={v} onchange={(nv) => d.setParam(n.id, p.name, nv)} />
            </Prop>
          {:else}
            <div class="none">No parameters.</div>
          {/each}
        </Section>
        <Section title="Ports" key="insp-node-ports" advanced>
          {#each t.inputs as p (p.name)}
            <Prop label="in · {p.name}"><span class="port" style:--p={portColor(p.type)}>{portTypeLabel(p.type)}</span></Prop>
          {/each}
          {#each t.outputs as p (p.name)}
            <Prop label="out · {p.name}"><span class="port" style:--p={portColor(p.type)}>{portTypeLabel(p.type)}</span></Prop>
          {/each}
        </Section>
      {/if}
    {/if}
  {:else if s?.kind === "device"}
    {@const n = cluster.node(s.id)}
    {#if n}
      {@const hw = system.of(n.id)}
      <IdentityEditor id={n.id} fallbackName={n.name} fallbackIcon={n.kind === "mcu" ? "circuit-cell" : n.kind === "foreign" ? "aperture" : "cpu"} sub="{n.model} · {n.address}" />
      <div class="actions">
        <Badge tone={HEALTH_TONE[n.health]} text={n.health} />
        <span class="mono dim">up {duration(n.uptimeS)}</span>
        <span class="grow"></span>
        <IconButton icon="power" label="Reboot" size={24} onclick={() => cluster.reboot(n.id)} />
        <IconButton icon="lifebuoy" label="Safe mode" size={24} onclick={() => cluster.safeMode(n.id)} />
      </div>
      <Section title="Load" key="insp-dev-load">
        <Prop label="CPU"><Meter value={n.cpu} /><span class="mono w">{(n.cpu * 100).toFixed(0)}%</span></Prop>
        <Prop label="Memory"><Meter value={n.memMiB / n.memTotalMiB} /><span class="mono w">{n.memMiB < 10 ? n.memMiB.toFixed(2) : n.memMiB.toFixed(0)} MiB</span></Prop>
        <Prop label="Temperature"><Meter value={(n.tempC - 20) / 65} warn={0.6} crit={0.8} /><span class="mono w">{n.tempC.toFixed(0)} °C</span></Prop>
        {#if hw}
          <Prop label="Cores">
            <span class="cores">{#each hw.cores as c, i (i)}<span class="core" data-tip="core {i} · {(c * 100).toFixed(0)}%"><span style:height="{Math.max(6, c * 100)}%"></span></span>{/each}</span>
          </Prop>
        {/if}
      </Section>
      <Section title="Services" key="insp-dev-svc" count={n.services.length}>
        {#each n.services as sv (sv.name)}
          <Prop label={sv.name}>
            <Badge tone={sv.state === "running" ? "ok" : sv.state === "failed" ? "err" : "neutral"} text={sv.state} />
            <IconButton icon="refresh" label="Restart {sv.name}" size={20} onclick={() => cluster.restartService(n.id, sv.name)} />
          </Prop>
        {/each}
      </Section>
    {/if}
  {:else if s?.kind === "process"}
    {@const [nodeId, pidStr] = s.id.split(":")}
    {@const p = system.of(nodeId)?.procs.find((x) => x.pid === Number(pidStr))}
    {#if p}
      <div class="node-head" style:--cat="var(--info)">
        <span class="cat"><Icon name="terminal-2" size={16} /></span>
        <div class="grow"><b class="ptitle">{p.name}</b><span class="sub">pid {p.pid} · {identity.name(nodeId, cluster.node(nodeId)?.name ?? nodeId)}</span></div>
      </div>
      <div class="actions">
        <Badge tone={p.state === "running" ? "ok" : "warn"} text={p.state} />
        <span class="grow"></span>
        <IconButton icon={p.state === "stopped" ? "player-play" : "player-pause"} label={p.state === "stopped" ? "Resume" : "Pause"} size={24} onclick={() => system.setState(nodeId, p.pid, p.state === "stopped" ? "running" : "stopped")} />
        <IconButton icon="x" label="Terminate" size={24} onclick={() => system.kill(nodeId, p.pid, "TERM")} />
        <IconButton icon="skull" label="Kill" tone="danger" size={24} onclick={() => system.kill(nodeId, p.pid, "KILL")} />
      </div>
      <Section title="Process" key="insp-proc">
        <Prop label="CPU"><Meter value={p.cpu} /><span class="mono w">{(p.cpu * 100).toFixed(1)}%</span></Prop>
        <Prop label="Memory"><span class="mono">{p.memMiB.toFixed(1)} MiB</span></Prop>
        <Prop label="Threads"><span class="mono">{p.threads}</span></Prop>
        <Prop label="Owner"><span>{p.owner.kind} · {p.owner.id}</span></Prop>
        <Prop label="Core" help="Pin to one core to keep vision timing steady.">
          <Choice value={p.core === null ? "any" : String(p.core)} options={[{ value: "any", label: "Any core" }, ...(system.of(nodeId)?.cores ?? []).map((_, i) => ({ value: String(i), label: `Core ${i}` }))]} onchange={(v) => system.pin(nodeId, p.pid, v === "any" ? null : Number(v))} />
        </Prop>
        <Prop label="Priority (nice)"><Slider value={p.nice} min={-20} max={19} onchange={(v) => system.renice(nodeId, p.pid, v)} /></Prop>
      </Section>
    {:else}
      <Empty icon="terminal-2" text="That process has exited." />
    {/if}
  {:else if s?.kind === "tag" && !s.parent}
    {@const t = field.layout.tags.find((x) => String(x.id) === s.id)}
    {#if t}
      <div class="node-head" style:--cat="#22d3ee">
        <span class="cat"><Icon name="target" size={16} /></span>
        <div class="grow"><b class="ptitle">Field tag {t.id}</b><span class="sub">{field.layout.name} · {field.layout.family}</span></div>
        <IconButton icon="trash" label="Remove tag" tone="danger" size={24} onclick={() => { field.remove(t.id); selection.select(null); }} />
      </div>
      <Section title="Pose on the field" key="insp-ftag">
        <Prop label="ID"><Num value={t.id} min={0} max={586} label="Tag id" onchange={(v) => field.update(t.id, { id: v })} /></Prop>
        <Prop label="x · y"><Num value={t.x} step={0.01} unit="m" width={76} label="x" onchange={(v) => field.update(t.id, { x: v })} /><Num value={t.y} step={0.01} unit="m" width={76} label="y" onchange={(v) => field.update(t.id, { y: v })} /></Prop>
        <Prop label="Height"><Num value={t.z} step={0.01} unit="m" width={76} label="z" onchange={(v) => field.update(t.id, { z: v })} /></Prop>
        <Prop label="Facing"><Slider value={t.yaw} min={-180} max={180} step={1} unit="°" onchange={(v) => field.update(t.id, { yaw: v })} /></Prop>
        <Prop label="Tilt"><Slider value={t.pitch} min={-90} max={90} step={1} unit="°" onchange={(v) => field.update(t.id, { pitch: v })} /></Prop>
      </Section>
    {/if}
  {:else if s?.kind === "tag"}
    {@const cam = s.parent ? cluster.camera(s.parent) : undefined}
    {@const pose = cam ? cluster.posesFor(cam).find((p) => String(p.id) === s.id) : undefined}
    <div class="node-head" style:--cat="#22d3ee">
      <span class="cat"><Icon name="target" size={16} /></span>
      <div class="grow"><b class="ptitle">Tag {s.id}</b><span class="sub">seen by {cam ? identity.name(cam.resourceId, cam.name) : "—"}</span></div>
    </div>
    <Section title="Pose in camera" key="insp-tag">
      {#if pose}
        <Prop label="Distance"><span class="mono">{Math.hypot(...pose.t).toFixed(2)} m</span></Prop>
        <Prop label="x · y · z"><span class="mono">{pose.t.map((v) => v.toFixed(2)).join(" · ")} m</span></Prop>
        <Prop label="roll · pitch · yaw"><span class="mono">{pose.r.map((v) => ((v * 180) / Math.PI).toFixed(1)).join(" · ")}°</span></Prop>
      {:else}
        <div class="none">Not in view this frame.</div>
      {/if}
    </Section>
  {:else}
    <Empty icon="click" text="Select a camera, pipeline, node, device, process or tag to inspect it here." />
  {/if}
</div>

<style>
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }
  .kind {
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--fg-3);
    margin-right: 4px;
    white-space: nowrap;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 10px 8px;
    border-bottom: 1px solid var(--line);
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .link {
    padding: 4px 10px 0;
  }
  .link button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11.5px;
    color: var(--accent-fg);
  }
  .ref {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 4px 10px;
    font-size: 12px;
    text-align: left;
  }
  .ref:hover {
    background: var(--s2);
  }
  .ref .d {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--c);
  }
  .ref :global(.badge) {
    margin-left: auto;
  }
  .none {
    padding: 4px 10px 6px;
    font-size: 11.5px;
    color: var(--fg-3);
  }
  .node-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
  }
  .cat {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: var(--r-2);
    color: var(--cat);
    background: color-mix(in oklab, var(--cat) 16%, transparent);
    flex-shrink: 0;
  }
  .label {
    width: 100%;
    padding: 1px 4px;
    margin-left: -4px;
    font: inherit;
    font-size: 14px;
    font-weight: 650;
    color: var(--fg);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--r-1);
    outline: none;
  }
  .label:hover {
    border-color: var(--line);
  }
  .label:focus {
    border-color: var(--accent-ring);
    background: var(--inset);
  }
  .ptitle {
    display: block;
    font-size: 14px;
    font-weight: 650;
  }
  .sub {
    display: block;
    font-size: 11px;
    color: var(--fg-3);
  }
  .summary {
    margin: 0;
    padding: 0 10px 8px;
    font-size: 11.5px;
    color: var(--fg-2);
    line-height: 1.45;
  }
  .timing {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px 8px;
    font-size: 11px;
    color: var(--fg-3);
    border-bottom: 1px solid var(--line);
  }
  .timing b {
    color: var(--fg);
    white-space: nowrap;
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11.5px;
  }
  .dim {
    color: var(--fg-3);
  }
  .w {
    min-width: 58px;
    text-align: right;
  }
  .port {
    font-family: var(--font-code);
    font-size: 11px;
    padding: 0 6px;
    border-radius: var(--r-1);
    color: var(--p);
    background: color-mix(in oklab, var(--p) 14%, transparent);
  }
  .cores {
    display: flex;
    gap: 3px;
    height: 18px;
    width: 100%;
  }
  .core {
    flex: 1;
    display: flex;
    align-items: flex-end;
    background: var(--line);
    border-radius: 2px;
    overflow: hidden;
  }
  .core span {
    width: 100%;
    background: var(--ok);
  }
</style>
