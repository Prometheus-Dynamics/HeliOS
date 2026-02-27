import type { PipelineNodeRuntimeMetrics } from '$lib/types/pipeline';

export function formatTimestamp(timestamp: number | null | undefined): string {
  if (!timestamp) return '—';
  const date = new Date(timestamp * 1000);
  return date.toLocaleString();
}

export function formatHeatDuration(value: number | null | undefined): string {
  if (!Number.isFinite(value) || !value || value <= 0) return '—';
  if (value >= 10) return `${value.toFixed(0)} ms`;
  if (value >= 1) return `${value.toFixed(1)} ms`;
  return `${Math.round(value * 1000)} µs`;
}

export function formatAverageTime(value: number | undefined): string {
  if (value == null || Number.isNaN(value)) return '—';
  return `${value.toFixed(2)} ms`;
}

export function formatAverageFps(value: number | undefined): string {
  if (value == null || Number.isNaN(value)) return '—';
  return `${value.toFixed(1)} fps`;
}

export function formatSampleAge(age: number | undefined | null): string {
  if (age == null || Number.isNaN(age)) return '—';
  if (age < 1000) {
    return `${Math.round(age)} ms ago`;
  }
  const seconds = Math.round(age / 1000);
  if (seconds < 60) {
    return `${seconds}s ago`;
  }
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) {
    return `${minutes}m ago`;
  }
  const hours = Math.round(minutes / 60);
  if (hours < 24) {
    return `${hours}h ago`;
  }
  const days = Math.round(hours / 24);
  return `${days}d ago`;
}

export function totalEdgeDrops(entry: PipelineNodeRuntimeMetrics | null | undefined): number {
  if (!entry) return 0;
  return entry.outputEdges?.reduce((sum, edge) => sum + edge.dropped, 0) ?? 0;
}

export function describeEdgeDrops(entry: PipelineNodeRuntimeMetrics | null | undefined): string {
  if (!entry || (entry.outputEdges?.length ?? 0) === 0) {
    return '';
  }
  return entry.outputEdges
    .map(
      (edge) =>
        `${edge.port} (${edge.policy}) – dropped ${edge.dropped}, depth ${edge.depth}/${edge.capacity}`
    )
    .join('\n');
}

export function summarizeSyncEvents(entry: PipelineNodeRuntimeMetrics | null | undefined) {
  if (!entry?.inputSync || entry.inputSync.length === 0) {
    return [];
  }
  return entry.inputSync.map((stat) => ({
    port: stat.port,
    drops: stat.drops ?? 0,
    stale: stat.stale ?? 0,
    missing: stat.missing ?? 0,
    total: (stat.drops ?? 0) + (stat.stale ?? 0) + (stat.missing ?? 0)
  }));
}

export function describeSyncSummary(summary: ReturnType<typeof summarizeSyncEvents>): string {
  if (summary.length === 0) return '';
  return summary.map((stat) => `${stat.port}: drop ${stat.drops}, stale ${stat.stale}, missing ${stat.missing}`).join('\n');
}

export function nodeMetricsHaveDetails(entry: PipelineNodeRuntimeMetrics | null | undefined): boolean {
  if (!entry) return false;
  return (
    (entry.outputEdges?.length ?? 0) > 0 ||
    (entry.inputQueues?.length ?? 0) > 0 ||
    (entry.outputSinks?.length ?? 0) > 0 ||
    (entry.inputSources?.length ?? 0) > 0 ||
    (entry.inputSync?.length ?? 0) > 0
  );
}

export function revisionDisplay(revision: string | null | undefined): string {
  if (!revision) return '—';
  return revision.slice(0, 12);
}

export const metricsRowKey = (streamId: string, nodeId: string): string => `${streamId}:${nodeId}`;

export function normalizeHeatmapNodeId(value: string | null | undefined): string | null {
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

export function matchesHeatmapSearch(nodeId: string, filters: { searchQuery: string; nodeIndex: Record<string, { keywords: string }> }): boolean {
  if (!filters.searchQuery) return true;
  const entry = filters.nodeIndex[nodeId];
  if (!entry) return false;
  return entry.keywords.includes(filters.searchQuery);
}

export function shouldIncludeHeatmapNode(
  nodeId: string,
  filters: {
    viewMode: 'all' | 'workload' | 'boundary';
    boundaryIndex: Record<string, boolean>;
    excludedNodes: Record<string, boolean>;
    searchQuery: string;
    nodeIndex: Record<string, { keywords: string }>;
    minAverageMs: number | null;
    minSampleCount: number | null;
  }
): boolean {
  if (!nodeId) return false;
  const isBoundary = filters.boundaryIndex[nodeId] ?? false;
  if (filters.excludedNodes[nodeId]) return false;
  if (filters.viewMode === 'workload') return !isBoundary;
  if (filters.viewMode === 'boundary') return isBoundary;
  if (!matchesHeatmapSearch(nodeId, filters)) return false;
  return true;
}
