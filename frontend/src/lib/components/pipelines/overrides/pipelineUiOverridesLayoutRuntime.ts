import type { StreamPipelineLayout } from '$lib/api/client';
import { StreamsApi } from '$lib/api/streamsApi';
import { normalizeGridOutputKeys } from '$lib/features/devices/camera/page/cameraPipelineState';
import type { PipelineUiControl } from '$lib/features/pipelines/pipelineUiTypes';
import { reportError } from '$lib/ui/errorPolicy';
import { clampGridSize } from '$lib/components/pipelines/overrides/pipelineUiOverridesPanels';

export function buildLayoutPayload(
  control: PipelineUiControl,
  pipelineId: string | null,
  rawPipelineId: string,
  rawPipelineUuid: string
): StreamPipelineLayout | null {
  if (control.type !== 'layout_toggle' || !control.layout) return null;

  const rows = clampGridSize(control.layout.rows ?? 1);
  const columns = clampGridSize(control.layout.columns ?? 1);
  const outputKeys = normalizeGridOutputKeys(rows, columns, control.layout.outputKeys ?? {});
  const resolvedPipelineId = resolveLayoutPipelineId(pipelineId, rawPipelineId, rawPipelineUuid);
  if (!resolvedPipelineId) return null;

  const slots = Object.entries(outputKeys)
    .map(([key, value]) => {
      const [rowRaw, columnRaw] = key.split(':');
      const row = Math.trunc(Number(rowRaw));
      const column = Math.trunc(Number(columnRaw));
      if (!Number.isInteger(row) || !Number.isInteger(column)) return null;
      if (row < 0 || column < 0 || row >= rows || column >= columns) return null;
      const outputKey = typeof value === 'string' ? value.trim() : '';
      if (!outputKey) return null;
      return { row, column, pipeline_id: resolvedPipelineId, output_key: outputKey };
    })
    .filter(Boolean);

  return { rows, columns, slots: slots as Array<{ row: number; column: number; pipeline_id: string; output_key: string }> };
}

export function disableOtherLayoutToggles(
  localValues: Record<string, string>,
  controls: PipelineUiControl[],
  activeId: string
): Record<string, string> | null {
  if (!controls.length) return null;
  const next = { ...localValues };
  let updated = false;
  controls.forEach((control) => {
    if (control.id === activeId) return;
    if (next[control.id] === 'true') {
      next[control.id] = 'false';
      updated = true;
    }
  });
  return updated ? next : null;
}

export async function persistStreamLayout(
  streamKey: string,
  payload: StreamPipelineLayout | null,
  shouldReportError: () => boolean
): Promise<void> {
  try {
    await StreamsApi.setPipelineLayout({ id: streamKey, requestBody: { pipeline_layout: payload ?? null } });
  } catch (error) {
    if (!shouldReportError()) return;
    reportError({
      title: 'Pipeline layout failed',
      error,
      fallback: 'Unable to update the pipeline layout right now.'
    });
  }
}

export async function resolveBaselineLayout(
  streamKey: string,
  streamLayout: StreamPipelineLayout | null,
  currentStreamId: string | null
): Promise<StreamPipelineLayout | null> {
  if (streamLayout && streamKey === currentStreamId) {
    return streamLayout ?? null;
  }
  try {
    const entry = await StreamsApi.getStream({ id: streamKey }, { cacheMs: 0 });
    return ((entry as { manifest?: { pipeline_layout?: StreamPipelineLayout | null } })?.manifest?.pipeline_layout ?? null);
  } catch (error) {
    console.warn('Failed to load stream layout baseline', error);
    return null;
  }
}

function resolveLayoutPipelineId(value: string | null, rawPipelineId: string, rawPipelineUuid: string): string | null {
  const trimmed = typeof value === 'string' ? value.trim() : '';
  if (!trimmed) return null;
  if (rawPipelineId && rawPipelineUuid && trimmed === rawPipelineId) return rawPipelineUuid;
  return trimmed;
}
