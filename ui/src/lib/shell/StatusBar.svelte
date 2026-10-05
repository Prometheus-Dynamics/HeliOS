<script lang="ts">
  // One line at the bottom: what is selected (with its key numbers), what
  // needs attention, the next setup step, and the live clock of the feeds.
  import Icon from "$lib/components/common/Icon.svelte";
  import { actionable, problems } from "$lib/core/fixes";
  import { guide } from "$lib/core/guide.svelte";
  import { identity } from "$lib/core/identity.svelte";
  import { selection } from "$lib/core/selection.svelte";
  import { shell } from "$lib/core/shell.svelte";
  import { pane, workspaces } from "$lib/core/workspace.svelte";
  import { cluster } from "$lib/stores/cluster.svelte";

  const s = $derived(selection.current);
  const readout = $derived.by(() => {
    if (!s) return null;
    if (s.kind === "camera") {
      const c = cluster.camera(s.id);
      return c ? `${identity.name(c.resourceId, c.name)} · ${c.settings.width}×${c.settings.height} @ ${c.stats.fps.toFixed(0)} fps · exp ${c.settings.exposureUs} µs · gain ${c.settings.gain.toFixed(1)}× · ${c.stats.latencyMs.toFixed(1)} ms` : null;
    }
    if (s.kind === "workload") {
      const w = cluster.workload(s.id);
      return w ? `${w.name} · r${w.revision} · ${w.state} · p50 ${w.perf.tickP50Ms.toFixed(2)} ms · p99 ${w.perf.tickP99Ms.toFixed(2)} ms` : null;
    }
    if (s.kind === "device") {
      const n = cluster.node(s.id);
      return n ? `${n.name} · ${n.address} · cpu ${(n.cpu * 100).toFixed(0)}% · ${n.memMiB.toFixed(1)} MiB · ${n.tempC.toFixed(0)} °C · ${n.os.name} ${n.os.version}` : null;
    }
    return `${s.kind.replace("-", " ")} ${s.id}${s.parent ? ` · ${s.parent}` : ""}`;
  });
  const issues = $derived(problems().filter(actionable));
  const steps = $derived(guide.checklist);
  const next = $derived(steps.find((x) => !x.done));
</script>

<footer class="sb" data-tour="status">
  <span class="sel">{#if readout}<Icon name="crosshair" size={11} />{readout}{:else}Nothing selected{/if}</span>
  <span class="grow"></span>
  {#if next}
    <button type="button" onclick={() => workspaces.open(shell.active, pane("setup"))} data-tip="Open the setup checklist">
      <Icon name="list-check" size={11} />Setup {steps.length - steps.filter((x) => !x.done).length}/{steps.length} · next: {next.title.toLowerCase()}
    </button>
  {/if}
  <button type="button" class:bad={issues.length > 0} onclick={() => shell.go("overview")}>
    <Icon name={issues.length ? "alert-triangle" : "circle-check"} size={11} />{issues.length ? `${issues.length} problem${issues.length > 1 ? "s" : ""}` : "OK"}
  </button>
  <span class="mono">{cluster.playing ? "live" : "paused"} · frame {cluster.frame}</span>
</footer>

<style>
  .sb {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 22px;
    padding: 0 8px;
    flex-shrink: 0;
    font-size: 11px;
    color: var(--fg-3);
    border-top: 1px solid var(--line);
    background: var(--s1);
    white-space: nowrap;
  }
  .sel {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: var(--font-code);
    font-size: 10.5px;
    color: var(--fg-2);
  }
  .grow {
    flex: 1;
  }
  button {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 100%;
    padding: 0 8px;
    font-size: 11px;
    color: var(--fg-3);
  }
  button:hover {
    color: var(--fg);
    background: var(--s2);
  }
  button.bad {
    color: var(--warn);
  }
  .mono {
    padding-left: 8px;
    font-family: var(--font-code);
    font-size: 10.5px;
  }
</style>
