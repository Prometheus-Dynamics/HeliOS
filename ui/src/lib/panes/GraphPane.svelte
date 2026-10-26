<script lang="ts">
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
  import Picker from "#lib/kit/Picker.svelte";
  import type { GraphDocument } from "#lib/api/model.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { toasts } from "#lib/stores/toasts.svelte.js";
  import { downloadJson, pickJson } from "#lib/files.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "#lib/workspace/panes.js";
  import Empty from "./Empty.svelte";
  import { follow } from "./follow.svelte";

  let { pane, ws }: PaneProps = $props();
  const target = follow(() => pane, () => ws, "workload", () => cluster.workloads[0]?.id);
  const workload = $derived(target.id ? cluster.workload(target.id) : undefined);
  const draft = $derived(workload ? drafts.get(workload.id) : undefined);
  const STATE_TONE = { running: "ok", starting: "info", quarantined: "err", stopped: "neutral" } as const;

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
  {/if}
  {#key workload.id}
    <SvelteFlowProvider>
      <GraphCanvas {draft} />
    </SvelteFlowProvider>
  {/key}
  <PlanSection pipelineId={workload.id} />
{:else}
  <Empty icon="schema" text="No pipeline selected." />
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
</style>
