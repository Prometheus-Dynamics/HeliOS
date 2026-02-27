<script lang="ts">
  import { BaseEdge, EdgeLabel, getBezierPath, getSmoothStepPath, getStraightPath, type EdgeProps } from '@xyflow/svelte';
  import type { PipelineConnectionRoute } from '$lib/types/pipeline';
  import type { PipelineEdgeData } from './types';
  import {
    DEFAULT_CONNECTION_STYLE,
    connectionStrokeWidth,
    isTeleportStyle,
    type NormalizedConnectionStyle
  } from './edgeStyle';

  const props: EdgeProps = $props();

  const data = $derived((props.data ?? null) as PipelineEdgeData | null);
  const style = $derived((data?.style ?? DEFAULT_CONNECTION_STYLE) as NormalizedConnectionStyle);
  const interactionWidth = $derived(
    typeof props.interactionWidth === 'number' && Number.isFinite(props.interactionWidth)
      ? props.interactionWidth
      : 56
  );
  const resolvedLabel = $derived(props.label ?? null);
  const resolvedLabelStyle = $derived(props.labelStyle);
  const isTeleport = $derived(isTeleportStyle(style));

  const buildPath = (route: PipelineConnectionRoute) => {
    switch (route) {
      case 'straight': {
        const [path, labelX, labelY] = getStraightPath({
          sourceX: props.sourceX,
          sourceY: props.sourceY,
          targetX: props.targetX,
          targetY: props.targetY
        });
        return { path, labelX, labelY };
      }
      case 'step': {
        const [path, labelX, labelY] = getSmoothStepPath({
          sourceX: props.sourceX,
          sourceY: props.sourceY,
          targetX: props.targetX,
          targetY: props.targetY,
          sourcePosition: props.sourcePosition,
          targetPosition: props.targetPosition,
          borderRadius: 0,
          offset: 12
        });
        return { path, labelX, labelY };
      }
      default: {
        const [path, labelX, labelY] = getBezierPath({
          sourceX: props.sourceX,
          sourceY: props.sourceY,
          targetX: props.targetX,
          targetY: props.targetY,
          sourcePosition: props.sourcePosition,
          targetPosition: props.targetPosition,
          curvature: style.curvature
        });
        return { path, labelX, labelY };
      }
    }
  };

  const pathInfo = $derived(buildPath(isTeleport ? 'bezier' : style.route));

  const parseStroke = (value: unknown): string | null => {
    if (!value) return null;
    if (typeof value === 'string') {
      const match = value.match(/stroke:\s*([^;]+)/i);
      return match ? match[1].trim() : null;
    }
    if (typeof value === 'object' && 'stroke' in value) {
      const stroke = (value as { stroke?: unknown }).stroke;
      return typeof stroke === 'string' ? stroke : null;
    }
    return null;
  };
  const baseColor = $derived(data?.fromColor ?? parseStroke(props.style) ?? 'var(--flow-accent)');
  const accentColor = $derived(
    props.selected
      ? `color-mix(in srgb, ${baseColor} 70%, var(--flow-edge-highlight) 30%)`
      : baseColor
  );
  const strokeWidth = $derived(connectionStrokeWidth(style) + (props.selected ? 0.35 : 0));
  const dash = $derived(style.dashed ? '8 6' : null);
  const opacity = $derived(() => {
    if (props.selected) return 1;
    if (style.emphasis === 'soft') return 0.75;
    if (style.emphasis === 'bold') return 1;
    return 0.9;
  });
  const edgeStyle = $derived(
    [
      `stroke:${accentColor}`,
      `stroke-width:${strokeWidth.toFixed(2)}`,
      `opacity:${opacity}`,
      dash ? `stroke-dasharray:${dash}` : null,
      'stroke-linecap:round',
      'stroke-linejoin:round'
    ]
      .filter(Boolean)
      .join('; ')
  );

  type PortalGeometry = {
    sourceEnd: { x: number; y: number };
    targetEnd: { x: number; y: number };
    normal: { x: number; y: number };
    tracePath: string;
  };

  const portal = $derived.by<PortalGeometry>(() => {
    const dx = props.targetX - props.sourceX;
    const dy = props.targetY - props.sourceY;
    const distance = Math.max(Math.hypot(dx, dy), 1);
    const dir = { x: dx / distance, y: dy / distance };
    const normal = { x: -dir.y, y: dir.x };
    const desiredStub = Math.max(10, Math.min(120, distance * 0.25));
    const maxStub = Math.max(8, (distance - 12) / 2);
    const stub = Math.min(desiredStub, maxStub);
    const sourceEnd = { x: props.sourceX + dir.x * stub, y: props.sourceY + dir.y * stub };
    const targetEnd = { x: props.targetX - dir.x * stub, y: props.targetY - dir.y * stub };
    return {
      sourceEnd,
      targetEnd,
      normal,
      tracePath: pathInfo.path
    };
  });

  const portalLabelOffset = 18;
  const sourceLabel = $derived(
    data?.toNodeName ? `to ${data.toNodeName}` : data?.toPort ? `to ${data.toPort}` : 'to target'
  );
  const targetLabel = $derived(
    data?.fromNodeName ? `from ${data.fromNodeName}` : data?.fromPort ? `from ${data.fromPort}` : 'from source'
  );
