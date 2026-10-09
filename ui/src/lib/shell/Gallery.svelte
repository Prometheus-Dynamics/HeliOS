<script lang="ts">
  // Add things on purpose: whole screens, prebuilt kits of panes, or single
  // panes. Every entry shows its layout and says what it is for.
  import Icon from "#lib/components/common/Icon.svelte";
  import { colorVar } from "#lib/core/identity.svelte.js";
  import { shell } from "#lib/core/shell.svelte.js";
  import { pane, workspaces } from "#lib/core/workspace.svelte.js";
  import { KITS } from "#lib/screens.js";
  import { allPanes } from "#lib/workspace/panes.js";
  import { gallery } from "./gallery.svelte";
  import LayoutThumb from "./LayoutThumb.svelte";
  import { importWorkspace } from "./workspace-actions";

  const current = $derived(workspaces.get(shell.active));
  const screens = $derived(workspaces.all.filter((w) => w.builtin));
  const panes = $derived(allPanes().filter((p) => p.type !== "welcome"));

  function close() {
    gallery.open = false;
  }
  function addScreen(id: string) {
    workspaces.show(id);
    shell.go(id);
    close();
  }
</script>

<svelte:window onkeydown={(e) => gallery.open && e.key === "Escape" && close()} />

{#if gallery.open}
  <div class="scrim" role="presentation" onpointerdown={close}></div>
  <div class="dlg" role="dialog" aria-label="Add to HeliOS">
    <header>
      <div class="tabs" role="tablist">
        <button type="button" role="tab" aria-selected={gallery.tab === "screens"} class:on={gallery.tab === "screens"} onclick={() => (gallery.tab = "screens")}><Icon name="layout-grid" size={14} />Screens</button>
        <button type="button" role="tab" aria-selected={gallery.tab === "kits"} class:on={gallery.tab === "kits"} onclick={() => (gallery.tab = "kits")}><Icon name="puzzle" size={14} />Kits for this screen</button>
        <button type="button" role="tab" aria-selected={gallery.tab === "panes"} class:on={gallery.tab === "panes"} onclick={() => (gallery.tab = "panes")}><Icon name="apps" size={14} />Single panes</button>
      </div>
      <button type="button" class="x" onclick={close} aria-label="Close"><Icon name="x" size={16} /></button>
    </header>

    <div class="body">
      {#if gallery.tab === "screens"}
        <p class="lead">A screen is a full layout in the left rail. Add the ones you need; remove any with right-click → Hide.</p>
        <div class="grid">
          {#each screens as w (w.id)}
            {@const inRail = workspaces.isVisible(w.id)}
            <div class="card" style:--c={colorVar(w.color)}>
              <div class="thumb"><LayoutThumb node={w.root} /></div>
              <div class="meta">
                <span class="ic"><Icon name={w.icon} size={14} /></span>
                <div class="txt"><b>{w.name}</b><i>{w.summary}</i></div>
              </div>
              <div class="acts">
                {#if inRail}
                  <button type="button" onclick={() => { shell.go(w.id); close(); }}>Open</button>
                  <span class="note">In the rail</span>
                {:else}
                  <button type="button" class="primary" onclick={() => addScreen(w.id)}><Icon name="plus" size={13} />Add screen</button>
                {/if}
              </div>
            </div>
          {/each}
          <div class="card blank">
            <div class="thumb empty"><Icon name="plus" size={20} /></div>
            <div class="meta"><div class="txt"><b>Blank screen</b><i>Start empty and add kits or panes.</i></div></div>
            <div class="acts">
              <button type="button" onclick={() => { shell.go(workspaces.create("My screen", "layout-grid", 5)); gallery.tab = "kits"; }}>Create</button>
              <button type="button" onclick={() => { close(); importWorkspace(); }}>Import…</button>
            </div>
          </div>
        </div>
      {:else if gallery.tab === "kits"}
        <p class="lead">A kit is a ready-made group of panes. It goes beside what is on <b>{current?.name}</b> now; drag its tabs anywhere afterwards.</p>
        <div class="grid">
          {#each KITS as k (k.id)}
            <div class="card">
              <div class="thumb"><LayoutThumb node={k.make()} /></div>
              <div class="meta">
                <span class="ic"><Icon name={k.icon} size={14} /></span>
                <div class="txt"><b>{k.title}</b><i>{k.summary}</i></div>
              </div>
              <div class="acts">
                <button type="button" class="primary" onclick={() => { workspaces.attach(shell.active, k.make(), "right"); close(); }}><Icon name="layout-sidebar-right" size={13} />Add right</button>
                <button type="button" onclick={() => { workspaces.attach(shell.active, k.make(), "bottom", 0.4); close(); }}>Add below</button>
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <p class="lead">One pane, added to the focused group of tabs on <b>{current?.name}</b>. Or drag one onto any edge to split.</p>
        <div class="list">
          {#each panes as p (p.type)}
            <button type="button" class="pane" onclick={() => { workspaces.open(shell.active, pane(p.type)); close(); }}>
              <Icon name={p.icon} size={15} />
              <b>{p.title}</b>
              <i>{p.summary}</i>
              <span class="grp">{p.group}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 900;
    background: rgba(0, 0, 0, 0.4);
  }
  .dlg {
    position: fixed;
    z-index: 901;
    top: 6vh;
    left: 50%;
    transform: translateX(-50%);
    width: min(1040px, calc(100vw - 40px));
    max-height: 86vh;
    display: flex;
    flex-direction: column;
    background: var(--s1);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-3);
    box-shadow: var(--shadow);
  }
  header {
    display: flex;
    align-items: center;
    padding: 8px 8px 8px 12px;
    border-bottom: 1px solid var(--line);
  }
  .tabs {
    flex: 1;
    display: flex;
    gap: 2px;
  }
  .tabs button {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 12px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--fg-3);
    border-radius: var(--r-2);
  }
  .tabs button.on {
    color: var(--fg);
    background: var(--s3);
  }
  .x {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    color: var(--fg-3);
    border-radius: var(--r-2);
  }
  .x:hover {
    color: var(--fg);
    background: var(--s3);
  }
  .body {
    overflow-y: auto;
    padding: 12px;
  }
  .lead {
    margin: 0 0 12px;
    font-size: 12.5px;
    color: var(--fg-2);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    gap: 10px;
  }
  .card {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--line);
    border-radius: var(--r-2);
    background: var(--s0);
    overflow: hidden;
  }
  .thumb {
    display: flex;
    height: 120px;
    padding: 6px;
    background: var(--inset);
    border-bottom: 1px solid var(--line);
  }
  .thumb.empty {
    align-items: center;
    justify-content: center;
    color: var(--fg-4);
  }
  .meta {
    display: flex;
    gap: 8px;
    padding: 8px 10px 4px;
    flex: 1;
  }
  .ic {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    flex-shrink: 0;
    border-radius: var(--r-1);
    color: var(--c, var(--fg-2));
    background: color-mix(in oklab, var(--c, var(--fg-3)) 16%, transparent);
  }
  .txt {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .txt b {
    font-size: 13px;
  }
  .txt i {
    font-style: normal;
    font-size: 11.5px;
    color: var(--fg-3);
    line-height: 1.4;
  }
  .acts {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px 10px;
  }
  .acts button {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding: 0 10px;
    font-size: 12px;
    font-weight: 600;
    color: var(--fg);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-1);
  }
  .acts button:hover {
    background: var(--s3);
  }
  .acts button.primary {
    color: var(--on-accent);
    background: var(--accent);
    border-color: var(--accent);
  }
  .note {
    font-size: 11px;
    color: var(--fg-3);
  }
  .list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(420px, 1fr));
    gap: 2px 12px;
  }
  .pane {
    display: grid;
    grid-template-columns: 18px 130px 1fr auto;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 8px;
    text-align: left;
    border-radius: var(--r-1);
    color: var(--fg-2);
  }
  .pane:hover {
    background: var(--s2);
    color: var(--fg);
  }
  .pane b {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--fg);
  }
  .pane i {
    font-style: normal;
    font-size: 11.5px;
    color: var(--fg-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .grp {
    font-size: 10.5px;
    color: var(--fg-4);
  }
</style>
