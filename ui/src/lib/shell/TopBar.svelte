<script lang="ts">
  // Workspace name and layout tools on the left, robot readouts in the middle
  // (each jumps to the screen that explains it), search and look on the right.
  import Icon from "#lib/components/common/Icon.svelte";
  import { commands } from "#lib/core/commands.svelte.js";
  import { colorVar } from "#lib/core/identity.svelte.js";
  import { menu } from "#lib/core/menu.svelte.js";
  import { prefs, THEMES } from "#lib/core/prefs.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { shell } from "#lib/core/shell.svelte.js";
  import { workspaces } from "#lib/core/workspace.svelte.js";
  import IconButton from "#lib/kit/IconButton.svelte";
  import { auth } from "#lib/stores/auth.svelte.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { showAddPane } from "#lib/workspace/library.js";
  import { exportWorkspace, importWorkspace } from "./workspace-actions";
  import { gallery } from "./gallery.svelte";
  import { help } from "./help.svelte";

  const ws = $derived(workspaces.get(shell.active));
  const online = $derived(cluster.nodes.filter((n) => n.health === "online").length);
  const degraded = $derived(cluster.nodes.filter((n) => n.health !== "online").length);
  const running = $derived(cluster.workloads.filter((w) => w.state === "running"));
  const quarantined = $derived(cluster.workloads.filter((w) => w.state === "quarantined").length);
  const p50 = $derived(running.length ? Math.max(...running.map((w) => w.perf.tickP50Ms)) : 0);
  const hottest = $derived(cluster.nodes.filter((n) => n.health !== "offline").reduce((a, n) => (n.tempC > a.tempC ? n : a), cluster.nodes[0]));
  const worstClock = $derived(cluster.nodes.filter((n) => n.health !== "offline").reduce((a, n) => Math.max(a, n.clock.offsetUs), 0));
  const tagsSeen = $derived(cluster.cameras.reduce((a, c) => a + cluster.detectionsFor(c).length, 0));

  let nameInput = $state<HTMLInputElement>();
  $effect(() => {
    if (shell.renaming) nameInput?.select();
  });
  function commitName() {
    const v = nameInput?.value.trim();
    if (ws && v) workspaces.rename(ws.id, { name: v });
    shell.renaming = false;
  }

  function lookMenu(event: MouseEvent) {
    menu.below(event.currentTarget as Element, [
      { heading: "Theme" },
      ...THEMES.map((t) => ({ label: t.name, hint: t.note, checked: prefs.theme === t.id, run: () => (prefs.theme = t.id) })),
      { separator: true },
      { heading: "Density" },
      { label: "Compact", checked: prefs.density === "compact", run: () => (prefs.density = "compact") },
      { label: "Comfortable", checked: prefs.density === "comfortable", run: () => (prefs.density = "comfortable") },
    ], "end");
  }

  function layoutMenu(event: MouseEvent) {
    if (!ws) return;
    const el = event.currentTarget as Element;
    menu.below(el, [
      { label: "Add pane", icon: "plus", run: () => showAddPane(el, ws.id, workspaces.focusedStack) },
      { label: "Rename", icon: "pencil", run: () => (shell.renaming = true) },
      { label: "Duplicate as new workspace", icon: "copy", run: () => shell.go(workspaces.saveAs(ws.id, `${ws.name} copy`, ws.icon, ws.color)) },
      { label: "Reset layout", icon: "rotate-clockwise", disabled: !workspaces.isModified(ws.id), run: () => workspaces.reset(ws.id) },
      { separator: true },
      { label: "Export layout…", icon: "download", run: () => exportWorkspace(ws.id) },
      { label: "Import layout…", icon: "upload", run: importWorkspace },
    ]);
  }
</script>

