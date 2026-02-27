import { connectionSignature, extractPortFromHandle } from './utils';
import type { ChannelPolicy, PipelineGraphPlan } from '$lib/types/pipeline';
import type { Edge, Node } from '@xyflow/svelte';
import type { PipelineEdgeData } from './types';
import { DEFAULT_CONNECTION_STYLE } from './edgeStyle';

const normalizeStyleSignature = (style: unknown): string => {
  if (!style || typeof style !== 'object') return '';
  const record = style as Record<string, unknown>;
  const route = typeof record.route === 'string' ? record.route : '';
  const curvature = typeof record.curvature === 'number' ? record.curvature : '';
  const emphasis = typeof record.emphasis === 'string' ? record.emphasis : '';
  const dashed = typeof record.dashed === 'boolean' ? record.dashed : '';
  return `${route}|${curvature}|${emphasis}|${dashed}`;
};

const DEFAULT_STYLE_SIGNATURE = normalizeStyleSignature(DEFAULT_CONNECTION_STYLE);

export const rebuildPlan = (
  graph: PipelineGraphPlan,
  nodes: Node[],
  edges: Edge[],
  edgeInteractionsEnabled: boolean
): PipelineGraphPlan => {
  let nodesChanged = false;
  const updatedNodes = Object.fromEntries(
    Object.entries(graph.nodes).map(([id, node]) => {
      const flowNode = nodes.find((candidate) => candidate.id === id);
      if (!flowNode) {
        return [id, node];
      }
      const nextX = flowNode.position.x;
      const nextY = flowNode.position.y;
      const currentX = node.info?.location?.x ?? 0;
      const currentY = node.info?.location?.y ?? 0;
      if (currentX === nextX && currentY === nextY) {
        return [id, node];
      }
      nodesChanged = true;
      return [
        id,
        {
          ...node,
          info: {
            ...node.info,
            location: {
              x: nextX,
              y: nextY
            }
          }
        }
      ];
    })
  );

  if (!edgeInteractionsEnabled) {
    if (!nodesChanged) {
      return graph;
    }
    return { ...graph, nodes: updatedNodes };
  }

  const edgeMap = new Map<
    string,
    { fromNode: string; fromPort: string; toNode: string; toPort: string; style: Record<string, unknown> | null }
  >();
  const edgeSignatureOrder: string[] = [];
  edges.forEach((edge) => {
    const edgeData = edge.data as PipelineEdgeData | undefined;
    const fromPort = edgeData?.fromPort ?? extractPortFromHandle(edge.sourceHandle ?? null);
    const toPort = edgeData?.toPort ?? extractPortFromHandle(edge.targetHandle ?? null);
    if (!fromPort || !toPort || !edge.source || !edge.target) return;
    const signature = `${edge.source}:${fromPort}->${edge.target}:${toPort}`;
    const styleCandidate =
      edgeData?.style && typeof edgeData.style === 'object' ? ({ ...(edgeData.style as Record<string, unknown>) } as Record<string, unknown>) : null;
    edgeMap.set(signature, {
      fromNode: edge.source,
      fromPort,
      toNode: edge.target,
      toPort,
      style: styleCandidate
    });
    edgeSignatureOrder.push(signature);
  });

  const updatedConnections: PipelineGraphPlan['connections'] = [];
  const consumed = new Set<string>();
  let connectionsChanged = false;

  // Preserve the existing connection ordering to avoid accidental churn/dirtying.
  (graph.connections ?? []).forEach((connection) => {
    const signature = connectionSignature(connection);
    const match = edgeMap.get(signature);
    if (!match) {
      // Edge removed in flow state; omit it.
      connectionsChanged = true;
      return;
    }
    consumed.add(signature);
    const nextPolicy: ChannelPolicy = connection.policy ?? 'NewestWins';
    const existingStyle = connection.style && typeof connection.style === 'object' ? { ...(connection.style as Record<string, unknown>) } : null;
    const candidateStyle = match.style;
    const candidateSig = normalizeStyleSignature(candidateStyle);
    const existingSig = normalizeStyleSignature(existingStyle);
    const shouldIncludeStyle = existingStyle != null || (candidateStyle != null && candidateSig !== DEFAULT_STYLE_SIGNATURE);
    const styleOut = shouldIncludeStyle
      ? (candidateStyle != null && candidateSig !== DEFAULT_STYLE_SIGNATURE ? candidateStyle : existingStyle)
      : null;

    if (
      connection.from.node !== match.fromNode ||
      connection.from.port !== match.fromPort ||
      connection.to.node !== match.toNode ||
      connection.to.port !== match.toPort ||
      (connection.policy ?? 'NewestWins') !== nextPolicy ||
      existingSig !== normalizeStyleSignature(styleOut)
    ) {
      connectionsChanged = true;
    }

    updatedConnections.push({
      from: { node: match.fromNode, port: match.fromPort },
      to: { node: match.toNode, port: match.toPort },
      policy: nextPolicy,
      ...(styleOut ? { style: styleOut as any } : {})
    });
  });

  // Append any new edges that were created in the flow UI but aren't in the plan yet.
  edgeSignatureOrder.forEach((signature) => {
    if (consumed.has(signature)) return;
    const match = edgeMap.get(signature);
    if (!match) return;
    connectionsChanged = true;
    const candidateStyle = match.style;
    const candidateSig = normalizeStyleSignature(candidateStyle);
    const styleOut = candidateStyle != null && candidateSig !== DEFAULT_STYLE_SIGNATURE ? candidateStyle : null;
    updatedConnections.push({
      from: { node: match.fromNode, port: match.fromPort },
      to: { node: match.toNode, port: match.toPort },
      policy: 'NewestWins',
      ...(styleOut ? { style: styleOut as any } : {})
    });
  });

  if (!nodesChanged && !connectionsChanged) {
    return graph;
  }
  return { ...graph, nodes: updatedNodes, connections: updatedConnections };
};
