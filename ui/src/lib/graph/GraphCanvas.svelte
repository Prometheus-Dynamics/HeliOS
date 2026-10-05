<script lang="ts">
  // The pipeline canvas for one workload's draft. Must sit inside a
  // SvelteFlowProvider. Edits go to the shared draft (see drafts.svelte.ts).
  import { untrack } from "svelte";
  import { Background, BackgroundVariant, MiniMap, SvelteFlow, useSvelteFlow, type Connection, type Node, type NodeTypes, type OnConnectEnd } from "@xyflow/svelte";
  import { CATALOG_BY_ID } from "$lib/api/catalog";
  import { prefs } from "$lib/core/prefs.svelte";
  import { selection } from "$lib/core/selection.svelte";
  import { toasts } from "$lib/stores/toasts.svelte";
  import { setEditorLive } from "./context";
  import type { Draft } from "./drafts.svelte";
  import { CATEGORY_COLORS, DRAG_MIME, checkConnection, edgeId, makeEdge, type FlowEdge, type FlowNode } from "./graph";
  import PipelineNode from "./PipelineNode.svelte";

  let { draft, minimap = true }: { draft: Draft; minimap?: boolean } = $props();

  const nodeTypes = { pipeline: PipelineNode } as unknown as NodeTypes;
  const flow = useSvelteFlow<FlowNode, FlowEdge>();
  const workload = $derived(draft.workload);
  const nodeSum = $derived(workload ? Object.values(workload.perf.nodeMs).reduce((s, v) => s + v, 0) : 0);
  const light = $derived(prefs.theme === "daylight" || prefs.theme === "field");

  setEditorLive({
    get nodeMs() {
      return workload?.perf.nodeMs ?? {};
    },
    get tickMs() {
      return Math.max(workload?.perf.tickP50Ms ?? 0, nodeSum);
    },
    get running() {
      return workload?.state === "running";
    },
  });

  $effect(() => {
    if (draft.fitSignal) untrack(() => setTimeout(() => fit(200), 30));
  });

  // Keep the app selection and the canvas selection in step.
  $effect(() => {
    const s = selection.current;
    if (s?.kind === "graph-node" && s.parent === draft.workloadId) untrack(() => draft.selected?.id !== s.id && draft.select(s.id));
  });

  let host = $state<HTMLDivElement>();

  // Fit once the canvas has a real size (panes can mount at zero height).
  function fit(duration = 0) {
    flow.fitView({ padding: 0.12, duration, maxZoom: 1 });
  }
  $effect(() => {
    if (!host) return;
    let last = 0;
    const ro = new ResizeObserver(([e]) => {
      const h = e.contentRect.height;
      if (last < 40 && h >= 40) setTimeout(() => fit(), 30);
      last = h;
    });
    ro.observe(host);
    return () => ro.disconnect();
  });

  function ondragover(event: DragEvent) {
    if (!event.dataTransfer?.types.includes(DRAG_MIME)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
  }
  function ondrop(event: DragEvent) {
    const id = event.dataTransfer?.getData(DRAG_MIME);
    const type = id ? CATALOG_BY_ID[id] : undefined;
    if (!type) return;
    event.preventDefault();
    const p = flow.screenToFlowPosition({ x: event.clientX, y: event.clientY });
    draft.add(type, { x: Math.round(p.x - 95), y: Math.round(p.y - 20) });
    const added = draft.nodes.at(-1);
    if (added) selection.select({ kind: "graph-node", id: added.id, parent: draft.workloadId });
  }

  function isValidConnection(c: Connection | FlowEdge): boolean {
    return checkConnection(draft.nodes, draft.edges, c).ok;
  }
  function onbeforeconnect(c: Connection): FlowEdge | null {
    const verdict = checkConnection(draft.nodes, draft.edges, c);
    if (!verdict.ok) {
      toasts.warning(verdict.reason);
      return null;
    }
    draft.snapshot();
    return makeEdge(draft.nodes, edgeId(), c.source, c.sourceHandle ?? "", c.target, c.targetHandle ?? "");
  }
  function onconnectend(_event: MouseEvent | TouchEvent, state: Parameters<OnConnectEnd>[1]) {
    if (!state.fromHandle || !state.toHandle || state.isValid !== false) return;
    const out = state.fromHandle.type === "source" ? state.fromHandle : state.toHandle;
    const inp = state.fromHandle.type === "source" ? state.toHandle : state.fromHandle;
    const verdict = checkConnection(draft.nodes, draft.edges, { source: out.nodeId, sourceHandle: out.id ?? null, target: inp.nodeId, targetHandle: inp.id ?? null });
    toasts.warning(verdict.ok ? "Connect an output to an input" : verdict.reason);
  }

  function onkeydown(event: KeyboardEvent) {
    if (!(event.metaKey || event.ctrlKey)) return;
    const k = event.key.toLowerCase();
    if (k === "z" && !event.shiftKey) {
      event.preventDefault();
      draft.undo();
    } else if ((k === "z" && event.shiftKey) || k === "y") {
      event.preventDefault();
      draft.redo();
    } else if (k === "s") {
      event.preventDefault();
      if (draft.dirty) draft.deploy();
    } else if (k === "l") {
      event.preventDefault();
      draft.layout();
    }
  }

  function minimapColor(n: Node) {
    const t = CATALOG_BY_ID[(n.data as FlowNode["data"] | undefined)?.type ?? ""];
    return t ? CATEGORY_COLORS[t.category] : "var(--fg-3)";
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="canvas" bind:this={host} {ondragover} {ondrop} {onkeydown} role="application" aria-label="Pipeline graph" tabindex="-1">
  <SvelteFlow
    bind:nodes={draft.nodes}
    bind:edges={draft.edges}
    {nodeTypes}
    colorMode={light ? "light" : "dark"}
    oninit={() => setTimeout(() => fit(), 30)}
    minZoom={0.2}
    maxZoom={2}
    deleteKey={["Backspace", "Delete"]}
    {isValidConnection}
    {onbeforeconnect}
    {onconnectend}
    onbeforedelete={async () => {
      draft.snapshot();
      return true;
    }}
    onnodedragstart={() => draft.snapshot()}
    onnodeclick={({ node }) => selection.select({ kind: "graph-node", id: node.id, parent: draft.workloadId })}
    onpaneclick={() => selection.select({ kind: "workload", id: draft.workloadId })}
    defaultEdgeOptions={{ class: "typed-edge" }}
    connectionRadius={24}
    proOptions={{ hideAttribution: true }}
  >
    <Background variant={BackgroundVariant.Dots} gap={20} size={1.1} />
    {#if minimap}
      <MiniMap position="bottom-right" nodeColor={minimapColor} nodeBorderRadius={3} pannable zoomable width={130} height={80} />
    {/if}
  </SvelteFlow>
  {#if !draft.nodes.length}
    <div class="empty">Drag nodes in from the catalog, or start from a template.</div>
  {/if}
</div>

<style>
  .canvas {
    position: relative;
    flex: 1;
    min-height: 0;
    outline: none;
    --xy-background-color: transparent;
    --xy-background-pattern-dots-color: var(--line-strong);
    --xy-edge-stroke: var(--fg-3);
    --xy-edge-stroke-selected: var(--fg);
    --xy-edge-stroke-width: 1.6;
    --xy-connectionline-stroke: var(--fg-2);
    --xy-connectionline-stroke-width: 1.6;
    --xy-minimap-background-color: var(--s2);
    --xy-minimap-mask-background-color: color-mix(in oklab, var(--s0) 60%, transparent);
    --xy-minimap-mask-stroke-color: var(--line-strong);
    --xy-minimap-node-stroke-color: transparent;
    --xy-selection-background-color: var(--accent-tint);
    --xy-selection-border: 1px solid var(--accent-ring);
  }
  .canvas :global(.svelte-flow) {
    background: transparent;
  }
  .canvas :global(.svelte-flow__minimap) {
    border: 1px solid var(--line);
    border-radius: var(--r-1);
    overflow: hidden;
    margin: 8px;
  }
  .canvas :global(.typed-edge .svelte-flow__edge-path) {
    stroke: var(--edge-color, var(--fg-3));
    stroke-opacity: 0.7;
  }
  .canvas :global(.typed-edge:hover .svelte-flow__edge-path),
  .canvas :global(.typed-edge.selected .svelte-flow__edge-path) {
    stroke-opacity: 1;
  }
  .canvas :global(.typed-edge.selected .svelte-flow__edge-path) {
    stroke-width: 2.6;
  }
  .canvas :global(.svelte-flow__node:focus-visible) {
    outline: none;
  }
  .empty {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    pointer-events: none;
    color: var(--fg-3);
    font-size: 12.5px;
  }
</style>
