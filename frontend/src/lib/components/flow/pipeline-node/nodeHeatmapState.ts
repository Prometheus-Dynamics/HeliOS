import { clamp01 } from '../nodePalette';
import { heatColorForIntensity } from './heatUtils';
import type { PipelineNodeHeatmapPayload } from '../pipeline-graph/types';

export type NodeHeatmapState = {
  hasMetrics: boolean;
  averageTime: number | null;
  globalAverage: number | null;
  averageDelta: number | null;
  intensity: number;
  active: boolean;
  perfMode: boolean;
  color: string | null;
  overlayOpacity: number;
  borderOpacity: number;
  borderSize: string;
  effectiveBorderColor: string;
};

export function buildNodeHeatmapState(params: {
  heatmap: PipelineNodeHeatmapPayload | null;
  heatmapMode: boolean;
  borderColor: string;
}): NodeHeatmapState {
  const hasMetrics = Boolean(params.heatmap);
  const averageTime =
    hasMetrics &&
    Number.isFinite(params.heatmap?.totalTimeMs) &&
    (params.heatmap?.totalTimeMs ?? 0) > 0
      ? params.heatmap?.totalTimeMs ?? null
      : null;
  const globalAverage =
    hasMetrics &&
    Number.isFinite(params.heatmap?.globalAverageTimeMs ?? null) &&
    (params.heatmap?.globalAverageTimeMs ?? 0) > 0
      ? params.heatmap?.globalAverageTimeMs ?? null
      : null;
  const averageDelta =
    hasMetrics &&
    typeof params.heatmap?.deltaFromAverageMs === 'number' &&
    Number.isFinite(params.heatmap?.deltaFromAverageMs)
      ? params.heatmap?.deltaFromAverageMs ?? null
      : null;
  const intensity = hasMetrics ? clamp01(params.heatmap?.normalized ?? 0) : 0;
  const active = Boolean(params.heatmapMode);
  const perfMode = active;
  const color = (() => {
    if (!active) return null;
    if (hasMetrics) {
      return heatColorForIntensity(intensity);
    }
    return 'rgba(148, 163, 184, 0.85)';
  })();
  const overlayOpacity = active && hasMetrics ? clamp01(0.02 + intensity * 0.12) : 0;
  const borderOpacity = (() => {
    if (!active) return 0;
    const base = 0.65;
    return clamp01(base + (hasMetrics ? intensity * 0.3 : 0));
  })();
  const borderWidth = (() => {
    if (!active) return 0;
    const base = 8;
    return hasMetrics ? base + intensity * 6 : base;
  })();
  const borderSize = `${borderWidth.toFixed(2)}px`;
  const effectiveBorderColor =
    active && color ? `color-mix(in srgb, ${color} 70%, ${params.borderColor} 30%)` : params.borderColor;

  return {
    hasMetrics,
    averageTime,
    globalAverage,
    averageDelta,
    intensity,
    active,
    perfMode,
    color,
    overlayOpacity,
    borderOpacity,
    borderSize,
    effectiveBorderColor
  };
}
