import type { Edge } from '@xyflow/svelte';
import type { PipelineConnectionStyle, PipelineGraphPlan } from '$lib/types/pipeline';
import type { EdgeSelection, PipelineEdgeData } from './types';

export function applyConnectionStyleToGraph(options: {
  selection: EdgeSelection;
  nextStyle: PipelineConnectionStyle | null;
  commit: boolean;
  plan: PipelineGraphPlan;
  edges: Edge[];
  defaultStyle: PipelineConnectionStyle;
  normalize: (style: PipelineConnectionStyle) => PipelineConnectionStyle;
}): { plan: PipelineGraphPlan; edges: Edge[] } {
  const normalized = options.nextStyle ? options.normalize(options.nextStyle) : options.defaultStyle;
  let nextPlan = options.plan;
  if (options.commit) {
    nextPlan = {
      ...options.plan,
      connections: options.plan.connections.map((connection) => {
        const matches =
          connection.from.node === options.selection.from.node &&
          connection.from.port === options.selection.from.port &&
          connection.to.node === options.selection.to.node &&
          connection.to.port === options.selection.to.port;
        if (!matches) return connection;
        const stylePayload = options.nextStyle ? { ...options.nextStyle } : undefined;
        return {
          ...connection,
          ...(stylePayload ? { style: stylePayload } : { style: undefined })
        };
      })
    };
  }

  const nextEdges = options.edges.map((edge) => {
    const data = edge.data as PipelineEdgeData | undefined;
    if (
      edge.source === options.selection.from.node &&
      edge.target === options.selection.to.node &&
      data?.fromPort === options.selection.from.port &&
      data?.toPort === options.selection.to.port
    ) {
      return {
        ...edge,
        data: {
          ...(edge.data as PipelineEdgeData),
          style: normalized
        }
      };
    }
    return edge;
  });

  return { plan: nextPlan, edges: nextEdges };
}
