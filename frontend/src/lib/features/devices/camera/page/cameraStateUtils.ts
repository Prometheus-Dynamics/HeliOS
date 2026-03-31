import type { StreamInfo, StreamManifest } from '$lib/api/httpClient';
import type { CalibrationParams, CalibrationResult } from '$lib/features/devices/camera/cameraCalibrationTypes';

type SolveDebugView = NonNullable<CalibrationResult['debugViews']>[number];

const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

const asNumber = (value: unknown): number | null => {
  const numeric = Number(value);
  return Number.isFinite(numeric) ? numeric : null;
};

const asFiniteNumberArray = (value: unknown): number[] | undefined => {
  if (!Array.isArray(value)) return undefined;
  const normalized = value
    .map((entry) => Number(entry))
    .filter((entry): entry is number => Number.isFinite(entry));
  return normalized.length ? normalized : undefined;
};

export function resolvePoseCameraRef(streamInfo: StreamInfo | null, fallback: string): string {
  const keys = streamInfo?.manifest?.capture?.device_keys;
  if (Array.isArray(keys) && keys.length) {
    const normalized = keys.map((value) => value?.trim()).filter(Boolean) as string[];
    const withSlash = normalized.find((value) => value.includes('/'));
    if (withSlash) return withSlash;
    const withColon = normalized.find((value) => value.includes(':'));
    if (withColon) return withColon;
    const sorted = [...normalized].sort();
    if (sorted.length) return sorted[0];
  }
  const identity = asRecord(streamInfo?.manifest?.identity);
  const identityFallback =
    // Current backend shape: `DeviceIdentity { id, alias, hardware_id }`.
    (typeof identity?.hardware_id === 'string' ? identity.hardware_id.trim() : '') ||
    (typeof identity?.alias === 'string' ? identity.alias.trim() : '') ||
    (typeof identity?.display === 'string' ? identity.display.trim() : '') ||
    (Array.isArray(identity?.keys)
      ? identity.keys.find((value): value is string => typeof value === 'string' && value.trim().length > 0)?.trim()
      : '') ||
    '';
  return identityFallback || fallback;
}

export function normalizeCalibrationSolveResult(value: unknown): CalibrationResult | null {
  const record = asRecord(value);
  if (!record) return null;
  const calibration = asRecord(record.calibration) ?? asRecord(record.camera) ?? record;
  const fx = asNumber(calibration.fx);
  const fy = asNumber(calibration.fy);
  const cx = asNumber(calibration.cx);
  const cy = asNumber(calibration.cy);
  const k1 = asNumber(calibration.k1);
  const k2 = asNumber(calibration.k2);
  const p1 = asNumber(calibration.p1);
  const p2 = asNumber(calibration.p2);
  const k3 = asNumber(calibration.k3);
  if ([fx, fy, cx, cy, k1, k2, p1, p2, k3].some((v) => v === null)) return null;
  const lensModelRaw = calibration.lensModel ?? calibration.lens_model ?? null;
  const lensModel = lensModelRaw === 'fisheye' || lensModelRaw === 'pinhole' ? lensModelRaw : undefined;
  const undistortIters = asNumber(calibration.undistortIters) ?? 5;
  const viewsUsed = asNumber(record.viewsUsed ?? record.views_used) ?? undefined;
  const pointsUsed = asNumber(record.pointsUsed ?? record.points_used) ?? undefined;
  const warnings = Array.isArray(record.warnings)
    ? record.warnings.map((warn) => String(warn)).filter((warn) => warn.length > 0)
    : undefined;
  const rawDebugViews = record.debugViews ?? record.debug_views;
  const debugViews: SolveDebugView[] | undefined = Array.isArray(rawDebugViews)
    ? rawDebugViews
        .map((view): SolveDebugView | null => {
          const viewRecord = asRecord(view);
          if (!viewRecord) return null;
          const image = typeof viewRecord.image === 'string' ? viewRecord.image : '';
          if (!image) return null;
          const tagsDetected = asNumber(viewRecord.tagsDetected ?? viewRecord.tags_detected) ?? 0;
          const pointsDetected = asNumber(viewRecord.pointsDetected ?? viewRecord.points_detected) ?? 0;
          const used = Boolean(viewRecord.used);
          const coverageRatio = asNumber(viewRecord.coverageRatio ?? viewRecord.coverage_ratio) ?? 0;
          const overlay =
            typeof viewRecord.overlay === 'string' || viewRecord.overlay === null
              ? (viewRecord.overlay as string | null)
              : undefined;
          const rawTagsDetected = asNumber(viewRecord.rawTagsDetected ?? viewRecord.raw_tags_detected) ?? undefined;
          const rawIds = asFiniteNumberArray(viewRecord.rawIds ?? viewRecord.raw_ids);
          const rawDuplicateIds = asFiniteNumberArray(viewRecord.rawDuplicateIds ?? viewRecord.raw_duplicate_ids);
          const rawOutOfRangeIds = asFiniteNumberArray(viewRecord.rawOutOfRangeIds ?? viewRecord.raw_out_of_range_ids);
          return {
            image,
            overlay,
            tagsDetected,
            pointsDetected,
            used,
            coverageRatio,
            rawTagsDetected,
            rawIds,
            rawDuplicateIds,
            rawOutOfRangeIds
          };
        })
        .filter((view): view is SolveDebugView => Boolean(view))
    : undefined;
  return {
    calibration: {
      fx,
      fy,
      cx,
      cy,
      k1,
      k2,
      p1,
      p2,
      k3,
      undistortIters,
      lensModel
    },
    reprojectionErrorPx: asNumber(record.reprojectionErrorPx ?? record.reprojection_error_px) ?? undefined,
    viewsUsed,
    pointsUsed,
    warnings,
    debugViews
  };
}

export function normalizeCalibrationParams(value: unknown): CalibrationParams | null {
  const record = asRecord(value);
  if (!record) return null;
  const fx = asNumber(record.fx);
  const fy = asNumber(record.fy);
  const cx = asNumber(record.cx);
  const cy = asNumber(record.cy);
  const k1 = asNumber(record.k1);
  const k2 = asNumber(record.k2);
  const p1 = asNumber(record.p1);
  const p2 = asNumber(record.p2);
  const k3 = asNumber(record.k3);
  if ([fx, fy, cx, cy, k1, k2, p1, p2, k3].some((v) => v === null)) return null;
  const lensModelRaw = record.lensModel ?? record.lens_model ?? null;
  const lensModel = lensModelRaw === 'fisheye' || lensModelRaw === 'pinhole' ? lensModelRaw : undefined;
  const undistortIters = asNumber(record.undistortIters) ?? 5;
  return { fx, fy, cx, cy, k1, k2, p1, p2, k3, undistortIters, lensModel };
}

export function extractCurrentCalibrationParams(manifestState: StreamManifest | null, stream: StreamInfo | null): CalibrationParams | null {
  const manifest = asRecord(manifestState) ?? asRecord(stream?.manifest);
  const camera = asRecord(manifest?.camera);
  const calibration =
    manifest?.calibration ??
    camera?.calibration ??
    camera?.intrinsics ??
    manifest?.intrinsics ??
    null;
  return normalizeCalibrationParams(calibration);
}

export function parseMetadataValue(value: string | null | undefined): unknown {
  if (value === null || value === undefined) return null;
  const raw = String(value).trim();
  if (!raw.length) return null;
  try {
    return JSON.parse(raw);
  } catch {
    return raw;
  }
}
