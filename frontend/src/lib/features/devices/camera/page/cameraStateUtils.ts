import type { StreamInfo, StreamManifest } from '$lib/api/httpClient';
import type { CalibrationSolveResult } from './cameraCalibrationStore.svelte';

type CalibrationParams = {
  fx: number;
  fy: number;
  cx: number;
  cy: number;
  k1: number;
  k2: number;
  p1: number;
  p2: number;
  k3: number;
  undistortIters: number;
  lensModel?: 'pinhole' | 'fisheye';
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
  const identity = streamInfo?.manifest?.identity;
  const identityFallback =
    // Current backend shape: `DeviceIdentity { id, alias, hardware_id }`.
    (identity as any)?.hardware_id?.trim?.() ||
    (identity as any)?.alias?.trim?.() ||
    identity?.display?.trim() ||
    (Array.isArray(identity?.keys) ? identity.keys.find((value) => value?.trim?.())?.trim() : '') ||
    '';
  return identityFallback || fallback;
}

export function normalizeCalibrationSolveResult(value: any): CalibrationSolveResult | null {
  if (!value || typeof value !== 'object') return null;
  const calibration = (value as any).calibration ?? (value as any).camera ?? value;
  if (!calibration || typeof calibration !== 'object') return null;
  const num = (v: any) => (Number.isFinite(Number(v)) ? Number(v) : null);
  const fx = num((calibration as any).fx);
  const fy = num((calibration as any).fy);
  const cx = num((calibration as any).cx);
  const cy = num((calibration as any).cy);
  const k1 = num((calibration as any).k1);
  const k2 = num((calibration as any).k2);
  const p1 = num((calibration as any).p1);
  const p2 = num((calibration as any).p2);
  const k3 = num((calibration as any).k3);
  if ([fx, fy, cx, cy, k1, k2, p1, p2, k3].some((v) => v === null)) return null;
  const lensModelRaw = (calibration as any).lensModel ?? (calibration as any).lens_model ?? null;
  const lensModel = lensModelRaw === 'fisheye' || lensModelRaw === 'pinhole' ? lensModelRaw : undefined;
  const undistortIters = num((calibration as any).undistortIters) ?? 5;
  const viewsUsed = num((value as any).viewsUsed ?? (value as any).views_used) ?? undefined;
  const pointsUsed = num((value as any).pointsUsed ?? (value as any).points_used) ?? undefined;
  const warnings = Array.isArray((value as any).warnings)
    ? (value as any).warnings.map((warn: any) => String(warn)).filter((warn: string) => warn.length)
    : undefined;
  const rawDebugViews = (value as any).debugViews ?? (value as any).debug_views;
  const debugViews = Array.isArray(rawDebugViews)
    ? rawDebugViews
        .map((view: any) => {
          if (!view || typeof view !== 'object') return null;
          const image = typeof view.image === 'string' ? view.image : '';
          if (!image) return null;
          const tagsDetected = num(view.tagsDetected ?? view.tags_detected) ?? 0;
          const pointsDetected = num(view.pointsDetected ?? view.points_detected) ?? 0;
          const used = Boolean(view.used);
          const coverageRatio = num(view.coverageRatio ?? view.coverage_ratio) ?? 0;
          const overlay =
            typeof view.overlay === 'string' || view.overlay === null ? (view.overlay as string | null) : undefined;
          const rawTagsDetected = num(view.rawTagsDetected ?? view.raw_tags_detected) ?? undefined;
          const rawIds = Array.isArray(view.rawIds ?? view.raw_ids)
            ? (view.rawIds ?? view.raw_ids).map((id: any) => Number(id)).filter((id: number) => Number.isFinite(id))
            : undefined;
          const rawDuplicateIds = Array.isArray(view.rawDuplicateIds ?? view.raw_duplicate_ids)
            ? (view.rawDuplicateIds ?? view.raw_duplicate_ids).map((id: any) => Number(id)).filter((id: number) => Number.isFinite(id))
            : undefined;
          const rawOutOfRangeIds = Array.isArray(view.rawOutOfRangeIds ?? view.raw_out_of_range_ids)
            ? (view.rawOutOfRangeIds ?? view.raw_out_of_range_ids).map((id: any) => Number(id)).filter((id: number) => Number.isFinite(id))
            : undefined;
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
        .filter(Boolean)
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
    reprojectionErrorPx: num((value as any).reprojectionErrorPx ?? (value as any).reprojection_error_px) ?? undefined,
    viewsUsed,
    pointsUsed,
    warnings,
    debugViews
  };
}

export function normalizeCalibrationParams(value: any): CalibrationParams | null {
  if (!value || typeof value !== 'object') return null;
  const num = (v: any) => (Number.isFinite(Number(v)) ? Number(v) : null);
  const fx = num((value as any).fx);
  const fy = num((value as any).fy);
  const cx = num((value as any).cx);
  const cy = num((value as any).cy);
  const k1 = num((value as any).k1);
  const k2 = num((value as any).k2);
  const p1 = num((value as any).p1);
  const p2 = num((value as any).p2);
  const k3 = num((value as any).k3);
  if ([fx, fy, cx, cy, k1, k2, p1, p2, k3].some((v) => v === null)) return null;
  const lensModelRaw = (value as any).lensModel ?? (value as any).lens_model ?? null;
  const lensModel = lensModelRaw === 'fisheye' || lensModelRaw === 'pinhole' ? lensModelRaw : undefined;
  const undistortIters = num((value as any).undistortIters) ?? 5;
  return { fx, fy, cx, cy, k1, k2, p1, p2, k3, undistortIters, lensModel };
}

export function extractCurrentCalibrationParams(manifestState: StreamManifest | null, stream: StreamInfo | null): CalibrationParams | null {
  const manifest = (manifestState as any) ?? (stream as any)?.manifest ?? null;
  const calibration =
    manifest?.calibration ??
    manifest?.camera?.calibration ??
    manifest?.camera?.intrinsics ??
    manifest?.intrinsics ??
    null;
  return normalizeCalibrationParams(calibration);
}

export function parseMetadataValue(value: string | null | undefined): any {
  if (value === null || value === undefined) return null;
  const raw = String(value).trim();
  if (!raw.length) return null;
  try {
    return JSON.parse(raw);
  } catch {
    return raw;
  }
}
