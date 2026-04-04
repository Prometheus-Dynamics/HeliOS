export type StreamCrop = [number, number, number, number];
export type StreamCrosshair = [number, number];
export type StreamOrderingMode =
  | 'none'
  | 'largest_to_smallest'
  | 'smallest_to_largest'
  | 'top_most'
  | 'bottom_most'
  | 'left_most'
  | 'right_most'
  | 'top_left'
  | 'top_right'
  | 'bottom_left'
  | 'bottom_right'
  | 'center_most'
  | 'crosshair';

const STREAM_ORDERING_MODE_VALUES: readonly StreamOrderingMode[] = [
  'none',
  'largest_to_smallest',
  'smallest_to_largest',
  'top_most',
  'bottom_most',
  'left_most',
  'right_most',
  'top_left',
  'top_right',
  'bottom_left',
  'bottom_right',
  'center_most',
  'crosshair'
];

export const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

export const normalizePipelineUuid = (value: unknown): string | null => {
  if (typeof value !== "string") return null;
  const trimmed = value.trim().toLowerCase();
  return trimmed.length ? trimmed : null;
};

export const clampCropValue = (value: number): number => {
  if (!Number.isFinite(value)) return 0;
  return Math.max(-1, Math.min(1, value));
};

export const normalizeStreamCrop = (crop: StreamCrop): StreamCrop => {
  let [x0, x1, y0, y1] = crop;
  x0 = clampCropValue(x0);
  x1 = clampCropValue(x1);
  y0 = clampCropValue(y0);
  y1 = clampCropValue(y1);
  if (x1 < x0) [x0, x1] = [x1, x0];
  if (y1 < y0) [y0, y1] = [y1, y0];
  return [x0, x1, y0, y1];
};

export const normalizeStreamCrosshair = (crosshair: StreamCrosshair): StreamCrosshair => {
  const x = clampCropValue(Number(crosshair[0] ?? 0));
  const y = clampCropValue(Number(crosshair[1] ?? 0));
  return [x, y];
};

export const normalizeStreamOrderingMode = (mode: unknown): StreamOrderingMode => {
  const normalized = String(mode ?? 'none')
    .trim()
    .toLowerCase()
    .replaceAll('-', '_')
    .replaceAll(' ', '_');
  return (STREAM_ORDERING_MODE_VALUES as readonly string[]).includes(normalized)
    ? (normalized as StreamOrderingMode)
    : 'none';
};

function calibrationPipelineSelected(value: unknown, calibrationModePipelineUuid: string): boolean {
  return normalizePipelineUuid(value) === normalizePipelineUuid(calibrationModePipelineUuid);
}

export const guidedModeFromManifest = (
  manifest: unknown,
  calibrationModePipelineUuid: string
): boolean | null => {
  if (!manifest || typeof manifest !== 'object') return null;
  const record = manifest as Record<string, unknown>;
  if (
    calibrationPipelineSelected(record.active_pipeline_id, calibrationModePipelineUuid) ||
    calibrationPipelineSelected(record.activePipelineId, calibrationModePipelineUuid) ||
    calibrationPipelineSelected(record.pipeline_id, calibrationModePipelineUuid) ||
    calibrationPipelineSelected(record.pipelineId, calibrationModePipelineUuid)
  ) {
    return true;
  }
  const pipelines = record.pipelines;
  if (Array.isArray(pipelines)) {
    for (const entry of pipelines) {
      if (!entry || typeof entry !== 'object') continue;
      const pipelineRecord = entry as Record<string, unknown>;
      if (calibrationPipelineSelected(pipelineRecord.pipeline_id ?? pipelineRecord.pipelineId ?? pipelineRecord.id, calibrationModePipelineUuid)) {
        return true;
      }
    }
  }
  const layout = record.pipeline_layout ?? record.pipelineLayout;
  if (layout && typeof layout === 'object') {
    const slots = (layout as Record<string, unknown>).slots;
    if (Array.isArray(slots)) {
      for (const slot of slots) {
        if (!slot || typeof slot !== 'object') continue;
        const slotRecord = slot as Record<string, unknown>;
        if (calibrationPipelineSelected(slotRecord.pipeline_id ?? slotRecord.pipelineId, calibrationModePipelineUuid)) {
          return true;
        }
      }
    }
  }
  return false;
};
