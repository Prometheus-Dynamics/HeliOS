<script lang="ts">
  // Logs from every device, coloured by device. Filter by level, device or
  // text; follow the tail or scroll back freely.
  import { clockTime } from "#lib/format.js";
  import { colorVar, identity } from "#lib/core/identity.svelte.js";
  import { menu } from "#lib/core/menu.svelte.js";
  import { workspaces } from "#lib/core/workspace.svelte.js";
  import IconButton from "#lib/kit/IconButton.svelte";
  import Seg from "#lib/kit/Seg.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import { downloadText } from "#lib/files.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "#lib/workspace/panes.js";

  let { pane, ws }: PaneProps = $props();
  const level = $derived((pane.props?.level as string) ?? "all");
  const nodes = $derived((pane.props?.nodes as string[] | undefined) ?? []);
  let query = $state("");
  let follow = $state(true);
  let scroller = $state<HTMLDivElement>();

  const lines = $derived(
    cluster.logs.filter(
      (l) =>
        (level === "all" || (level === "warn" ? l.level !== "info" : l.level === "error")) &&
        (!nodes.length || nodes.includes(l.nodeId)) &&
        (!query || `${l.unit} ${l.text}`.toLowerCase().includes(query.toLowerCase())),
    ),
  );
  const shown = $derived(lines.slice(-400));

  $effect(() => {
    void shown.length;
    if (follow && scroller) queueMicrotask(() => scroller && (scroller.scrollTop = scroller.scrollHeight));
  });

  function deviceMenu(e: MouseEvent) {
    menu.below(e.currentTarget as Element, [
      { heading: "Devices" },
      { label: "All devices", checked: !nodes.length, run: () => workspaces.setProps(ws, pane.id, { nodes: [] }) },
      ...cluster.nodes.map((n) => ({
        label: identity.name(n.id, n.name),
        checked: nodes.includes(n.id),
        run: () => workspaces.setProps(ws, pane.id, { nodes: nodes.includes(n.id) ? nodes.filter((x) => x !== n.id) : [...nodes, n.id] }),
      })),
    ], "end");
  }
</script>

<PaneBar>
  <input class="q" placeholder="Filter…" bind:value={query} aria-label="Filter logs" />
  <Seg label="Level" value={level} options={[{ value: "all", label: "All" }, { value: "warn", label: "Warn+" }, { value: "error", label: "Errors" }]} onchange={(v) => workspaces.setProps(ws, pane.id, { level: v })} />
  <IconButton icon="server" label="Devices" active={nodes.length > 0} size={24} onclick={deviceMenu} />
  <IconButton icon="arrow-right" label={follow ? "Following the tail" : "Follow the tail"} active={follow} size={24} onclick={() => (follow = !follow)} />
  <IconButton icon="download" label="Save these lines" size={24} onclick={() => downloadText("helios-logs.txt", lines.map((l) => `${new Date(l.ts).toISOString()} ${l.nodeId} ${l.unit} ${l.level.toUpperCase()} ${l.text}`).join("\n"))} />
</PaneBar>

<div
  class="logs"
  bind:this={scroller}
  onscroll={() => {
    if (!scroller) return;
    follow = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 24;
  }}
>
  {#each shown as l (l.ts + l.unit + l.text)}
    <div class="line lv-{l.level}">
      <span class="t">{clockTime(l.ts)}</span>
      <span class="dev" style:--c={colorVar(identity.get(l.nodeId).color)}>{identity.name(l.nodeId, cluster.node(l.nodeId)?.name ?? l.nodeId)}</span>
      <span class="u">{l.unit}</span>
      <span class="msg">{l.text}</span>
    </div>
  {/each}
</div>

<style>
  .q {
    width: 120px;
    height: 22px;
    padding: 0 7px;
    margin-right: 4px;
    font: inherit;
    font-size: 11.5px;
    color: var(--fg);
    background: var(--inset);
    border: 1px solid var(--line);
    border-radius: var(--r-1);
    outline: none;
  }
  .q:focus {
    border-color: var(--accent-ring);
  }
  .logs {
    flex: 1;
    overflow-y: auto;
    padding: 4px 0;
    font-family: var(--font-code);
    font-size: 11.5px;
    line-height: 19px;
  }
  .line {
    display: flex;
    gap: 8px;
    padding: 0 10px;
    white-space: nowrap;
  }
  .line:hover {
    background: var(--s2);
  }
  .t {
    color: var(--fg-4);
  }
  .dev {
    width: 86px;
    flex-shrink: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--c);
  }
  .u {
    width: 100px;
    flex-shrink: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--fg-3);
  }
  .msg {
    color: var(--fg-2);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .lv-warn .msg {
    color: var(--warn);
  }
  .lv-error {
    background: var(--err-bg);
  }
  .lv-error .msg {
    color: var(--err);
  }
</style>
