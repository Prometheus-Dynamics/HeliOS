import type { ApiPortDescriptor } from '$lib/types/pipeline-api';
import type { Edge } from '@xyflow/svelte';
import { getPortType, makeHandleId, type PortOrders, typeKey } from './utils';
import type {
  PipelineDataType,
  PipelineGraphPlan,
  PipelineRegistryEntry
} from '$lib/types/pipeline';
import { normalizePipelinePortName } from '$lib/features/pipelines/boundary';
import type { ActiveConnection, PipelineEdgeData } from './types';
import { DEFAULT_PORT_COLOR, resolvePortColor } from '../pipeline-node/portUtils';
import {
  DEFAULT_CONNECTION_STYLE,
  connectionStrokeWidth,
  normalizeConnectionStyle,
  type NormalizedConnectionStyle
} from './edgeStyle';

const GENERIC_TYPE_KEYS = new Set(['generic', 'any', 'unknown', 'dynamic']);

const isGenericType = (value: PipelineDataType | undefined): boolean => {
  if (!value) return true;
  if (typeof value === 'string') {
    const trimmed = value.trim().toLowerCase();
    return trimmed.length === 0 || GENERIC_TYPE_KEYS.has(trimmed);
  }
  const kind = typeof value.kind === 'string' ? value.kind.trim().toLowerCase() : '';
  return kind.length === 0 || GENERIC_TYPE_KEYS.has(kind);
};

export const buildEdgeId = (connection: PipelineGraphPlan['connections'][number], index: number) =>
  `${connection.from.node}-${connection.to.node}-${connection.from.port}-${connection.to.port}-${index}`;

const resolveFanInPortType = (
  faninInputs: PipelineRegistryEntry['faninInputs'] | undefined,
  port: string
): PipelineDataType | undefined => {
  if (!faninInputs || faninInputs.length === 0) return undefined;
  const normalizedPort = normalizePipelinePortName(port);
  for (const fanin of faninInputs) {
    const normalizedPrefix = normalizePipelinePortName(fanin.prefix);
    if (!normalizedPort.startsWith(normalizedPrefix)) continue;
    const suffix = normalizedPort.slice(normalizedPrefix.length);
    if (!/^\d+$/u.test(suffix)) continue;
    return fanin.dataType ?? undefined;
  }
  return undefined;
};

const resolvePortType = (
  primary: Record<string, PipelineDataType> | undefined,
  fallback: Record<string, PipelineDataType> | undefined,
  port: string,
  faninInputs?: PipelineRegistryEntry['faninInputs']
): PipelineDataType | undefined => {
  const primaryType = primary?.[port];
  if (primaryType !== undefined) {
    return primaryType;
  }
  if (faninInputs && faninInputs.length > 0) {
    const faninType = resolveFanInPortType(faninInputs, port);
    if (faninType) {
      return faninType;
    }
  }
  const fallbackType = fallback?.[port];
  if (fallbackType) {
    return fallbackType;
  }
  return primaryType;
};

export const resolveEdgeFromColor = (
  graph: PipelineGraphPlan,
  nodeId: string,
  port: string,
  registryEntry?: PipelineRegistryEntry | null
): string => {
  const node = graph.nodes?.[nodeId];
  const resolved = getPortType(graph, nodeId, port, 'source');
  const dataType =
    resolved && !isGenericType(resolved)
      ? resolved
      : resolvePortType(node?.outputs, registryEntry?.outputs, port);
  const descriptor = node?.source?.outputs?.[port];
  return resolvePortColor(dataType, descriptor).color ?? DEFAULT_PORT_COLOR;
};

const makeEdgeData = (
  connection: PipelineGraphPlan['connections'][number],
  graph: PipelineGraphPlan,
  edgeId: string,
  resolveRegistryEntryForNode?: (node: PipelineGraphPlan['nodes'][string]) => PipelineRegistryEntry | null
): PipelineEdgeData => {
  const fromNode = graph.nodes[connection.from.node];
  const toNode = graph.nodes[connection.to.node];
  const fromRegistry = fromNode && resolveRegistryEntryForNode ? resolveRegistryEntryForNode(fromNode) : null;
  const toRegistry = toNode && resolveRegistryEntryForNode ? resolveRegistryEntryForNode(toNode) : null;
  let fromType: PipelineDataType | undefined;
  let toType: PipelineDataType | undefined;

  if (fromNode) {
    fromType = resolvePortType(fromNode.outputs, fromRegistry?.outputs, connection.from.port);
  }

  if (toNode) {
    toType = resolvePortType(toNode.inputs, toRegistry?.inputs, connection.to.port, toRegistry?.faninInputs);
  }

  if (isGenericType(fromType) && !isGenericType(toType)) {
    fromType = toType;
  }
  if (isGenericType(toType) && !isGenericType(fromType)) {
    toType = fromType;
  }

  const fromDescriptor = (fromNode?.source as { outputs?: Record<string, ApiPortDescriptor | undefined> } | null | undefined)?.outputs?.[
    connection.from.port
  ];
  const fromColor =
    resolvePortColor(fromType, fromDescriptor).color ??
    resolveEdgeFromColor(graph, connection.from.node, connection.from.port, fromRegistry);
  return {
    edgeId,
    fromPort: connection.from.port,
    toPort: connection.to.port,
    fromType,
    toType,
    fromColor,
    style: normalizeConnectionStyle(connection.style),
    fromNodeName: fromNode?.metadata?.name ?? connection.from.node,
    toNodeName: toNode?.metadata?.name ?? connection.to.node
  };
};

