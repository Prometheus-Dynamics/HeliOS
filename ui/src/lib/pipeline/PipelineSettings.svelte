<script lang="ts">
  // Everything about one pipeline, in the order it is set up: what it reads (its inputs: the
  // camera with the size and pyramid it asks for, or another pipeline's output), how its
  // detector searches, its tuning stage by stage, and where its results go and where it runs.
  // Camera controls (exposure, gain, ...) are the camera's, shared by its pipelines: not here.
  import { CATALOG_BY_ID } from "#lib/api/catalog.js";
  import { ENGINE_CPU_CORES, type Workload } from "#lib/api/model.js";
  import Icon from "#lib/components/common/Icon.svelte";
  import { capabilitiesOf } from "#lib/core/defaults.js";
  import { DICTIONARIES, guide } from "#lib/core/guide.svelte.js";
  import { colorVar, identity } from "#lib/core/identity.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { shell } from "#lib/core/shell.svelte.js";
  import { drafts } from "#lib/graph/drafts.svelte.js";
  import { CATEGORY_COLORS, CATEGORY_ICONS } from "#lib/graph/graph.js";
  import Badge from "#lib/kit/Badge.svelte";
  import Choice from "#lib/kit/Choice.svelte";
  import IconButton from "#lib/kit/IconButton.svelte";
  import IdentityEditor from "#lib/kit/IdentityEditor.svelte";
  import Num from "#lib/kit/Num.svelte";
  import Prop from "#lib/kit/Prop.svelte";
  import Section from "#lib/kit/Section.svelte";
  import Seg from "#lib/kit/Seg.svelte";
  import Spark from "#lib/kit/Spark.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import ParamRow from "./ParamRow.svelte";
  import { byGroup, isChangedParam, matchesParam } from "./params";

  let { workload: w, focusNode = null }: { workload: Workload; focusNode?: string | null } = $props();

  const STATE_TONE = { running: "ok", starting: "info", quarantined: "err", stopped: "neutral" } as const;
  const d = $derived(drafts.get(w.id));
  const caps = $derived(capabilitiesOf(w.graph));
  const camera = $derived(cluster.cameraOf(w));
  const dictionary = $derived(String(w.graph.nodes.find((n) => n.params.dictionary !== undefined)?.params.dictionary ?? ""));
  const family = $derived(dictionary.startsWith("aruco") ? ("aruco" as const) : ("apriltag" as const));

  let query = $state("");
  let changedOnly = $state(false);

  /** Each stage (node) with the parameters the filters let through. */
  const stages = $derived(
    d.nodes
      .map((n) => {
        const type = CATALOG_BY_ID[n.data.type];
        const params = (type?.params ?? []).filter((p) => matchesParam(p, query) && (!changedOnly || isChangedParam(p, n.data.params[p.name])));
        const changed = (type?.params ?? []).filter((p) => isChangedParam(p, n.data.params[p.name])).length;
        return { node: n, type, params, changed };
      })
      .filter((s) => s.type && s.type.params.length > 0 && (s.params.length > 0 || (!query && !changedOnly))),
  );
  const changedTotal = $derived(stages.reduce((sum, s) => sum + s.changed, 0));

  /** Output sizes a camera input can ask for: the camera's own, or a half or a quarter of it. */
  function sizes(id: string | undefined) {
    const c = id ? cluster.camera(id) : undefined;
    const base = { value: "", label: "Camera's own" };
    if (!c) return [base];
    const { width, height } = c.settings;
    return [base, ...[1, 2, 4].map((f) => ({ value: `${width / f}x${height / f}`, label: `${width / f}×${height / f}${f === 1 ? " (full)" : f === 2 ? " (half)" : " (quarter)"}` }))];
  }

  const producers = $derived(cluster.workloads.filter((x) => x.id !== w.id));
  /** Devices a pipeline can run on; with one device there is nothing to choose. */
  const hosts = $derived(cluster.nodes.filter((n) => n.runsPipelines || n.id === w.nodeId));
  const portsOf = (id: string | undefined) => (cluster.workload(id ?? "")?.outputs ?? []).map((o) => o.split(".").at(-1) ?? o);
