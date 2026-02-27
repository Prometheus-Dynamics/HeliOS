import type { Edge } from '@xyflow/svelte';
import type { PipelineGraphPlan } from '$lib/types/pipeline';
import type { EdgeSelection, PipelineEdgeData } from './types';

export function findConnectionForSelection(selection: EdgeSelection | null, plan: PipelineGraphPlan) {
  if (!selection) return null;
  return plan.connections.find(
    (connection) =>
      connection.from.node === selection.from.node &&
      connection.from.port === selection.from.port &&
      connection.to.node === selection.to.node &&
      connection.to.port === selection.to.port
  );
}

export function deriveSelectionFromId(
  edgeId: string | null,
  edges: Edge[],
  plan: PipelineGraphPlan
): EdgeSelection | null {
  if (!edgeId) return null;
  const edgeMatch = edges.find((edge) => edge.id === edgeId || (edge.data as PipelineEdgeData | undefined)?.edgeId === edgeId);
  if (edgeMatch) {
    const data = edgeMatch.data as PipelineEdgeData | undefined;
    if (edgeMatch.source && edgeMatch.target && data?.fromPort && data?.toPort) {
      return {
        id: data.edgeId ?? edgeMatch.id,
        from: { node: edgeMatch.source, port: data.fromPort, dataType: data.fromType },
        to: { node: edgeMatch.target, port: data.toPort, dataType: data.toType }
      };
    }
  }
  const connection = plan.connections.find((conn, index) => {
    const id = `${conn.from.node}-${conn.to.node}-${conn.from.port}-${conn.to.port}-${index}`;
    return id === edgeId;
  });
  if (!connection) return null;
  return {
    id: edgeId,
    from: { node: connection.from.node, port: connection.from.port, dataType: plan.nodes[connection.from.node]?.outputs?.[connection.from.port] },
    to: { node: connection.to.node, port: connection.to.port, dataType: plan.nodes[connection.to.node]?.inputs?.[connection.to.port] }
  };
}
