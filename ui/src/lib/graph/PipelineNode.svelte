<script lang="ts">
  // A graph node on the canvas: a glass card with its category hint, typed
  // ports (inputs left, outputs right), key params and live timing.
  import { Handle, Position, type NodeProps } from "@xyflow/svelte";
  import { CATALOG_BY_ID } from "$lib/api/catalog";
  import Icon from "$lib/components/common/Icon.svelte";
  import { ms } from "$lib/format";
  import { getEditorLive } from "./context";
  import { CATEGORY_COLORS, CATEGORY_ICONS, formatParam, keyParams, portColor, portTypeLabel, type FlowNode } from "./graph";

  let { id, data, selected }: NodeProps<FlowNode> = $props();

  const live = getEditorLive();
  const type = $derived(CATALOG_BY_ID[data.type]);
  const color = $derived(type ? CATEGORY_COLORS[type.category] : "var(--fg-faint)");
  const params = $derived(keyParams(type, data.params));
  const timing = $derived(live?.running ? live.nodeMs[id] : undefined);
  const share = $derived(timing !== undefined && live && live.tickMs > 0 ? Math.min(1, timing / live.tickMs) : 0);
  const rows = $derived(Math.max(type?.inputs.length ?? 0, type?.outputs.length ?? 0));
</script>

<div class="node" class:selected class:unknown={!type} style="--cat: {color}" aria-label="{data.label} node">
  <header>
    <span class="cat-icon"><Icon name={type ? CATEGORY_ICONS[type.category] : "alert-triangle"} size={13} /></span>
    <div class="min-w-0 flex-1">
      <div class="title truncate">{data.label}</div>
      <div class="sub truncate">{type ? `${type.category} · ${type.plugin}` : `unknown type ${data.type}`}</div>
    </div>
  </header>

  {#if rows}
    <div class="ports">
      <div class="col">
        {#each type?.inputs ?? [] as port (port.name)}
          <div class="port in" title="{port.name}: {port.type}">
            <Handle type="target" position={Position.Left} id={port.name} class="typed-handle" style="--port: {portColor(port.type)}" />
            <span class="truncate">{port.name}</span>
          </div>
        {/each}
      </div>
      <div class="col right">
        {#each type?.outputs ?? [] as port (port.name)}
          <div class="port out" title="{port.name}: {portTypeLabel(port.type)}">
            <span class="truncate">{port.name}</span>
            <Handle type="source" position={Position.Right} id={port.name} class="typed-handle" style="--port: {portColor(port.type)}" />
          </div>
        {/each}
      </div>
    </div>
  {/if}

  {#if params.length}
    <dl class="params">
      {#each params as row (row.param.name)}
        <div class:changed={row.changed}>
          <dt class="truncate">{row.param.name}</dt>
          <dd class="mono truncate">{formatParam(row.value)}</dd>
        </div>
      {/each}
    </dl>
  {/if}

  <footer>
    <Icon name="stopwatch" size={12} />
    <span class="mono">{timing !== undefined ? ms(timing) : "—"}</span>
    <span class="bar" aria-hidden="true"><span style="width: {share * 100}%"></span></span>
    {#if timing !== undefined}<span class="mono share">{Math.round(share * 100)}%</span>{/if}
  </footer>
</div>

<style>
  .node {
    width: 210px;
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
    padding: 9px 10px 7px;
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
    display: flex;
    justify-content: space-between;
    gap: 8px;
    padding: 4px 0 6px;
    border-top: 1px solid var(--hairline);
  }
  .col {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .col.right {
    align-items: flex-end;
  }
  .port {
    position: relative;
    display: flex;
    align-items: center;
    height: 18px;
    padding: 0 10px;
    color: var(--fg-muted);
    font-size: 11px;
    max-width: 120px;
  }
  .params {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 6px 10px;
    border-top: 1px solid var(--hairline);
  }
  .params div {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }
  .params dt {
    color: var(--fg-faint);
    font-size: 10.5px;
  }
  .params dd {
    color: var(--fg-muted);
    font-size: 10.5px;
    max-width: 50%;
  }
  .params .changed dd {
    color: var(--fg);
  }
  footer {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px 8px;
    border-top: 1px solid var(--hairline);
    color: var(--fg-faint);
    font-size: 10.5px;
  }
  footer .mono {
    color: var(--fg-muted);
  }
  .bar {
    flex: 1;
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
  .share {
    min-width: 26px;
    text-align: right;
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
