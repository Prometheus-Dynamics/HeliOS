<script lang="ts">
  // A graph node on the canvas, kept compact: a header (category, title,
  // plugin), one row per port pair (inputs left, outputs right) and a footer
  // with live timing and how many parameters differ from the catalog. Values
  // stay in Pipeline settings.
  import { Handle, Position, type NodeProps } from "@xyflow/svelte";
  import { CATALOG_BY_ID } from "#lib/api/catalog.js";
  import Icon from "#lib/components/common/Icon.svelte";
  import { ms } from "#lib/format.js";
  import { getEditorLive } from "./context";
  import { CATEGORY_COLORS, CATEGORY_ICONS, changedParams, portColor, portTypeLabel, type FlowNode } from "./graph";

  let { id, data, selected }: NodeProps<FlowNode> = $props();

  const live = getEditorLive();
  const type = $derived(CATALOG_BY_ID[data.type]);
  const color = $derived(type ? CATEGORY_COLORS[type.category] : "var(--fg-faint)");
  const changed = $derived(changedParams(type, data.params).length);
  const timing = $derived(live?.running ? live.nodeMs[id] : undefined);
  const share = $derived(timing !== undefined && live && live.tickMs > 0 ? Math.min(1, timing / live.tickMs) : 0);
  const inputs = $derived(type?.inputs ?? []);
  const outputs = $derived(type?.outputs ?? []);
  const rows = $derived(Array.from({ length: Math.max(inputs.length, outputs.length) }, (_, i) => i));
</script>

<div class="node" class:selected class:unknown={!type} style="--cat: {color}" aria-label="{data.label} node">
  <header>
    <span class="cat-icon"><Icon name={type ? CATEGORY_ICONS[type.category] : "alert-triangle"} size={13} /></span>
    <div class="min-w-0 flex-1">
      <div class="title truncate">{data.label}</div>
      <div class="sub truncate">{type ? `${type.category} · ${type.plugin}` : `unknown type ${data.type}`}</div>
    </div>
  </header>

  {#if rows.length}
    <div class="ports">
      {#each rows as i (i)}
        {@const input = inputs[i]}
        {@const output = outputs[i]}
        <div class="row">
          <div class="port in">
            {#if input}
              <Handle type="target" position={Position.Left} id={input.name} class="typed-handle" style="--port: {portColor(input.type)}" />
              <span class="name truncate" title="{input.name}: {portTypeLabel(input.type)}">{input.name}</span>
            {/if}
          </div>
          <div class="port out">
            {#if output}
              <span class="name truncate" title="{output.name}: {portTypeLabel(output.type)}">{output.name}</span>
              <Handle type="source" position={Position.Right} id={output.name} class="typed-handle" style="--port: {portColor(output.type)}" />
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}

  <footer>
    <Icon name="stopwatch" size={12} />
    <span class="mono">{timing !== undefined ? ms(timing) : "—"}</span>
    <span class="bar" title={timing !== undefined ? `${Math.round(share * 100)}% of the tick` : undefined} aria-hidden="true"><span style="width: {share * 100}%"></span></span>
    {#if changed}<span class="changed" title="Parameters that differ from the default">{changed} changed</span>{/if}
  </footer>
</div>

<style>
  /* Sizes mirror NODE_* in graph.ts (the auto-layout uses the same numbers). */
  .node {
    width: 216px;
    background: var(--layer);
    border: 1px solid var(--glass-border);
    border-radius: var(--r-card);
    box-shadow: var(--shadow-lift);
    color: var(--fg);
    font-size: 12px;
    position: relative;
    transition:
      border-color var(--t-fast),
      box-shadow var(--t-fast);
  }
  .node::before {
    content: "";
    position: absolute;
    inset: 0 0 auto 0;
    height: 2px;
    border-radius: var(--r-card) var(--r-card) 0 0;
    background: var(--cat);
    opacity: 0.8;
  }
  .node:hover {
    border-color: var(--glass-border-strong);
  }
  .node.selected {
    border-color: var(--accent-ring);
    box-shadow:
      0 0 0 3px var(--accent-tint),
      var(--shadow-lift);
  }
  .node.unknown {
    border-style: dashed;
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 44px;
    box-sizing: border-box;
    padding: 0 10px;
  }
  .cat-icon {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 6px;
    color: var(--cat);
    background: color-mix(in srgb, var(--cat) 14%, transparent);
    flex-shrink: 0;
  }
  .title {
    font-weight: 600;
    font-size: 12.5px;
    line-height: 1.2;
  }
  .sub {
    font-size: 10.5px;
    color: var(--fg-faint);
  }
  .ports {
    padding: 4px 0 6px;
    border-top: 1px solid var(--hairline);
  }
  .row {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    height: 22px;
  }
  .port {
    position: relative;
    display: flex;
    align-items: center;
    flex: 1 1 0;
    min-width: 0;
    color: var(--fg-muted);
    font-size: 11px;
  }
  .port.in {
    padding-left: 12px;
    justify-content: flex-start;
  }
  .port.out {
    padding-right: 12px;
    justify-content: flex-end;
  }
  .name {
    min-width: 0;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    box-sizing: border-box;
    padding: 0 10px;
    border-top: 1px solid var(--hairline);
    color: var(--fg-faint);
    font-size: 10.5px;
  }
  footer .mono {
    color: var(--fg-muted);
  }
  .bar {
    flex: 1;
    min-width: 24px;
    height: 3px;
    border-radius: 2px;
    background: var(--glass-strong);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--cat);
    opacity: 0.75;
  }
  .changed {
    flex-shrink: 0;
    padding: 1px 6px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 500;
    white-space: nowrap;
    color: var(--accent-fg);
    background: var(--accent-tint);
  }
  .node :global(.typed-handle) {
    width: 10px;
    height: 10px;
    background: var(--layer);
    border: 2px solid var(--port);
  }
  .node :global(.typed-handle.connectionindicator:hover),
  .node :global(.typed-handle.connectingfrom) {
    background: var(--port);
  }
  .node :global(.typed-handle.connectingto.valid) {
    background: var(--port);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--port) 30%, transparent);
  }
  .node :global(.typed-handle.connectingto:not(.valid)) {
    border-color: var(--err);
  }
</style>
