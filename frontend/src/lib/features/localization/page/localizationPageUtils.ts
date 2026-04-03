import type { LocalizationMarker } from '$lib';
import type { StreamMetrics } from '$lib/api/client';
import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
import { cameraKeyForSource } from '$lib/features/localization/utils';

export const applyDeviceSeparation = (params: {
  markers: LocalizationMarker[];
  separateCameras: boolean;
  selectedSources: LocalizationPipelineSource[];
  primaryCameraKey?: string | null;
}): LocalizationMarker[] => {
  const { markers, separateCameras, selectedSources, primaryCameraKey } = params;
  if (!separateCameras) return markers;
  if (selectedSources.length <= 1) return markers;

  const stride = 2.2;
  const cameraOrder: string[] = [];
  const seen = new Set<string>();
  selectedSources.forEach((source) => {
    const key = cameraKeyForSource(source) ?? source.id;
    if (!key || seen.has(key)) return;
    seen.add(key);
    cameraOrder.push(key);
  });
  if (cameraOrder.length <= 1) return markers;
  if (primaryCameraKey && seen.has(primaryCameraKey)) {
    const nextOrder = cameraOrder.filter((key) => key !== primaryCameraKey);
    nextOrder.unshift(primaryCameraKey);
    cameraOrder.splice(0, cameraOrder.length, ...nextOrder);
  }
  const sourceIndex = new Map<string, number>(cameraOrder.map((key, index) => [key, index]));
  return markers.map((marker) => {
    const source = marker.source;
    const key = cameraKeyForSource(source ?? null) ?? '';
    const index = key ? sourceIndex.get(key) ?? 0 : 0;
    if (!index) return marker;
    const [x, y, z] = marker.position;
    return { ...marker, position: [x + index * stride, y, z] } as LocalizationMarker;
  });
};

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
