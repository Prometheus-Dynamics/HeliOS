<script lang="ts">
  // The pipeline viewer and editor: the graph with each node's live time, the plan's device
  // transfers drawn on the edges they cross and listed below, and a header with the tick against
  // the frame budget. Follows the selected pipeline, or stays pinned to one.
  import PlanSection from "#lib/components/pipeline/PlanSection.svelte";
  import { SvelteFlowProvider } from "@xyflow/svelte";
  import { identity } from "#lib/core/identity.svelte.js";
  import { menu } from "#lib/core/menu.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { drafts } from "#lib/graph/drafts.svelte.js";
  import GraphCanvas from "#lib/graph/GraphCanvas.svelte";
  import { TEMPLATES, matchNodes, tableFor, templateGraph } from "#lib/graph/graph.js";
  import Icon from "#lib/components/common/Icon.svelte";
  import Badge from "#lib/kit/Badge.svelte";
  import IconButton from "#lib/kit/IconButton.svelte";
  import Meter from "#lib/kit/Meter.svelte";
  import Picker from "#lib/kit/Picker.svelte";
  import type { GraphDocument } from "#lib/api/model.js";
  import { plans, transferKey, transfersOf, type Transfer } from "#lib/pipeline/plan.svelte.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { toasts } from "#lib/stores/toasts.svelte.js";
  import { downloadJson, pickJson } from "#lib/files.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "#lib/workspace/panes.js";
  import Empty from "./Empty.svelte";
  import { followPipeline } from "./follow.svelte";

  let { pane, ws }: PaneProps = $props();
  const target = followPipeline(() => pane, () => ws, (camera) => cluster.pipelineOf(camera), () => cluster.workloads[0]?.id);
  const workload = $derived(target.id ? cluster.workload(target.id) : undefined);
  const draft = $derived(workload ? drafts.get(workload.id) : undefined);
  const STATE_TONE = { running: "ok", starting: "info", quarantined: "err", stopped: "neutral" } as const;
  /** The per-frame budget on the CM5 (ms): the header shows the tick against it. */
  const BUDGET_MS = 1;

  // The plan, per revision: the transfers it names are drawn on the graph's edges.
  const transfers = $derived(workload?.state === "running" ? transfersOf(plans.get(workload.id).plan) : []);
  const transferByEdge = $derived(new Map<string, Transfer>(transfers.map((t) => [transferKey(t.edge.from_node_id, t.edge.from_port, t.edge.to_node_id, t.edge.to_port), t])));

  // Find box: Enter steps through the matches, each one centred and selected.
  let query = $state("");
  let at = $state(-1);
  const matches = $derived(draft && query.trim() ? matchNodes(draft.nodes, query) : []);

  function jump() {
    if (!draft || !workload || !matches.length) return;
    at = (at + 1) % matches.length;
    const node = matches[at];
    draft.focusNode(node.id);
    selection.select({ kind: "graph-node", id: node.id, parent: workload.id });
  }
  function findKey(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      jump();
    } else if (event.key === "Escape") {
      query = "";
      at = -1;
    }
  }

  function templates(e: MouseEvent) {
    if (!draft || !workload) return;
    menu.below(e.currentTarget as Element, [
      { heading: "Replace with a template" },
      ...TEMPLATES.map((t) => ({ label: t.title, icon: t.icon, hint: t.summary, run: () => draft.replace(templateGraph(t.id, tableFor(workload.id))) })),
    ]);
  }

  function exportGraph() {
    if (!draft || !workload) return;
    const doc = { ...draft.document, metadata: { name: workload.name, exported_from: workload.id, revision: workload.revision } };
    downloadJson(`${workload.name.toLowerCase().replace(/\W+/g, "-")}.graph.json`, doc);
  }

  async function importGraph() {
    if (!draft) return;
    const doc = await pickJson<GraphDocument & { schema_version?: number }>();
    if (!doc || doc.format !== "daedalus.graph" || !Array.isArray(doc.nodes)) return toasts.error("That file is not a versioned daedalus.graph document");
    draft.replace(doc);
    const unknown = draft.unknown.length;
    toasts[unknown ? "warning" : "success"](unknown ? `Imported, but ${unknown} node type(s) are not installed on this robot` : `Imported ${doc.nodes.length} nodes`);
  }
</script>

