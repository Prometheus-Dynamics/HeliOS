<script lang="ts">
  import { untrack } from "svelte";
  import "../app.css";
  import "@xyflow/svelte/dist/style.css";
  import Toasts from "$lib/components/shell/Toasts.svelte";
  import { commands } from "$lib/core/commands.svelte";
  import { prefs } from "$lib/core/prefs.svelte";
  import { selection } from "$lib/core/selection.svelte";
  import { shell } from "$lib/core/shell.svelte";
  import { workspaces } from "$lib/core/workspace.svelte";
  import "$lib/panes";
  import { registerScreens } from "$lib/screens";
  import { registerCommands } from "$lib/shell/commands";
  import { registerIntents } from "$lib/shell/intents";
  import MenuHost from "$lib/shell/MenuHost.svelte";
  import Gallery from "$lib/shell/Gallery.svelte";
  import Onboarding from "$lib/shell/Onboarding.svelte";
  import Tour from "$lib/shell/Tour.svelte";
  import { help } from "$lib/shell/help.svelte";
  import Palette from "$lib/shell/Palette.svelte";
  import Rail from "$lib/shell/Rail.svelte";
  import StatusBar from "$lib/shell/StatusBar.svelte";
  import TipHost from "$lib/shell/TipHost.svelte";
  import TopBar from "$lib/shell/TopBar.svelte";
  import { cluster } from "$lib/stores/cluster.svelte";
  import { system } from "$lib/stores/system.svelte";
  import Workspace from "$lib/workspace/Workspace.svelte";

  let { children } = $props();

  registerScreens();
  registerCommands();
  registerIntents();

  $effect(() => {
    prefs.apply();
    shell.fromHash();
  });

  $effect(() => {
    // Start with something to show in panes that follow the selection.
    if (!selection.current && cluster.cameras[0]) selection.select({ kind: "camera", id: cluster.cameras[0].resourceId });
    if (!selection.last.workload && cluster.workloads[0]) selection.last.workload = { kind: "workload", id: cluster.workloads[0].id };
    if (!selection.last.device && cluster.nodes[0]) selection.last.device = { kind: "device", id: cluster.nodes[0].id };
  });

  // Separate from the effects above, and untracked: those re-run as live data
  // arrives, and restarting the stores on every change would refetch in a loop.
  $effect(() => {
    const stopCluster = untrack(() => cluster.start());
    const stopSystem = untrack(() => system.start());
    return () => {
      stopCluster();
      stopSystem();
    };
  });

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "F1") {
      event.preventDefault();
      help.explain = !help.explain;
      return;
    }
    if (event.key === "Escape" && help.explain) {
      help.explain = false;
      return;
    }
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      commands.open = !commands.open;
      return;
    }
    const target = event.target as HTMLElement;
    if (target.closest("input, textarea, select, [contenteditable]") || event.metaKey || event.ctrlKey || event.altKey) return;
    if (event.key === " " && !target.closest("button, [role=button], [role=tab]")) {
      event.preventDefault();
      cluster.playing = !cluster.playing;
      return;
    }
    const index = Number(event.key) - 1;
    const all = workspaces.shown;
    if (index >= 0 && index < Math.min(9, all.length)) shell.go(all[index].id);
  }
</script>

<svelte:window {onkeydown} onhashchange={() => shell.fromHash()} />

<div class="app">
  <Rail />
  <div class="main">
    <TopBar />
    {#if help.explain}
      {@const cur = workspaces.get(shell.active)}
      <button type="button" class="explain-bar" onclick={() => (help.explain = false)}>
        <b>{cur?.name}</b>{cur?.summary ? ` · ${cur.summary}` : ""}
        <span>Each pane is labelled below. Click anywhere or press Esc to close.</span>
      </button>
    {/if}
    {#key shell.active}
      <Workspace id={shell.active} />
    {/key}
  </div>
</div>
<StatusBar />
{@render children()}
<MenuHost />
<Palette />
<Gallery />
<Onboarding />
<Tour />
<TipHost />
<Toasts />

<style>
  :global(body) {
    display: flex;
    flex-direction: column;
    height: 100dvh;
  }
  .app {
    flex: 1;
    min-height: 0;
    display: flex;
    background: var(--s0);
    color: var(--fg);
    overflow: hidden;
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .explain-bar {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin: 0 4px 6px 0;
    padding: 6px 10px;
    font-size: 12.5px;
    text-align: left;
    color: var(--fg-2);
    background: var(--accent-tint);
    border: 1px solid var(--accent-ring);
    border-radius: var(--r-2);
  }
  .explain-bar b {
    color: var(--fg);
  }
  .explain-bar span {
    margin-left: auto;
    font-size: 11.5px;
    color: var(--fg-3);
  }
</style>
