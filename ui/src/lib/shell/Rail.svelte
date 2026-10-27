<script lang="ts">
  // The four jobs down the left edge (Set up, Run, Tune, Device), then the screens you made under
  // the line. Right-click to rename, recolour, reset. On a phone-wide window the rail is a bar
  // along the bottom.
  import Icon from "#lib/components/common/Icon.svelte";
  import { commands } from "#lib/core/commands.svelte.js";
  import { colorVar, ID_COLORS } from "#lib/core/identity.svelte.js";
  import { menu } from "#lib/core/menu.svelte.js";
  import { shell } from "#lib/core/shell.svelte.js";
  import { workspaces, type Workspace } from "#lib/core/workspace.svelte.js";
  import { guide } from "#lib/core/guide.svelte.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { gallery } from "./gallery.svelte";
  import { help } from "./help.svelte";
  import { WS_ICONS, renameWorkspace } from "./workspace-actions";

  const jobs = $derived(workspaces.all.filter((w) => w.builtin));
  const custom = $derived(workspaces.custom);
  const alerts = $derived(cluster.attention.length);
  /** Setup steps still to do: shown on the Set up job until the checklist is done. */
  const todo = $derived(guide.checklist.filter((s) => !s.done).length);

  function helpMenu(el: Element) {
    const r = el.getBoundingClientRect();
    menu.at(r.right + 6, r.top - 120, [
      { heading: "Help" },
      { label: "What's on this screen?", icon: "info-circle", shortcut: "F1", run: () => (help.explain = true) },
      { label: "Take the tour", icon: "rocket", run: () => help.startTour() },
      { label: "First-time setup again", icon: "rotate-clockwise", run: () => (help.onboarding = true) },
      { separator: true },
      { label: "Search everything", icon: "search", shortcut: "Ctrl K", run: () => (commands.open = true) },
    ]);
  }

  function wsMenu(w: Workspace) {
    return menu.context(() => [
      { heading: w.name },
      { label: "Rename…", icon: "pencil", run: () => renameWorkspace(w.id) },
      { label: "Colour", items: ID_COLORS.map((c) => ({ label: `Colour ${c}`, color: colorVar(c), run: () => workspaces.rename(w.id, { color: c }) })) },
      { label: "Icon", items: WS_ICONS.map((i) => ({ label: i, icon: i, run: () => workspaces.rename(w.id, { icon: i }) })) },
      { separator: true },
      { label: "Duplicate", icon: "copy", run: () => shell.go(workspaces.saveAs(w.id, `${w.name} copy`, w.icon, w.color)) },
      ...(w.builtin
        ? [{ label: "Reset layout", icon: "rotate-clockwise" as const, disabled: !workspaces.isModified(w.id), run: () => workspaces.reset(w.id) }]
        : [{ label: "Delete screen", icon: "trash" as const, danger: true, run: () => { workspaces.remove(w.id); shell.go("run"); } }]),
    ]);
  }
</script>