</script>

<IdentityEditor id={w.id} fallbackName={w.name} fallbackIcon="schema" sub="revision {w.revision} · {cluster.node(w.nodeId)?.name}" />
<div class="actions">
  <Badge tone={STATE_TONE[w.state]} text={w.state} />
  <span class="caps">{caps.length ? caps.map((c) => c.replace("_", " ")).join(" · ") : "custom"}</span>
  <span class="grow"></span>
  {#if w.state === "running"}
    <IconButton icon="player-stop" label="Stop" size={24} onclick={() => cluster.stopWorkload(w.id)} />
  {:else}
    <IconButton icon="player-play" label="Start" size={24} tone="ok" onclick={() => cluster.restartWorkload(w.id)} />
  {/if}
  <IconButton icon="refresh" label="Restart" size={24} onclick={() => cluster.restartWorkload(w.id)} />
  <IconButton icon="history" label="Roll back" size={24} disabled={!cluster.canRollBack(w.id)} onclick={() => d.rollBack()} />
  <IconButton icon="schema" label="Open the graph" size={24} onclick={() => shell.go("pipelines")} />
</div>

<Section title="Inputs" key="pipe-inputs" count={Object.keys(w.inputs).length}>
  {#each Object.entries(w.inputs) as [name, input] (name)}
    {#if input.source === "camera"}
      <Prop label="{name} · camera" help="The camera this input reads. Its exposure, gain and other controls are the camera's own (Camera controls).">
        <Choice label="{name} camera" value={input.resourceId ?? ""} options={cluster.cameras.map((c) => ({ value: c.resourceId, label: identity.name(c.resourceId, c.name) }))} onchange={(v) => cluster.setInput(w.id, name, { resourceId: v })} />
      </Prop>
      <Prop label="Resolution" help="The size this pipeline asks of the camera. Smaller is faster; the camera keeps its own mode for other pipelines.">
        <Choice
          label="Resolution"
          value={input.outputWidth && input.outputHeight ? `${input.outputWidth}x${input.outputHeight}` : ""}
          options={sizes(input.resourceId)}
          onchange={(v) => {
            const [width, height] = v ? v.split("x").map(Number) : [undefined, undefined];
            void cluster.setInput(w.id, name, { outputWidth: width, outputHeight: height });
          }}
        />
      </Prop>
      <Prop label="Pyramid levels" help="Half-size levels attached to each frame (made by the ISP where it can). Detectors read 1; the 3D map and feature tracks read 3." changed={input.pyramid !== undefined} onreset={() => cluster.setInput(w.id, name, { pyramid: undefined })}>
        <Num value={input.pyramid ?? 0} min={0} max={4} step={1} width={56} label="Pyramid levels" onchange={(v) => cluster.setInput(w.id, name, { pyramid: v })} />
      </Prop>
    {:else}
      <Prop label="{name} · pipeline" help="Another pipeline's output this input takes, by frame.">
        <Choice label="{name} pipeline" value={input.pipeline ?? ""} options={producers.map((p) => ({ value: p.id, label: identity.name(p.id, p.name) }))} onchange={(v) => cluster.setInput(w.id, name, { source: "pipeline", pipeline: v })} />
      </Prop>
      <Prop label="Output" help="Which of the producer's outputs.">
        <Choice label="{name} output" value={input.port ?? ""} options={portsOf(input.pipeline).map((p) => ({ value: p, label: p }))} onchange={(v) => cluster.setInput(w.id, name, { source: "pipeline", port: v })} />
      </Prop>
      <Prop label="Max wait (ms)" help="How long a frame waits for the producer's value of the same frame before using its latest.">
        <Num value={input.maxWaitMs ?? 50} min={0} max={1000} step={5} width={64} label="Max wait" onchange={(v) => cluster.setInput(w.id, name, { source: "pipeline", maxWaitMs: v })} />
      </Prop>
    {/if}
  {:else}
    <div class="none">This pipeline reads no inputs.</div>
  {/each}
</Section>

{#if caps.includes("tags")}
  <Section title="Detection" key="pipe-detect">
    <Prop label="Tag family" help="The kind of tags to look for. A family other than the tags' own finds nothing at all.">
      <Choice label="Tag family" value={dictionary} options={DICTIONARIES[family]} onchange={(v) => cluster.setDictionary(w.id, v)} />
    </Prop>
    <Prop label="Search" help="Tracked: windows around known tags each frame, a full search every few frames (fast). Full: the whole frame every frame.">
      <Seg label="Search" value={w.searchMode ?? "tracked"} options={[{ value: "tracked", label: "Tracked" }, { value: "full", label: "Every frame" }]} onchange={(v) => cluster.setSearch(w.id, v as "tracked" | "full", w.fullSearchEvery)} />
    </Prop>
    {#if (w.searchMode ?? "tracked") === "tracked"}
      <Prop label="Full search every (frames)" help="How often the whole frame is searched for new tags.">
        <Num value={w.fullSearchEvery ?? 8} min={1} max={1000} step={1} width={64} label="Full search every" onchange={(v) => cluster.setSearch(w.id, "tracked", v)} />
      </Prop>
    {/if}
  </Section>
{/if}

<Section title="Tuning, by stage" key="pipe-tuning" count={changedTotal ? `${changedTotal} changed` : undefined}>
  <div class="tools">
    <input class="search" placeholder="Search settings" bind:value={query} aria-label="Search settings" />
    <button type="button" class="filter" class:on={changedOnly} aria-pressed={changedOnly} onclick={() => (changedOnly = !changedOnly)}>Changed only</button>
  </div>
  {#each stages as s (s.node.id)}
    <Section title={s.node.data.label} key="pipe-stage-{w.id}-{s.node.id}" open={s.changed > 0 || Boolean(query) || changedOnly || focusNode === s.node.id} count={s.changed ? `${s.changed} changed` : s.type!.params.length}>
      {#snippet actions()}
        <span class="cat" style:--cat={CATEGORY_COLORS[s.type!.category]}><Icon name={CATEGORY_ICONS[s.type!.category]} size={12} /></span>
      {/snippet}
      {@const filtering = Boolean(query) || changedOnly}
      <!-- Expert settings (declared advanced) fold away unless changed or searched for. -->
      {@const expert = filtering ? [] : s.params.filter((p) => p.advanced && !isChangedParam(p, s.node.data.params[p.name]))}
      {@const everyday = s.params.filter((p) => !expert.includes(p))}
      {#each byGroup(everyday) as [group, list] (group)}
        {#if group}<div class="group">{group}</div>{/if}
        {#each list as p (p.name)}
          <ParamRow param={p} value={s.node.data.params[p.name]} onchange={(v) => d.setParam(s.node.id, p.name, v)} />
        {/each}
      {/each}
      {#if expert.length}
        <Section title="Expert settings" key="pipe-expert-{w.id}-{s.node.id}" open={false} count={expert.length}>
          {#each byGroup(expert) as [group, list] (group)}
            {#if group}<div class="group">{group}</div>{/if}
            {#each list as p (p.name)}
              <ParamRow param={p} value={s.node.data.params[p.name]} onchange={(v) => d.setParam(s.node.id, p.name, v)} />
            {/each}
          {/each}
        </Section>
      {/if}
    </Section>
  {:else}
    <div class="none">{query || changedOnly ? "Nothing matches." : "No stage has settings."}</div>
  {/each}
  {#if d.dirty}
    <div class="deploy">
      <span>Unsaved tuning</span>
      <button type="button" class="ghost" onclick={() => d.discard()}>Discard</button>
      <button type="button" class="apply" onclick={() => d.deploy()}>Deploy</button>
    </div>
  {/if}
</Section>

<Section title="Output and placement" key="pipe-out">
  <Prop label="NetworkTables" help="Where robot code reads this pipeline's results.">
    <span class="mono">{camera ? (guide.table(camera) ?? "—") : "—"}</span>
  </Prop>
  {#if hosts.length > 1}
    <Prop label="Runs on" help="Moving a pipeline keeps its camera: frames travel over the network if the camera is elsewhere.">
      <Choice label="Runs on" value={w.nodeId} options={hosts.map((n) => ({ value: n.id, label: identity.name(n.id, n.name) }))} onchange={(v) => (w.nodeId = v)} />
    </Prop>
  {/if}
  <Prop label="CPU core" help="All of a camera's work runs on one core; pin it so nothing else slows it.">
    <Choice label="CPU core" value={w.cpu === null ? "auto" : String(w.cpu)} options={[{ value: "auto", label: "Auto" }, ...ENGINE_CPU_CORES.map((core) => ({ value: String(core), label: `Core ${core}` }))]} onchange={(v) => cluster.setCpu(w.id, v === "auto" ? null : Number(v))} />
    {#if w.cpuError}<Badge tone="warn" text={w.cpuError} />{:else if w.cpuApplied !== null}<span class="mono">on core {w.cpuApplied}</span>{/if}
  </Prop>
</Section>

<Section title="Performance" key="pipe-perf" open={false}>
  <Prop label="Tick p50 / p99"><span class="mono">{w.perf.tickP50Ms.toFixed(2)} / {w.perf.tickP99Ms.toFixed(2)} ms</span></Prop>
  <Prop label="Trend"><Spark values={w.perf.tickHistory} width={120} height={18} max={1.2} color={colorVar(identity.get(w.id).color)} /></Prop>
  <Prop label="Restarts"><span class="mono">{w.restarts}</span></Prop>
  {#if camera}
    <div class="link"><button type="button" onclick={() => selection.select({ kind: "camera", id: camera.resourceId })}>Camera controls for {identity.name(camera.resourceId, camera.name)} <Icon name="arrow-right" size={12} /></button></div>
  {/if}
</Section>

<style>
  .actions {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 10px 8px;
    border-bottom: 1px solid var(--line);
  }
  .caps {
    font-size: 11px;
    color: var(--fg-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .none {
    padding: 4px 10px 6px;
    font-size: 11.5px;
    color: var(--fg-3);
  }
  .tools {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 2px 10px 6px;
  }
  .search {
    flex: 1;
    min-width: 120px;
    height: 24px;
    padding: 0 8px;
    font: inherit;
    font-size: 12px;
    color: var(--fg);
    background: var(--inset);
    border: 1px solid var(--line);
    border-radius: var(--r-1);
    outline: none;
  }
  .search:focus {
    border-color: var(--accent-ring);
  }
  .filter {
    height: 24px;
    padding: 0 8px;
    font-size: 11.5px;
    color: var(--fg-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
    white-space: nowrap;
  }
  .filter.on {
    color: var(--accent-fg);
    border-color: var(--accent-ring);
    background: color-mix(in oklab, var(--accent) 12%, transparent);
  }
  .group {
    padding: 6px 10px 2px;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--fg-3);
  }
  .cat {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: var(--r-1);
    color: var(--cat);
    background: color-mix(in oklab, var(--cat) 16%, transparent);
  }
  .deploy {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 6px 10px 8px;
    padding: 6px 8px;
    font-size: 11.5px;
    color: var(--fg-2);
    border: 1px solid var(--accent-ring);
    border-radius: var(--r-2);
  }
  .deploy span {
    flex: 1;
  }
  .deploy button {
    height: 22px;
    padding: 0 10px;
    font-size: 11.5px;
    font-weight: 600;
    border-radius: var(--r-1);
  }
  .ghost {
    color: var(--fg-2);
    border: 1px solid var(--line-strong);
  }
  .apply {
    color: var(--on-accent);
    background: var(--accent);
    border: 1px solid var(--accent);
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11.5px;
  }
  .link {
    padding: 4px 10px 6px;
  }
  .link button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11.5px;
    color: var(--accent-fg);
  }
</style>
