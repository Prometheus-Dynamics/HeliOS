<script lang="ts">
  // The pipeline's Daedalus plan (`GET /v1/pipelines/{id}/plan`, explain_plan): the nodes it runs and
  // the edges between them. A device transfer (a DeviceUpload or DeviceDownload, or an edge that
  // crosses residency) is listed first: each one is a round trip the graph pays for (AGENTS.md rule 4).
  // Fetched when opened, so a closed section costs nothing.
  import { api, errorText } from "#lib/api/client.js";
  import type { PipelinePlan, PlanEdge } from "#lib/api/types.js";

  let { pipelineId }: { pipelineId: string } = $props();

  let open = $state(false);
  let plan = $state<PipelinePlan | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);

  async function load(id: string) {
    loading = true;
    error = null;
    try {
      const next = await api.pipelinePlan(id);
      if (id === pipelineId) plan = next;
    } catch (err) {
      if (id === pipelineId) {
        plan = null;
        error = errorText(err);
      }
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    if (open) void load(pipelineId);
  });

  const edges = $derived(plan?.plan.edges ?? []);
  const transfers = $derived(edges.filter((e) => e.adapter_steps.length > 0 || e.crosses_residency));
  const label = (id: string) => plan?.plan.nodes.find((n) => n.id === id)?.label ?? id;
  const step = (edge: PlanEdge) => edge.adapter_steps.map((s) => (typeof s === "string" ? s : JSON.stringify(s))).join(", ");
</script>

<details class="plan" bind:open>
  <summary>
    <span>Plan</span>
    {#if plan}<small>{plan.plan.nodes.length} nodes · {edges.length} edges · {transfers.length} device {transfers.length === 1 ? "transfer" : "transfers"}</small>{/if}
  </summary>
  <div class="body">
    {#if loading && !plan}
      <p class="dim">Reading the plan…</p>
    {:else if error}
      <p class="dim">{error}</p>
    {:else if plan}
      {#if transfers.length}
        <ul class="transfers" aria-label="Device transfers">
          {#each transfers as e (e.index)}
            <li data-transfer>
              <b>{label(e.from_node_id)}</b> → <b>{label(e.to_node_id)}</b>
              <span class="dim">{e.from_port} → {e.to_port} · {e.transport}</span>
              {#if step(e)}<code>{step(e)}</code>{/if}
              {#if e.crosses_residency}<span class="warn">crosses residency</span>{/if}
            </li>
          {/each}
        </ul>
      {:else}
        <p class="ok">No device transfers: every edge stays on one side.</p>
      {/if}
      <ol class="nodes">
        {#each plan.plan.nodes as n (n.index)}
          <li><b>{n.label}</b> <span class="dim">{n.id} · {n.compute}</span></li>
        {/each}
      </ol>
    {/if}
  </div>
</details>

<style>
  .plan {
    border-top: 1px solid var(--line);
    background: var(--s1);
    font-size: 12px;
  }
  summary {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 6px 10px;
    cursor: pointer;
    font-weight: 600;
    color: var(--fg);
  }
  summary small {
    color: var(--fg-3);
    font-weight: 400;
  }
  .body {
    display: grid;
    gap: 8px;
    padding: 4px 10px 10px;
    max-height: 220px;
    overflow: auto;
  }
  ul,
  ol {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    gap: 4px;
  }
  li {
    display: grid;
    gap: 2px;
  }
  code {
    font-family: var(--mono, monospace);
    color: var(--fg-2);
    white-space: pre-wrap;
  }
  .dim {
    color: var(--fg-3);
  }
  .ok {
    color: var(--ok);
    margin: 0;
  }
  .warn {
    color: var(--warn);
  }
</style>