export const edgeClassName = (active: ActiveConnection | null, data: PipelineEdgeData, isSelected: boolean) => {
  if (isSelected) return undefined;
  const classes: string[] = [];
  if (active && active.typeKey) {
    const candidateKey =
      active.handleType === 'source' ? typeKey(data.fromType) : typeKey(data.toType);
    if (!candidateKey) {
      classes.push('edge-disabled');
    } else if (candidateKey === active.typeKey) {
      classes.push('edge-compatible');
    } else {
      classes.push('edge-incompatible');
    }
  }
  return classes.join(' ') || undefined;
};

export const buildFlowEdges = (
  graph: PipelineGraphPlan,
  selectedId: string | null,
  connection: ActiveConnection | null,
  portOrders: PortOrders,
  resolveRegistryEntryForNode?: (node: PipelineGraphPlan['nodes'][string]) => PipelineRegistryEntry | null
): Edge[] => {
  return graph.connections.flatMap((conn, index) => {
    const id = buildEdgeId(conn, index);
    const data = makeEdgeData(conn, graph, id, resolveRegistryEntryForNode);
    const styleConfig: NormalizedConnectionStyle = data.style ?? DEFAULT_CONNECTION_STYLE;
    const isSelected = !!selectedId && id === selectedId;
    const showLabel = isSelected || !!connection;
    const samePort = conn.from.port === conn.to.port;
    const labelText = samePort ? conn.from.port : `${conn.from.port}→${conn.to.port}`;
    const labelStyle = showLabel
      ? [
          'font-size: 0.65rem',
          'letter-spacing: 0.1em',
          'font-weight: 600',
          'padding: 0.15rem 0.4rem',
          'border-radius: 6px',
          `background: color-mix(in srgb, var(--flow-surface-soft, #111827) ${isSelected ? '90%' : '80%'}, transparent)`,
          'border: 1px solid var(--flow-border, #1f2937)',
          `opacity: ${isSelected ? '1' : '0.9'}`
        ].join('; ')
      : undefined;
    const strokeWidth = connectionStrokeWidth(styleConfig) + (isSelected ? 0.6 : 0);
    const strokeColor = data.fromColor ?? 'var(--flow-accent)';
    const style = isSelected
      ? `stroke: var(--color-secondary-400, #a855f7); stroke-width: ${strokeWidth.toFixed(2)};`
      : [
          `stroke: ${strokeColor}`,
          `stroke-width: ${strokeWidth.toFixed(2)}`,
          styleConfig.dashed ? 'stroke-dasharray: 8 6' : null
        ]
          .filter(Boolean)
          .join('; ');
    const sourceHandle = makeHandleId(graph, portOrders, conn.from, 'source');
    const targetHandle = makeHandleId(graph, portOrders, conn.to, 'target');
    if (!sourceHandle || !targetHandle) {
      console.warn('Edge missing handle metadata', {
        id,
        sourceHandle,
        targetHandle,
        connection: conn
      });
    }
    const edgeClasses = edgeClassName(connection, data, isSelected);
    const extraClasses = styleConfig.route === 'teleport' ? 'edge-teleport' : null;
    const mergedClass = [edgeClasses, extraClasses].filter(Boolean).join(' ') || undefined;
    const edge: Edge = {
      id,
      source: conn.from.node,
      target: conn.to.node,
      type: 'pipeline',
      animated: true,
      selected: isSelected,
      selectable: true,
      class: mergedClass,
      data,
      style,
      labelStyle,
      label: showLabel ? labelText : undefined,
      interactionWidth: styleConfig.route === 'teleport' ? 96 : 84
    };
    if (sourceHandle) {
      edge.sourceHandle = sourceHandle;
    }
    if (targetHandle) {
      edge.targetHandle = targetHandle;
    }
    return [edge];
  });
};
