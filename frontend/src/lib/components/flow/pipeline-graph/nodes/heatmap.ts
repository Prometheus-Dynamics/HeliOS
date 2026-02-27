import type { PipelineGraphHeatmap, PipelineNodeHeatmapPayload } from '../types';
import type { PipelineGraphPlan } from '$lib/types/pipeline';

const clamp01 = (value: number): number => Math.max(0, Math.min(1, value));
const quantize = (value: number, step: number): number => Math.round(value / step) * step;

export type HeatmapStats = {
  averageTimeMs: number | null;
};

export const deriveHeatmapStats = (
  heatmap: PipelineGraphHeatmap | null | undefined
): HeatmapStats | null => {
  if (!heatmap?.enabled) {
    return null;
  }
  const nodes = Object.values(heatmap.nodes ?? {});
  if (!nodes.length) {
    return null;
  }
  const times = nodes
    .map((node) => Number(node?.totalTimeMs ?? 0))
    .filter((value) => Number.isFinite(value) && value > 0);
  if (times.length === 0) {
    return null;
  }
  const sum = times.reduce((acc, value) => acc + value, 0);
  const average = sum / times.length;
  return {
    averageTimeMs: Number.isFinite(average) && average > 0 ? average : null
  };
};

export const createNodeHeatmapPayload = (
  heatmap: PipelineGraphHeatmap | null | undefined,
  nodeId: string,
  stats: HeatmapStats | null
): PipelineNodeHeatmapPayload | null => {
  if (!heatmap?.enabled || heatmap.maxValue <= 0) {
    return null;
  }
  const entry = heatmap.nodes[nodeId];
  if (!entry) {
    return null;
  }
  const total = Number(entry.totalTimeMs ?? 0);
  if (!Number.isFinite(total) || total <= 0) {
    return null;
  }
  const scaleMin = Math.min(Math.max(heatmap.minValue ?? 0, 0), heatmap.maxValue);
  const range = heatmap.maxValue - scaleMin;
  const normalized = range <= Number.EPSILON ? 1 : clamp01((total - scaleMin) / range);
  const normalizedQuantized = quantize(normalized, 0.01);
  const quantizeMs = (value: number): number => quantize(value, value < 1 ? 0.01 : 0.1);
  const totalQuantized = quantizeMs(total);
  const average = stats?.averageTimeMs ?? null;
  const deltaFromAverage = average == null ? null : total - average;
  const relativeDelta = average && average > 0 && deltaFromAverage != null ? deltaFromAverage / average : null;
  return {
    normalized: normalizedQuantized,
    totalTimeMs: totalQuantized,
    peakTimeMs: quantizeMs(entry.peakTimeMs ?? 0),
    averageFps: entry.averageFps != null ? quantize(entry.averageFps, 0.05) : null,
    sampleCount: entry.sampleCount,
    streamCount: entry.streamCount,
    globalAverageTimeMs: average,
    deltaFromAverageMs: deltaFromAverage != null ? quantizeMs(deltaFromAverage) : null,
    deltaFromAveragePercent: relativeDelta != null ? quantize(relativeDelta, 0.01) : null
  };
};

const normalizeHeatmapNodeId = (value: string | null | undefined): string | null => {
  if (typeof value !== 'string') {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
};

const gatherHeatmapNodeIds = (
  planNode: PipelineGraphPlan['nodes'][string],
  nodeKey: string
): string[] => {
  const ids = new Set<string>();
  const register = (candidate: string | null | undefined) => {
    const normalized = normalizeHeatmapNodeId(candidate);
    if (normalized) {
      ids.add(normalized);
    }
  };
  register(nodeKey);
  register(planNode.id);
  register(planNode.backendId);
  register(planNode.info?.id);
  register(planNode.source?.id);
  register(planNode.source?.info?.id);
  return Array.from(ids);
};

export const resolveNodeHeatmapPayload = (
  heatmap: PipelineGraphHeatmap | null | undefined,
  planNode: PipelineGraphPlan['nodes'][string],
  nodeKey: string,
  stats: HeatmapStats | null
): PipelineNodeHeatmapPayload | null => {
  if (!heatmap?.enabled || heatmap.maxValue <= 0) {
    return null;
  }
  const candidates = gatherHeatmapNodeIds(planNode, nodeKey);
  for (const candidate of candidates) {
    const payload = createNodeHeatmapPayload(heatmap, candidate, stats);
    if (payload) {
      return payload;
    }
  }
  return null;
};