<PaneBar>
  <Picker
    label="Pipeline"
    icon="schema"
    value={workload?.id}
    options={cluster.workloads.map((w) => ({ id: w.id, name: identity.name(w.id, w.name), hint: cluster.node(w.nodeId)?.name }))}
    onpick={(id) => (target.pinned ? target.pin(id) : selection.select({ kind: "workload", id }))}
    pinned={target.pinned}
    ontogglepin={() => target.toggle()}
  />
  {#if workload && draft}
    <Badge tone={STATE_TONE[workload.state]} text="{workload.state} · r{workload.revision}" />
    {#if draft.dirty}<Badge tone="accent" text="draft" />{/if}
    <span class="sep"></span>
    <label class="find">
      <Icon name="search" size={12} />
      <input
        type="search"
        placeholder="Find a node"
        aria-label="Find a node by label or type"
        bind:value={query}
        oninput={() => (at = -1)}
        onkeydown={findKey}
      />
      {#if query.trim()}<span class="count mono">{matches.length ? `${at + 1}/${matches.length}` : "0"}</span>{/if}
    </label>
    <IconButton icon="maximize" label="Fit to view" size={24} disabled={!draft.nodes.length} onclick={() => draft.fit()} />
    <IconButton icon="layout-distribute-horizontal" label="Auto-arrange" shortcut="Ctrl L" size={24} onclick={() => draft.layout()} />
    <span class="sep"></span>
    <IconButton icon="arrow-back-up" label="Undo" shortcut="Ctrl Z" size={24} disabled={!draft.canUndo} onclick={() => draft.undo()} />
    <IconButton icon="arrow-forward-up" label="Redo" shortcut="Ctrl Shift Z" size={24} disabled={!draft.canRedo} onclick={() => draft.redo()} />
    <IconButton icon="stack-2" label="Templates" size={24} onclick={templates} />
    <IconButton icon="file-import" label="Import graph" size={24} onclick={importGraph} />
    <IconButton icon="file-export" label="Export graph" size={24} onclick={exportGraph} />
    <span class="sep"></span>
    {#if draft.dirty}
      <IconButton icon="x" label="Discard changes" size={24} onclick={() => draft.discard()} />
    {:else if cluster.canRollBack(workload.id)}
      <IconButton icon="history" label="Roll back to revision {workload.revision - 1}" size={24} onclick={() => draft.rollBack()} />
    {/if}
    <IconButton icon="upload" label="Deploy as revision {workload.revision + 1}" shortcut="Ctrl S" text="Deploy" tone={draft.dirty ? "accent" : "default"} disabled={!draft.dirty} size={24} onclick={() => draft.deploy()} />
  {/if}
</PaneBar>

{#if workload && draft}
  {#if workload.state === "quarantined"}
    <div class="banner">
      <b>{workload.status?.reason ?? "Quarantined"}</b> {workload.status?.hint ?? `after ${workload.restarts} restarts. The rest of the robot is unaffected.`}
      <button type="button" onclick={() => draft.rollBack()}>Roll back</button>
      <button type="button" onclick={() => cluster.restartWorkload(workload.id)}>Restart</button>
    </div>
  {:else if workload.state !== "running" && workload.status && workload.status.state !== "ok"}
    <div class="banner info">
      <b>{workload.status.reason}</b> {workload.status.hint ?? ""}
    </div>
  {/if}
  {#if workload.state === "running"}
    {@const p50 = workload.perf.tickP50Ms}
    <div class="tick" data-tip="The pipeline's tick: median and 99th percentile, its rate, and the median's share of the {BUDGET_MS} ms per-frame budget">
      <span class="kpi"><b class="mono">{p50.toFixed(2)}</b> ms p50</span>
      <span class="kpi"><b class="mono">{workload.perf.tickP99Ms.toFixed(2)}</b> ms p99</span>
      <span class="kpi"><b class="mono">{workload.perf.fps}</b> fps</span>
      <span class="budget"><Meter value={p50 / BUDGET_MS} warn={0.75} crit={1} height={5} /><small>{Math.round((p50 / BUDGET_MS) * 100)}% of {BUDGET_MS} ms</small></span>
      <span class="kpi" class:warn={transfers.length > 0}><Icon name="arrows-transfer-down" size={13} />{transfers.length === 0 ? "no transfers" : `${transfers.length} ${transfers.length === 1 ? "transfer" : "transfers"}`}</span>
    </div>
  {/if}
  {#key workload.id}
    <SvelteFlowProvider>
      <GraphCanvas {draft} transfers={transferByEdge} />
    </SvelteFlowProvider>
  {/key}
  <PlanSection pipelineId={workload.id} revision={workload.revision} running={workload.state === "running"} />
{:else if cluster.workloads.length}
  <Empty icon="schema" text="Pick a pipeline in the tab bar, or click one in the Pipelines list." />
{:else}
  <Empty icon="schema" text="No pipeline yet. Choose what each camera runs on the Set up screen and its graph appears here." />
{/if}

<style>
  .find {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    width: 168px;
    height: 24px;
    box-sizing: border-box;
    padding: 0 7px;
    color: var(--fg-3);
    background: var(--s2);
    border: 1px solid var(--line);
    border-radius: var(--r-1);
    flex-shrink: 1;
    min-width: 0;
  }
  .find:focus-within {
    border-color: var(--accent-ring);
  }
  .find input {
    flex: 1;
    min-width: 0;
    background: transparent;
    border: 0;
    outline: none;
    font: inherit;
    font-size: 12px;
    color: var(--fg);
  }
  .find input::placeholder {
    color: var(--fg-faint);
  }
  .count {
    font-size: 10.5px;
    color: var(--fg-faint);
  }
  .banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 10px;
    font-size: 12px;
    color: var(--err);
    background: var(--err-bg);
    border-bottom: 1px solid color-mix(in oklab, var(--err) 30%, transparent);
  }
  .banner.info {
    color: var(--fg-2);
    background: var(--info-bg);
    border-bottom-color: color-mix(in oklab, var(--info) 30%, transparent);
  }
  .banner b {
    color: var(--fg);
  }
  .banner button {
    padding: 2px 8px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--fg);
    border: 1px solid color-mix(in oklab, var(--err) 40%, transparent);
    border-radius: var(--r-1);
  }
  .banner button:first-of-type {
    margin-left: auto;
  }
  .tick {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 14px;
    padding: 5px 10px;
    font-size: 11.5px;
    color: var(--fg-3);
    border-bottom: 1px solid var(--line);
    background: var(--s1);
  }
  .kpi {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }
  .kpi b {
    color: var(--fg);
    font-weight: 600;
  }
  .kpi.warn {
    color: var(--warn);
    font-weight: 600;
  }
  .budget {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 120px;
    max-width: 260px;
  }
  .budget small {
    white-space: nowrap;
    font-size: 11px;
  }
  .mono {
    font-family: var(--font-code);
  }
</style>