{#snippet item(w: Workspace, i: number)}
  {@const active = shell.active === w.id}
  {@const badge = w.id === "run" ? alerts : w.id === "setup" ? todo : 0}
  <button
    type="button"
    class="item"
    class:active
    style:--c={colorVar(w.color)}
    onclick={() => shell.go(w.id)}
    oncontextmenu={wsMenu(w)}
    aria-label={w.name}
    aria-current={active ? "page" : undefined}
    data-tip={`${w.name}${i < 9 ? `  ·  ${i + 1}` : ""}${w.summary ? `\n${w.summary}` : ""}`}
  >
    <Icon name={w.icon} size={18} stroke={active ? 2 : 1.7} />
    <span class="lbl">{w.name}</span>
    {#if badge}<span class="badge" class:warn={w.id === "run"} aria-label="{badge} {w.id === 'run' ? 'problems' : 'setup steps left'}">{badge}</span>{/if}
    {#if workspaces.isModified(w.id)}<span class="mod" aria-label="layout changed"></span>{/if}
  </button>
{/snippet}

<nav class="rail" aria-label="Screens" data-tour="rail">
  <button type="button" class="logo" aria-label="HeliOS" onclick={() => shell.go("run")}>
    <svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
      <circle cx="12" cy="12" r="4.2" fill="var(--accent)" />
      {#each Array.from({ length: 8 }) as _, k (k)}
        <rect x="11.2" y="1.6" width="1.6" height="4" rx="0.8" fill="var(--accent)" opacity="0.75" transform="rotate({k * 45} 12 12)" />
      {/each}
    </svg>
  </button>

  <div class="list">
    {#each jobs as w, i (w.id)}
      {@render item(w, i)}
    {/each}
    {#if custom.length}<div class="rule"></div>{/if}
    {#each custom as w, i (w.id)}
      {@render item(w, jobs.length + i)}
    {/each}
    <button type="button" class="item add" aria-label="Add a screen" data-tip="Add a screen of your own, or a kit of panes to this one" onclick={() => gallery.show("screens")} data-tour="add-screen">
      <Icon name="plus" size={16} />
    </button>
  </div>

  <div class="foot">
    <button type="button" class="item" aria-label="Help" data-tip="Tour, what's on this screen, first-time setup" onclick={(e) => helpMenu(e.currentTarget)} data-tour="help">
      <Icon name="help" size={18} />
    </button>
    <button type="button" class="item" aria-label="Search and run" data-tip="Search and run · Ctrl K" onclick={() => (commands.open = true)}>
      <Icon name="search" size={17} />
    </button>
  </div>
</nav>

<style>
  .rail {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 64px;
    flex-shrink: 0;
    padding: 6px 0;
    gap: 2px;
  }
  .logo {
    position: relative;
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    margin-bottom: 4px;
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    overflow-y: auto;
    scrollbar-width: none;
  }
  .item {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 2px;
    width: 58px;
    height: 44px;
    border-radius: var(--r-2);
    color: var(--fg-3);
  }
  .lbl {
    max-width: 56px;
    font-size: 9.5px;
    font-weight: 600;
    letter-spacing: 0.01em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item:hover {
    color: var(--fg);
    background: var(--s2);
  }
  .item.active {
    color: var(--fg);
    background: var(--s2);
  }
  .item.active :global(svg) {
    color: var(--c);
  }
  .item.active::before {
    content: "";
    position: absolute;
    left: -4px;
    top: 8px;
    bottom: 8px;
    width: 3px;
    border-radius: 0 2px 2px 0;
    background: var(--c);
  }
  .badge {
    position: absolute;
    top: 3px;
    right: 5px;
    min-width: 15px;
    height: 15px;
    padding: 0 4px;
    font-size: 9.5px;
    font-weight: 700;
    line-height: 15px;
    color: var(--on-accent);
    background: var(--accent);
    border-radius: var(--r-1);
  }
  .badge.warn {
    color: #fff;
    background: var(--err);
  }
  .mod {
    position: absolute;
    right: 5px;
    bottom: 4px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--fg-3);
  }
  .add {
    height: 30px;
    color: var(--fg-4);
    border: 1px dashed var(--line-strong);
    margin: 4px auto 0;
  }
  .rule {
    height: 1px;
    margin: 4px 8px;
    background: var(--line-strong);
  }
  .foot .item {
    height: 34px;
  }

  /* Phone-wide: a bar along the bottom with the jobs; the rest is in the top bar. */
  @media (max-width: 759px) {
    .rail {
      flex-direction: row;
      width: 100%;
      height: 54px;
      padding: 0 4px;
      border-top: 1px solid var(--line);
      background: var(--s1);
    }
    .logo,
    .foot,
    .rule,
    .add {
      display: none;
    }
    .list {
      flex-direction: row;
      justify-content: space-around;
      overflow-x: auto;
      overflow-y: hidden;
      width: 100%;
    }
    .item {
      flex: 1 0 64px;
      height: 46px;
    }
    .item.active::before {
      left: 10px;
      right: 10px;
      top: auto;
      bottom: -4px;
      width: auto;
      height: 3px;
      border-radius: 2px 2px 0 0;
    }
  }
</style>
