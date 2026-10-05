<script lang="ts">
  // A tab stack: tabs (drag to reorder, move, or dock beside another stack),
  // the active pane's toolbar in the same row, and the pane itself.
  import Icon from "$lib/components/common/Icon.svelte";
  import { colorVar, identity } from "$lib/core/identity.svelte";
  import { menu } from "$lib/core/menu.svelte";
  import { pane as makePane, workspaces, type DropZone, type LayoutNode, type PaneRef } from "$lib/core/workspace.svelte";
  import { setBarSlot } from "./bar";
  import { NEW_PANE_MIME, PANE_MIME, addPaneTo, paneMenu } from "./library";
  import { paneDef } from "./panes";
  import { help } from "$lib/shell/help.svelte";

  let { stack, ws }: { stack: Extract<LayoutNode, { kind: "stack" }>; ws: string } = $props();

  let barTarget = $state<HTMLElement | null>(null);
  let body = $state<HTMLDivElement>();
  let zone = $state<DropZone | null>(null);

  // The pane toolbar shares the tab row while it fits, and drops to its own
  // row underneath when it doesn't, so nothing is ever clipped.
  let header = $state<HTMLDivElement>();
  let strip = $state<HTMLDivElement>();
  let wrap = $state(false);
  $effect(() => {
    if (!header || !barTarget || !strip) return;
    const measure = () => {
      const tools = barTarget!.firstElementChild as HTMLElement | null;
      const need = tools ? tools.scrollWidth + 12 : 0;
      const free = header!.clientWidth - Math.min(strip!.scrollWidth, header!.clientWidth * 0.6) - 28;
      wrap = need > free;
    };
    const ro = new ResizeObserver(measure);
    ro.observe(header);
    const mo = new MutationObserver(measure);
    mo.observe(barTarget, { childList: true, subtree: true });
    measure();
    return () => {
      ro.disconnect();
      mo.disconnect();
    };
  });

  const active = $derived(stack.tabs[stack.active]);
  const focused = $derived(workspaces.focusedStack === stack.id);
  setBarSlot({
    get target() {
      return barTarget;
    },
    get active() {
      return true;
    },
  });

  function title(t: PaneRef): string {
    const def = paneDef(t.type);
    const fromProps = def?.label?.(t.props ?? {});
    return fromProps || def?.title || t.type;
  }
  function tabColor(t: PaneRef): string | null {
    const id = paneDef(t.type)?.identityOf?.(t.props ?? {});
    return id ? colorVar(identity.get(id).color) : null;
  }

  function dragStart(event: DragEvent, index: number) {
    event.dataTransfer?.setData(PANE_MIME, JSON.stringify({ from: stack.id, index }));
    event.dataTransfer!.effectAllowed = "move";
    workspaces.dragging = { from: stack.id, index };
  }

  function accepts(event: DragEvent) {
    const types = event.dataTransfer?.types ?? [];
    return types.includes(PANE_MIME) || types.includes(NEW_PANE_MIME);
  }

  function zoneAt(event: DragEvent): DropZone {
    const r = body!.getBoundingClientRect();
    const x = (event.clientX - r.left) / r.width;
    const y = (event.clientY - r.top) / r.height;
    const edge = Math.min(x, 1 - x, y, 1 - y);
    if (edge > 0.25) return "center";
    if (edge === x) return "left";
    if (edge === 1 - x) return "right";
    if (edge === y) return "top";
    return "bottom";
  }

  function drop(event: DragEvent, target: DropZone) {
    event.preventDefault();
    zone = null;
    const moved = event.dataTransfer?.getData(PANE_MIME);
    const fresh = event.dataTransfer?.getData(NEW_PANE_MIME);
    if (moved) {
      const { from, index } = JSON.parse(moved);
      workspaces.move(ws, from, index, stack.id, target);
    } else if (fresh) {
      if (target === "center") {
        workspaces.focusedStack = stack.id;
        workspaces.open(ws, makePane(fresh));
      } else workspaces.split(ws, stack.id, makePane(fresh), target);
    }
    workspaces.dragging = null;
  }

  function tabMenu(event: MouseEvent, index: number) {
    const t = stack.tabs[index];
    menu.context(() => [
      { label: "Close", icon: "x", run: () => workspaces.close(ws, stack.id, index) },
      { label: "Close others", disabled: stack.tabs.length < 2, run: () => {
          while (stack.tabs.length > 1) workspaces.close(ws, stack.id, stack.tabs[0] === t ? 1 : 0);
        } },
      { separator: true },
      { label: "Move to the right", icon: "layout-grid", disabled: stack.tabs.length < 2, run: () => workspaces.move(ws, stack.id, index, stack.id, "right") },
      { label: "Move below", disabled: stack.tabs.length < 2, run: () => workspaces.move(ws, stack.id, index, stack.id, "bottom") },
      { label: workspaces.maximized === stack.id ? "Restore" : "Maximize", icon: "arrows-maximize", shortcut: "dbl-click", run: toggleMax },
    ])(event);
  }

  function stackMenu(event: MouseEvent) {
    const add = addPaneTo(ws, stack.id);
    const closed = workspaces.closed[ws] ?? [];
    menu.below(event.currentTarget as Element, [
      { heading: "Add here" },
      ...paneMenu(add),
      { label: "Reopen closed", icon: "history", disabled: !closed.length, items: [...closed].reverse().map((c) => ({ label: title(c.ref), icon: paneDef(c.ref.type)?.icon, run: () => workspaces.reopen(ws, c.ref.id) })) },
      { separator: true },
      { label: "Split right", icon: "layout-grid", items: paneMenu(addPaneTo(ws, stack.id, "right")) },
      { label: "Split down", icon: "layout-list", items: paneMenu(addPaneTo(ws, stack.id, "bottom")) },
      { separator: true },
      { label: workspaces.maximized === stack.id ? "Restore" : "Maximize", icon: "arrows-maximize", run: toggleMax },
      { label: "Close all here", icon: "x", danger: true, run: () => [...stack.tabs].forEach(() => workspaces.close(ws, stack.id, 0)) },
    ], "end");
  }

  function toggleMax() {
    workspaces.maximized = workspaces.maximized === stack.id ? null : stack.id;
  }
