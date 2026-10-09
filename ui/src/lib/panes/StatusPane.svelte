<script lang="ts">
  // The robot at a glance: what needs attention, then one line per device
  // with its load, so problems are visible before anyone goes looking.
  import Icon from "#lib/components/common/Icon.svelte";
  import { colorVar, identity } from "#lib/core/identity.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { shell } from "#lib/core/shell.svelte.js";
  import { nodeIcon } from "#lib/present.js";
  import Spark from "#lib/kit/Spark.svelte";
  import Swatch from "#lib/kit/Swatch.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { problems } from "#lib/core/fixes.js";

  const list = $derived(problems());
  const running = $derived(cluster.workloads.filter((w) => w.state === "running").length);
  const memMiB = $derived(cluster.nodes.filter((n) => n.kind === "raze").reduce((a, n) => a + n.memMiB, 0));
</script>

<div class="status">
  {#if list.length}
    <div class="att">
      {#each list as p (p.id)}
        <div class="item t-{p.severity}">
          <Icon name={p.severity === "error" ? "alert-circle" : "alert-triangle"} size={14} />
          <div class="pt"><b>{p.title}</b><i>{p.why}</i></div>
          {#if p.fixes.length}
            <div class="fx">
              {#each p.fixes.slice(0, 2) as f (f.label)}
                <button type="button" class:primary={f.primary} onclick={f.run} data-tip={f.label}><Icon name={f.icon} size={12} />{f.label}</button>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {:else}
    <div class="allgood"><Icon name="circle-check" size={14} /> No problems.</div>
  {/if}

  <div class="kpis">
    <div><b>{running}</b><span>pipelines running</span></div>
    <div><b>{cluster.cameras.length}</b><span>cameras</span></div>
    <div><b>{memMiB.toFixed(0)}</b><span>MiB on Razes</span></div>
  </div>

  <div class="devs">
    {#each cluster.nodes as n (n.id)}
      <button type="button" class="dev" class:off={n.health === "offline"} style:--c={colorVar(identity.get(n.id).color)} onclick={() => selection.select({ kind: "device", id: n.id })} ondblclick={() => shell.go("hardware")}>
        <Swatch id={n.id} icon={nodeIcon(n.kind)} size={18} />
        <span class="nm">{identity.name(n.id, n.name)}</span>
        <span class="h h-{n.health}"></span>
        <Spark values={n.cpuHistory} width={44} height={14} color="var(--c)" />
        <span class="mono">{(n.cpu * 100).toFixed(0)}%</span>
        <span class="mono" class:hot={n.tempC > 60}>{n.tempC.toFixed(0)}°</span>
      </button>
    {/each}
  </div>
</div>

<style>
  .status {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }
  .att {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 6px;
  }
  .item {
    display: flex;
    align-items: flex-start;
    gap: 7px;
    padding: 5px 8px;
    font-size: 12px;
    text-align: left;
    border-radius: var(--r-1);
  }
  .pt {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .pt b {
    font-weight: 600;
    color: var(--fg);
  }
  .pt i {
    font-style: normal;
    font-size: 11px;
    color: var(--fg-2);
  }
  .item {
    flex-wrap: wrap;
  }
  .pt {
    flex: 1 1 calc(100% - 24px) !important;
  }
  /* Fix buttons sit under the text, so neither squeezes the other. */
  .fx {
    display: flex;
    flex: 1 0 100%;
    flex-wrap: wrap;
    padding-left: 21px;
    gap: 4px;
    flex-shrink: 0;
  }
  .fx button {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 20px;
    padding: 0 6px;
    font-size: 11px;
    font-weight: 600;
    color: var(--fg);
    background: var(--s1);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
    white-space: nowrap;
  }
  .fx button.primary {
    color: var(--on-accent);
    background: var(--accent);
    border-color: var(--accent);
  }
  .t-error {
    color: var(--err);
    background: var(--err-bg);
  }
  .t-warning {
    color: var(--warn);
    background: var(--warn-bg);
  }

  .allgood {
    display: flex;
    gap: 6px;
    align-items: center;
    margin: 6px;
    padding: 6px 8px;
    font-size: 12px;
    color: var(--ok);
    background: var(--ok-bg);
    border-radius: var(--r-1);
  }
  .kpis {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    border-top: 1px solid var(--line);
    border-bottom: 1px solid var(--line);
  }
  .kpis div {
    display: flex;
    flex-direction: column;
    padding: 6px 10px;
  }
  .kpis div + div {
    border-left: 1px solid var(--line);
  }
  .kpis b {
    font-family: var(--font-code);
    font-size: 17px;
  }
  .kpis span {
    font-size: 10.5px;
    color: var(--fg-3);
  }
  .devs {
    padding: 3px 0;
  }
  .dev {
    display: grid;
    grid-template-columns: 18px 1fr 7px 44px 30px 26px;
    align-items: center;
    gap: 7px;
    width: 100%;
    height: 28px;
    padding: 0 10px;
    font-size: 12px;
    text-align: left;
  }
  .dev:hover {
    background: var(--s2);
  }
  .dev.off {
    opacity: 0.45;
  }
  .nm {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .h {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }
  .h-online {
    background: var(--ok);
  }
  .h-degraded {
    background: var(--warn);
  }
  .h-offline {
    background: var(--err);
  }
  .mono {
    font-family: var(--font-code);
    font-size: 11px;
    text-align: right;
    color: var(--fg-2);
  }
  .hot {
    color: var(--warn);
  }
</style>