<header class="top">
  {#if ws}
    <div class="ws" style:--c={colorVar(ws.color)}>
      <span class="ws-icon"><Icon name={ws.icon} size={14} stroke={2} /></span>
      {#if shell.renaming}
        <input
          class="ws-name-input"
          bind:this={nameInput}
          value={ws.name}
          aria-label="Workspace name"
          onblur={commitName}
          onkeydown={(e) => {
            if (e.key === "Enter") commitName();
            if (e.key === "Escape") shell.renaming = false;
          }}
        />
      {:else}
        <button type="button" class="ws-name" ondblclick={() => (shell.renaming = true)} onclick={layoutMenu} data-tip="Layout · double-click to rename">
          {ws.name}
          <Icon name="chevron-down" size={12} stroke={2.2} />
        </button>
      {/if}
      {#if workspaces.isModified(ws.id)}
        <button type="button" class="mod" onclick={() => workspaces.reset(ws.id)} data-tip="Layout changed from the default. Click to reset.">modified</button>
      {/if}
      <button type="button" class="add" onclick={() => gallery.show("kits")} data-tip="Add a kit of panes or a single pane to this screen"><Icon name="plus" size={13} />Add</button>
      <button type="button" class="add ghost" onclick={() => (help.explain = !help.explain)} data-tip="Label every pane on this screen · F1"><Icon name="info-circle" size={13} />What's here?</button>
    </div>
  {/if}

  <div class="chips" data-tour="readouts">
    <button type="button" class="chip" onclick={() => shell.go("hardware")} data-tip="Devices online">
      <span class="dot" class:warn={degraded > 0}></span>
      <b>{online}</b><span class="of">/{cluster.nodes.length}</span> nodes
    </button>
    <button type="button" class="chip" onclick={() => shell.go("pipelines")} data-tip="Pipelines running / quarantined">
      <Icon name="schema" size={13} />
      <b>{running.length}</b> run
      {#if quarantined}<span class="bad"><Icon name="alert-triangle" size={12} />{quarantined}</span>{/if}
    </button>
    <button type="button" class="chip" onclick={() => shell.go("pipelines")} data-tip="Slowest pipeline tick, p50">
      <Icon name="stopwatch" size={13} /><b class="mono">{p50.toFixed(2)}</b>ms
    </button>
    <button type="button" class="chip" onclick={() => shell.go("vision")} data-tip="Tags detected this frame, all cameras">
      <Icon name="target" size={13} /><b class="mono">{tagsSeen}</b>tags
    </button>
    {#if hottest}
      <button type="button" class="chip" class:hot={hottest.tempC > 60} onclick={() => { selection.select({ kind: "device", id: hottest.id }); shell.go("hardware"); }} data-tip="Hottest: {hottest.name}">
        <Icon name="temperature" size={13} /><b class="mono">{hottest.tempC.toFixed(0)}</b>°C
      </button>
    {/if}
    <span class="chip static" class:hot={worstClock > 1000} data-tip="Worst clock offset (PTP)">
      <Icon name="clock" size={13} /><b class="mono">{worstClock >= 1000 ? (worstClock / 1000).toFixed(1) : worstClock}</b>{worstClock >= 1000 ? "ms" : "µs"}
    </span>
  </div>

  <div class="right">
    {#if auth.status}
      <button
        type="button"
        class="sec"
        class:open={auth.open}
        onclick={() => shell.go("settings")}
        data-tour="security"
        data-tip={auth.open ? "Open: anyone on this network can change this device, update it or reboot it. Click to secure it with a password." : `Secured: a password or API token is needed.${auth.status.via === "session" ? " You are signed in." : ""} Click for security settings.`}
      >
        <Icon name={auth.open ? "lock-open" : "lock"} size={13} />{auth.open ? "Open" : "Secured"}
      </button>
    {/if}
    <IconButton icon={cluster.playing ? "player-pause" : "player-play"} label={cluster.playing ? "Pause live feeds" : "Resume live feeds"} shortcut="Space" onclick={() => (cluster.playing = !cluster.playing)} />
    <button type="button" class="search" onclick={() => (commands.open = true)} data-tour="search">
      <Icon name="search" size={13} />
      <span>Search, run, or describe a problem</span>
      <kbd>Ctrl K</kbd>
    </button>
    <IconButton icon="contrast-2" label="Theme and density" onclick={lookMenu} />
  </div>
</header>

<style>
  .top {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 38px;
    padding: 0 6px 0 2px;
    flex-shrink: 0;
  }
  .ws {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .ws-icon {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: var(--r-1);
    color: var(--c);
    background: color-mix(in oklab, var(--c) 16%, transparent);
  }
  .ws-name {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 26px;
    padding: 0 6px;
    font-size: 13px;
    font-weight: 650;
    color: var(--fg);
    border-radius: var(--r-1);
    white-space: nowrap;
  }
  .ws-name:hover {
    background: var(--s2);
  }
  .ws-name-input {
    width: 180px;
    height: 26px;
    padding: 0 6px;
    font: inherit;
    font-size: 13px;
    font-weight: 650;
    color: var(--fg);
    background: var(--inset);
    border: 1px solid var(--accent-ring);
    border-radius: var(--r-1);
    outline: none;
  }
  .mod {
    height: 20px;
    padding: 0 6px;
    font-size: 10.5px;
    color: var(--fg-3);
    border: 1px solid var(--line);
    border-radius: var(--r-1);
    white-space: nowrap;
  }
  .mod:hover {
    color: var(--fg);
    border-color: var(--line-strong);
  }
  .add {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding: 0 8px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--fg-2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
    white-space: nowrap;
  }
  .add:hover {
    color: var(--fg);
    background: var(--s2);
  }
  .add.ghost {
    border-color: transparent;
    color: var(--fg-3);
  }
  .chips {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 2px;
    min-width: 0;
    overflow: hidden;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding: 0 8px;
    font-size: 11.5px;
    color: var(--fg-3);
    border-radius: var(--r-1);
    white-space: nowrap;
  }
  .chip:not(.static):hover {
    color: var(--fg-2);
    background: var(--s2);
  }
  .chip + .chip {
    border-left: 1px solid var(--line);
    border-radius: 0;
  }
  .chip b {
    color: var(--fg);
    font-weight: 600;
  }
  .chip .of {
    margin-left: -3px;
  }
  .chip.hot,
  .chip.hot b {
    color: var(--warn);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--ok);
  }
  .dot.warn {
    background: var(--warn);
  }
  .bad {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    color: var(--err);
    font-weight: 650;
  }
  .right {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .sec {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 8px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--ok);
    border: 1px solid color-mix(in oklab, var(--ok) 40%, transparent);
    border-radius: var(--r-1);
    white-space: nowrap;
  }
  .sec.open {
    color: var(--warn);
    border-color: color-mix(in oklab, var(--warn) 40%, transparent);
  }
  .sec:hover {
    background: var(--s2);
  }
  .search {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 250px;
    height: 26px;
    padding: 0 6px 0 9px;
    font-size: 12px;
    color: var(--fg-3);
    background: var(--inset);
    border: 1px solid var(--line);
    border-radius: var(--r-2);
  }
  .search:hover {
    border-color: var(--line-strong);
    color: var(--fg-2);
  }
  .search span {
    flex: 1;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  kbd {
    font-family: var(--font-code);
    font-size: 10px;
    padding: 1px 5px;
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
  }
  .mono {
    font-family: var(--font-code);
  }
</style>
