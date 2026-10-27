<script lang="ts">
  // One line at the bottom: what is selected (with its key numbers), what
  // needs attention, the next setup step, and the live clock of the feeds.
  import Icon from "#lib/components/common/Icon.svelte";
  import RecordingIndicator from "#lib/components/recording/RecordingIndicator.svelte";
  import { actionable, problems } from "#lib/core/fixes.js";
  import { guide } from "#lib/core/guide.svelte.js";
  import { identity } from "#lib/core/identity.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { shell } from "#lib/core/shell.svelte.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { liveState } from "#lib/stores/live.svelte.js";

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
  /** Robot code's view: whether pipeline results reach NetworkTables. */
  const nt = $derived.by(() => {
    const state = liveState.nt4;
    if (!cluster.live || !state) return null;
    if (!state.settings.enabled) return { tone: "", text: "NT off", tip: "Publishing to NetworkTables is off (Device › Robot link)" };
    if (!state.server) return { tone: "warn", text: "NT: no team", tip: "Set the team number in Device › Robot link so results reach robot code" };
    return state.connected
      ? { tone: "ok", text: `NT ${state.server}`, tip: `Publishing every frame to ${state.server} under ${state.settings.prefix}` }
      : { tone: "warn", text: `NT waiting`, tip: `Not connected to ${state.server} yet: is the device on the robot network?` };
  });
</script>

<footer class="sb" data-tour="status">
  <span class="sel">{#if readout}<Icon name="crosshair" size={11} />{readout}{:else}Nothing selected{/if}</span>
  <span class="grow"></span>
  {#if next}
    <button type="button" onclick={() => shell.open("setup", "setup")} data-tip="Open the setup checklist">
      <Icon name="list-check" size={11} />Setup {steps.length - steps.filter((x) => !x.done).length}/{steps.length} · next: {next.title.toLowerCase()}
    </button>
  {/if}
  <button type="button" class:bad={issues.length > 0} onclick={() => shell.open("run", "status")}>
    <Icon name={issues.length ? "alert-triangle" : "circle-check"} size={11} />{issues.length ? `${issues.length} problem${issues.length > 1 ? "s" : ""}` : "OK"}
  </button>
  {#if cluster.live && !liveState.connected}
    <button type="button" class="bad" onclick={() => shell.open("run", "status")} data-tip={liveState.error ?? "The device's event stream is not connected. The page retries on its own."}>
      <Icon name="alert-triangle" size={11} />Device link lost
    </button>
  {/if}
  {#if nt}
    <button type="button" class="nt {nt.tone}" onclick={() => shell.open("device", "robot")} data-tip={nt.tip}><Icon name="plug" size={11} />{nt.text}</button>
  {/if}
  <RecordingIndicator />
  <span class="mono">{cluster.live ? "live" : `${cluster.playing ? "live" : "paused"} · frame ${cluster.frame}`}</span>
</footer>

<style>
  .nt.ok {
    color: var(--ok, #3ccf7e);
  }
  .nt.warn {
    color: var(--warn, #f5a524);
  }
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
