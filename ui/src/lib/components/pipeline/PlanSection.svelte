<script lang="ts">
  // The pipeline's plan under the graph: the device transfers first (each one a round trip the
  // graph pays for), then every node with where it computes. The plan is read per revision
  // (pipeline/plan.svelte.ts); the graph above marks the same transfers on its edges.
  import Icon from "#lib/components/common/Icon.svelte";
  import { plans, transfersOf } from "#lib/pipeline/plan.svelte.js";

  let { pipelineId, revision, open = false }: { pipelineId: string; revision: number; open?: boolean } = $props();

  $effect(() => {
    void plans.load(pipelineId, revision);
  });
  const state = $derived(plans.get(pipelineId));
  const plan = $derived(state.plan);
  const transfers = $derived(transfersOf(plan));
  const label = (id: string) => plan?.plan.nodes.find((n) => n.id === id)?.label ?? id;
  const KIND = { upload: "to the GPU", download: "to the CPU", cross: "crosses residency" } as const;
</script>

<details class="plan" {open}>
  <summary>
    <span>Plan</span>
    {#if plan}
      <small>{plan.plan.nodes.length} nodes · {plan.plan.edges.length} edges</small>
      <small class:warn={transfers.length > 0}><Icon name="arrows-transfer-down" size={12} />{transfers.length === 0 ? "no device transfers" : `${transfers.length} device ${transfers.length === 1 ? "transfer" : "transfers"}`}</small>
    {:else if state.loading}
      <small>reading…</small>
    {:else if state.error}
      <small>{state.error}</small>
    {/if}
  </summary>
  <div class="body">
    {#if plan}
      {#if transfers.length}
        <ul class="transfers" aria-label="Device transfers">
          {#each transfers as t (t.edge.index)}
            <li data-transfer>
              <b>{label(t.edge.from_node_id)}</b> → <b>{label(t.edge.to_node_id)}</b>
              <span class="dim">{t.edge.from_port} → {t.edge.to_port} · {t.edge.transport} · {KIND[t.kind]}</span>
              {#if t.steps}<code>{t.steps}</code>{/if}
            </li>
          {/each}
        </ul>
        <p class="dim">Each transfer copies the frame between the CPU and the GPU. Group the GPU stages together, or add an adapter, to remove a round trip.</p>
      {:else}
        <p class="ok">No device transfers: every edge stays on one side.</p>
      {/if}
      <ol class="nodes">
        {#each plan.plan.nodes as n (n.index)}
          <li><b>{n.label}</b> <span class="dim">{n.id} · {n.compute}</span></li>
        {/each}
      </ol>
    {:else if state.error}
      <p class="dim">{state.error}</p>
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
    align-items: center;
    gap: 10px;
    padding: 5px 10px;
    cursor: pointer;
    font-weight: 600;
    color: var(--fg);
  }
  summary small {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--fg-3);
    font-weight: 400;
  }
  summary small.warn {
    color: var(--warn);
    font-weight: 600;
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
  .transfers li {
    padding: 4px 8px;
    border-left: 2px solid var(--warn);
    background: var(--warn-bg);
    border-radius: 0 var(--r-1) var(--r-1) 0;
  }
  code {
    font-family: var(--font-code);
    font-size: 11px;
    color: var(--fg-2);
    white-space: pre-wrap;
  }
  p {
    margin: 0;
  }
  .dim {
    color: var(--fg-3);
  }
  .ok {
    color: var(--ok);
  }
</style>