</script>

{#if isTeleport}
  <g class={`pipeline-edge pipeline-edge--teleport ${props.class ?? ''}`.trim()}>
    <path
      class="svelte-flow__edge-interaction"
      d={pathInfo.path}
      stroke-opacity={0}
      stroke-width={interactionWidth}
      fill="none"
    />
    <path
      class="pipeline-edge__trace"
      d={pathInfo.path}
      style={`stroke:${accentColor};stroke-width:${(strokeWidth * 0.9).toFixed(2)};stroke-dasharray:4 18;opacity:${props.selected ? 0.35 : 0.12};fill:none`}
    />
    <path
      class="pipeline-edge__stub"
      d={`M ${props.sourceX} ${props.sourceY} L ${portal.sourceEnd.x} ${portal.sourceEnd.y}`}
      style={edgeStyle}
    />
    <path
      class="pipeline-edge__stub"
      d={`M ${portal.targetEnd.x} ${portal.targetEnd.y} L ${props.targetX} ${props.targetY}`}
      style={edgeStyle}
    />
    <g
      class="pipeline-edge__portal"
      transform={`translate(${portal.sourceEnd.x} ${portal.sourceEnd.y})`}
      style={`--pipeline-portal-color:${accentColor}`}
    >
      <circle r="8.5" class="pipeline-edge__portal-ring" />
      <circle r="5.5" class="pipeline-edge__portal-core" />
    </g>
    <g
      class="pipeline-edge__portal"
      transform={`translate(${portal.targetEnd.x} ${portal.targetEnd.y})`}
      style={`--pipeline-portal-color:${accentColor}`}
    >
      <circle r="8.5" class="pipeline-edge__portal-ring" />
      <circle r="5.5" class="pipeline-edge__portal-core" />
    </g>
    {#if sourceLabel}
      <EdgeLabel
        x={portal.sourceEnd.x + portal.normal.x * portalLabelOffset}
        y={portal.sourceEnd.y + portal.normal.y * portalLabelOffset}
        style="font-size:0.62rem;letter-spacing:0.08em;padding:2px 6px;border-radius:6px;background:color-mix(in srgb, var(--flow-surface-soft, #0f172a) 85%, transparent);border:1px solid color-mix(in srgb, var(--flow-border, #1f2937) 80%, transparent);opacity:0.95;color:var(--flow-text,#f8fafc);"
        selectEdgeOnClick
      >
        {sourceLabel}
      </EdgeLabel>
    {/if}
    {#if targetLabel}
      <EdgeLabel
        x={portal.targetEnd.x - portal.normal.x * portalLabelOffset}
        y={portal.targetEnd.y - portal.normal.y * portalLabelOffset}
        style="font-size:0.62rem;letter-spacing:0.08em;padding:2px 6px;border-radius:6px;background:color-mix(in srgb, var(--flow-surface-soft, #0f172a) 85%, transparent);border:1px solid color-mix(in srgb, var(--flow-border, #1f2937) 80%, transparent);opacity:0.95;color:var(--flow-text,#f8fafc);"
        selectEdgeOnClick
      >
        {targetLabel}
      </EdgeLabel>
    {/if}
    {#if resolvedLabel}
      <EdgeLabel x={pathInfo.labelX} y={pathInfo.labelY} style={resolvedLabelStyle} selectEdgeOnClick>
        {resolvedLabel}
      </EdgeLabel>
    {/if}
  </g>
{:else}
  <BaseEdge
    id={props.id}
    path={pathInfo.path}
    label={resolvedLabel ?? undefined}
    labelX={pathInfo.labelX}
    labelY={pathInfo.labelY}
    labelStyle={resolvedLabelStyle}
    markerStart={props.markerStart}
    markerEnd={props.markerEnd}
    interactionWidth={interactionWidth}
    style={edgeStyle}
    class={props.class}
  />
{/if}

<style>
  :global(.pipeline-edge__trace) {
    pointer-events: none;
  }

  :global(.pipeline-edge__portal-core) {
    fill: color-mix(in srgb, var(--pipeline-portal-color, var(--flow-accent)) 88%, transparent);
    stroke: color-mix(in srgb, var(--pipeline-portal-color, var(--flow-accent)) 80%, transparent);
    stroke-width: 1.25;
  }

  :global(.pipeline-edge__portal-ring) {
    fill: color-mix(in srgb, var(--pipeline-portal-color, var(--flow-accent)) 18%, transparent);
    stroke: color-mix(in srgb, var(--pipeline-portal-color, var(--flow-accent)) 60%, transparent);
    stroke-width: 1.4;
  }

  :global(.pipeline-edge__portal) {
    filter: drop-shadow(0 0 6px color-mix(in srgb, var(--pipeline-portal-color, var(--flow-accent)) 55%, transparent));
    pointer-events: none;
  }
</style>