</script>

<div class="stack" class:focused role="group" onpointerdowncapture={() => (workspaces.focusedStack = stack.id)}>
  <div
    class="tabs"
    class:wrap
    bind:this={header}
    role="tablist"
    tabindex="-1"
    ondblclick={(e) => {
      if ((e.target as HTMLElement).closest(".tabs-strip") || e.target === e.currentTarget) toggleMax();
    }}
    ondragover={(e) => {
      if (accepts(e)) {
        e.preventDefault();
        zone = "center";
      }
    }}
    ondragleave={() => (zone = null)}
    ondrop={(e) => drop(e, "center")}
  >
    <div class="tabs-strip" bind:this={strip}>
      {#each stack.tabs as t, i (t.id)}
        {@const def = paneDef(t.type)}
        {@const color = tabColor(t)}
        <div
          class="tab"
          data-tip={def ? `${def.title}: ${def.summary}` : undefined}
          class:active={i === stack.active}
          role="tab"
          tabindex="0"
          aria-selected={i === stack.active}
          draggable="true"
          ondragstart={(e) => dragStart(e, i)}
          ondragend={() => (workspaces.dragging = null)}
          onclick={() => workspaces.activate(ws, stack.id, i)}
          onkeydown={(e) => e.key === "Enter" && workspaces.activate(ws, stack.id, i)}
          onauxclick={(e) => e.button === 1 && workspaces.close(ws, stack.id, i)}
          oncontextmenu={(e) => tabMenu(e, i)}
          style:--tab-color={color}
        >
          <span class="tab-icon"><Icon name={def?.icon ?? "alert-triangle"} size={13} stroke={1.9} /></span>
          <span class="tab-title">{title(t)}</span>
          <button type="button" class="tab-x" aria-label="Close {title(t)}" onclick={(e) => { e.stopPropagation(); workspaces.close(ws, stack.id, i); }}>
            <Icon name="x" size={11} stroke={2.2} />
          </button>
        </div>
      {/each}
    </div>
    <button type="button" class="stack-btn" aria-label="Pane options" data-tip="Add a pane here, split, reopen closed, maximize" onclick={stackMenu}>
      <Icon name="dots-vertical" size={14} />
    </button>
    <div class="bar" bind:this={barTarget}></div>
  </div>

  <div
    class="body"
    bind:this={body}
    role="tabpanel"
    tabindex="-1"
    ondragover={(e) => {
      if (accepts(e)) {
        e.preventDefault();
        zone = zoneAt(e);
      }
    }}
    ondragleave={(e) => {
      if (!body?.contains(e.relatedTarget as Node)) zone = null;
    }}
    ondrop={(e) => drop(e, zoneAt(e))}
  >
    {#if active}
      {@const def = paneDef(active.type)}
      {#key active.id}
        {#if def}
          <def.component pane={active} {ws} />
        {:else}
          <div class="missing">
            <Icon name="alert-triangle" size={18} />
            <span>No pane called “{active.type}” in this build.</span>
          </div>
        {/if}
      {/key}
    {/if}
    {#if help.explain && active}
      {@const def = paneDef(active.type)}
      <button type="button" class="explain" onclick={() => (help.explain = false)}>
        <span class="ex-title"><Icon name={def?.icon ?? "circle-dashed"} size={16} />{def?.title ?? active.type}</span>
        <span class="ex-body">{def?.summary}</span>
        {#if stack.tabs.length > 1}
          <span class="ex-more">Also in this group: {stack.tabs.filter((t) => t !== active).map((t) => title(t)).join(", ")}</span>
        {/if}
      </button>
    {/if}
    {#if workspaces.dragging || zone}
      <div class="drop-catcher" class:hidden={!zone}>
        {#if zone}<div class="zone z-{zone}"></div>{/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    width: 100%;
    min-width: 0;
    min-height: 0;
    background: var(--s1);
    border: 1px solid var(--line);
    border-radius: var(--r-2);
    overflow: hidden;
  }
  .stack.focused {
    border-color: color-mix(in oklab, var(--accent) 28%, var(--line));
  }
  .tabs {
    display: flex;
    flex-wrap: wrap;
    align-items: stretch;
    min-height: 30px;
    flex-shrink: 0;
    background: var(--s2);
    border-bottom: 1px solid var(--line);
  }
  .tabs > * {
    height: 30px;
  }
  .tabs-strip {
    order: 0;
    display: flex;
    min-width: 0;
    flex: 0 1 auto;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .tab {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 200px;
    padding: 0 4px 0 9px;
    font-size: 12px;
    font-weight: 500;
    color: var(--fg-3);
    border-right: 1px solid var(--line);
    cursor: default;
    user-select: none;
    flex-shrink: 0;
  }
  .tab:hover {
    color: var(--fg-2);
    background: color-mix(in oklab, var(--s3) 60%, transparent);
  }
  .tab.active {
    color: var(--fg);
    background: var(--s1);
  }
  .tab.active::before {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    height: 2px;
    background: var(--tab-color, var(--accent));
  }
  .stack:not(.focused) .tab.active::before {
    opacity: 0.45;
  }
  .tab-icon {
    display: inline-flex;
    color: var(--tab-color, currentColor);
  }
  .tab-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tab-x {
    display: inline-flex;
    padding: 3px;
    border-radius: var(--r-1);
    color: var(--fg-3);
    opacity: 0;
  }
  .tab:hover .tab-x,
  .tab.active .tab-x {
    opacity: 1;
  }
  .tab-x:hover {
    background: var(--s3);
    color: var(--fg);
  }
  .bar {
    order: 1;
    flex: 1 1 0;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    min-width: 0;
    padding: 0 2px 0 8px;
    overflow: hidden;
  }
  .stack-btn {
    order: 2;
  }
  .tabs:not(.wrap) .tabs-strip:only-child {
    flex: 1;
  }
  /* Toolbar too wide for the tab row: give it its own row. */
  .wrap .tabs-strip {
    flex: 1 1 0;
  }
  .wrap .bar {
    order: 3;
    flex: 1 0 100%;
    justify-content: flex-start;
    padding: 0 4px;
    overflow-x: auto;
    scrollbar-width: thin;
    border-top: 1px solid var(--line);
    background: var(--s1);
  }
  .stack-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    color: var(--fg-3);
    flex-shrink: 0;
  }
  .stack-btn:hover {
    color: var(--fg);
    background: var(--s3);
  }
  .body {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    /* Panes adapt to their own width (see Prop, Seg). */
    container: pane / inline-size;
  }
  .missing {
    margin: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--fg-3);
  }
  .explain {
    position: absolute;
    inset: 0;
    z-index: 30;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 16px;
    text-align: center;
    background: color-mix(in oklab, var(--s0) 86%, transparent);
    border: 2px dashed var(--accent-ring);
    border-radius: var(--r-1);
    backdrop-filter: blur(2px);
  }
  .ex-title {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 15px;
    font-weight: 650;
    color: var(--fg);
  }
  .ex-title :global(svg) {
    color: var(--accent);
  }
  .ex-body {
    max-width: 340px;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--fg-2);
  }
  .ex-more {
    max-width: 340px;
    font-size: 11.5px;
    color: var(--fg-3);
  }
  .drop-catcher {
    position: absolute;
    inset: 0;
    z-index: 20;
  }
  .drop-catcher.hidden {
    pointer-events: auto;
  }
  .zone {
    position: absolute;
    background: color-mix(in oklab, var(--accent) 18%, transparent);
    border: 2px solid var(--accent);
    border-radius: var(--r-2);
    pointer-events: none;
    transition: all 90ms var(--ease-out);
  }
  .z-center {
    inset: 6px;
  }
  .z-left {
    inset: 6px 50% 6px 6px;
  }
  .z-right {
    inset: 6px 6px 6px 50%;
  }
  .z-top {
    inset: 6px 6px 50% 6px;
  }
  .z-bottom {
    inset: 50% 6px 6px 6px;
  }
</style>
