import type { StreamMetrics } from '$lib/ts-bindings/http/client';

export const pipelineGraphTotalMs = (metrics: StreamMetrics | null | undefined): number | null => {
  const nodes = metrics?.pipeline?.nodes ?? null;
  if (!nodes) return null;
  let total = 0;
  for (const runtime of Object.values(nodes)) {
    const ms = runtime.metrics.average_time_ms;
    if (typeof ms === 'number' && Number.isFinite(ms)) {
      total += ms;
    }
  }
  return total;
};

export const removeStreamMetrics = (params: {
  streamId: string;
  streamMetricsById: Record<string, StreamMetrics>;
  streamMetricsUpdatedAtById: Record<string, number>;
  streamMetricsErrorById: Record<string, string>;
}): {
  streamMetricsById: Record<string, StreamMetrics>;
  streamMetricsUpdatedAtById: Record<string, number>;
  streamMetricsErrorById: Record<string, string>;
} => {
  const { streamId, streamMetricsById, streamMetricsUpdatedAtById, streamMetricsErrorById } = params;
  const rest = { ...streamMetricsById };
  delete rest[streamId];
  const restUpdated = { ...streamMetricsUpdatedAtById };
  delete restUpdated[streamId];
  const restErr = { ...streamMetricsErrorById };
  delete restErr[streamId];
  return {
    streamMetricsById: rest,
    streamMetricsUpdatedAtById: restUpdated,
    streamMetricsErrorById: restErr
  };
};
