import type { LocalizationMarker } from '$lib';
import type { LocalizationDetectionPose, LocalizationPoseSpace, LocalizationSolverOutputs } from './localizationConfig';
import type { LocalizationPipelineSource } from './pipelineSources';

export function toNumber(value: unknown, fallback: number): number {
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  if (typeof value === 'string') {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return parsed;
  }
  return fallback;
}

export function normalizeMarkerArray(payload: unknown): unknown[] {
  if (Array.isArray(payload)) return payload;
  if (payload && typeof payload === 'object' && 'detections' in (payload as Record<string, unknown>)) {
    const detections = (payload as Record<string, unknown>).detections;
    return Array.isArray(detections) ? detections : [];
  }
  return [];
}

export function parseMarker(entry: unknown): LocalizationMarker | null {
  if (!entry || typeof entry !== 'object') return null;
  const record = entry as Record<string, unknown>;

  const translation = record.translation;
  if (!translation || typeof translation !== 'object') return null;
  const t = translation as { x?: unknown; y?: unknown; z?: unknown };
  const x = toNumber(t.x, 0);
  const y = toNumber(t.y, 0);
  const z = toNumber(t.z, 0);

  let yaw = 0;
  let quaternion: { x: number; y: number; z: number; w: number } | undefined;
  const rotation = record.rotation;
  if (rotation && typeof rotation === 'object') {
    const rotationObj = rotation as Record<string, unknown>;
    yaw = toNumber(rotationObj.yaw, 0);
    const quat = rotationObj.quaternion;
    if (quat && typeof quat === 'object') {
      const q = quat as Record<string, unknown>;
      const qx = toNumber(q.x, NaN);
      const qy = toNumber(q.y, NaN);
      const qz = toNumber(q.z, NaN);
      const qw = toNumber(q.w, NaN);
      if ([qx, qy, qz, qw].every((v) => Number.isFinite(v))) {
        quaternion = { x: qx, y: qy, z: qz, w: qw };
      }
    }
  }

  const tagId = record.id ?? record.tagId ?? record.marker_id ?? record.markerId;
  const tagSize = toNumber(record.tagSize ?? record.tag_size ?? record.tag_size_m, 0);
  const codeRotation = toNumber(record.codeRotation ?? record.code_rotation, NaN);
  const bits = record.bits;
  let tagBits: { width: number; border: number; rows: string[] } | undefined;
  if (bits && typeof bits === 'object') {
    const b = bits as Record<string, unknown>;
    const width = toNumber(b.width, NaN);
    const border = toNumber(b.border, NaN);
    const rows = Array.isArray(b.rows) ? b.rows.filter((row): row is string => typeof row === 'string') : [];
    if (Number.isFinite(width) && width > 0 && Number.isFinite(border) && border >= 0 && rows.length === width) {
      tagBits = { width, border, rows };
    }
  }

  const stableId = typeof tagId === 'number' || typeof tagId === 'string' ? String(tagId) : 'unknown';
  return {
    id: `tag-${stableId}`,
    label: typeof tagId === 'number' || typeof tagId === 'string' ? `Tag ${tagId}` : 'Tag',
    position: [x, y, z],
    heading: yaw,
    quaternion,
    targetType: 'aruco-plane',
    status: 'tracking',
    color: '#38bdf8',
    tagId: typeof tagId === 'number' || typeof tagId === 'string' ? tagId : undefined,
    tagSize: tagSize > 0 ? tagSize : undefined,
    tagBits,
    codeRotation: Number.isFinite(codeRotation) ? codeRotation : undefined
  };
}

export function markersFromSample(payload: unknown): LocalizationMarker[] {
  return normalizeMarkerArray(payload)
    .map((entry) => parseMarker(entry))
    .filter((marker): marker is LocalizationMarker => Boolean(marker));
}

export function detectionPosesForSpace(
  outputs: LocalizationSolverOutputs | null,
  space: LocalizationPoseSpace
): LocalizationDetectionPose[] {
  if (!outputs) return [];
  switch (space) {
    case 'tag_in_camera':
      return outputs.tagInCamera ?? [];
    case 'camera_in_tag':
      return outputs.cameraInTag ?? [];
    case 'tag_in_robot':
      return outputs.tagInRobot ?? [];
    case 'robot_in_tag':
      return outputs.robotInTag ?? [];
    case 'camera_in_field':
    case 'robot_in_field':
      return [];
    default:
      return [];
  }
}

export function markersFromDetections(options: {
  detections: (LocalizationDetectionPose & { color?: string | null; profileId?: string | null })[];
  sources: LocalizationPipelineSource[];
  selectedSources: LocalizationPipelineSource[];
  colors: readonly string[];
}): LocalizationMarker[] {
  const { detections, sources, selectedSources, colors } = options;
  if (detections.length === 0) return [];
  if (selectedSources.length === 0) return [];

  // `selectedSources` is the authoritative visibility filter for the viewer.
  // If a source is disabled/removed in the UI, its detections must not render.
  const sourceIndex = new Map<string, number>(selectedSources.map((source, index) => [source.id, index]));
  const selectedIds = new Set<string>(sourceIndex.keys());
  return detections
    .filter((det) => selectedIds.has(det.sourceId))
    .map((det) => {
    const source = sources.find((candidate) => candidate.id === det.sourceId) ?? null;
    const index = sourceIndex.get(det.sourceId) ?? 0;
    const color = det.color ?? colors[index % colors.length] ?? '#38bdf8';
    const pose = det.pose;
    const label = source?.streamLabel ? `${source.streamLabel} · Tag ${det.tagId}` : `Tag ${det.tagId}`;
    const markerId = det.profileId ? `${det.profileId}:${det.sourceId}:${det.tagId}` : `${det.sourceId}:${det.tagId}`;
    return {
      id: markerId,
      label,
      targetType: 'aruco-plane',
      tagId: det.tagId,
      tagSize: det.tagSize ?? undefined,
      tagBits: det.tagBits ?? undefined,
      codeRotation: det.codeRotation ?? undefined,
      position: [pose.translation.x, pose.translation.y, pose.translation.z],
      quaternion: pose.rotation.quaternion,
      color,
      source: {
        id: source?.id ?? det.sourceId,
        streamId: source?.streamId,
        outputKey: source?.outputKey,
        streamLabel: source?.streamLabel,
        cameraUid: source?.cameraUid ?? det.cameraUid,
        cameraPath: source?.cameraPath,
        pipelineId: source?.pipelineId,
        pipelineLabel: source?.pipelineLabel,
        sourceIndex: index,
        profileId: det.profileId ?? undefined
      }
    } as LocalizationMarker;
  });
}

export function looksLikePoseOutputKey(outputKey: string): boolean {
  const key = outputKey.toLowerCase();
  return (
    key.includes('pose') ||
    key.includes('detect') ||
    key.includes('imu') ||
    key.includes('odom') ||
    key.includes('tag_in_') ||
    key.includes('camera_in_') ||
    key.includes('robot_in_') ||
    key.startsWith('solver:')
  );
}

export function isPoseSample(payload: unknown): boolean {
  if (Array.isArray(payload)) {
    const first = payload[0];
    if (!first || typeof first !== 'object') return false;
    const record = first as Record<string, unknown>;
    return typeof record.id === 'number' && Array.isArray(record.corners);
  }
  if (!payload || typeof payload !== 'object') return false;
  if (markersFromSample(payload).length > 0) return true;
  const record = payload as Record<string, unknown>;
  const poseCandidate = record.pose && typeof record.pose === 'object' ? (record.pose as Record<string, unknown>) : record;
  const translation = poseCandidate.translation;
  const rotation = poseCandidate.rotation ?? poseCandidate.orientation;
  if (translation && typeof translation === 'object') {
    const t = translation as Record<string, unknown>;
    if (typeof t.x === 'number' && typeof t.y === 'number' && typeof t.z === 'number') {
      return true;
    }
  }
  if (rotation && typeof rotation === 'object') {
    const r = rotation as Record<string, unknown>;
    if (typeof r.yaw === 'number' || typeof r.roll === 'number' || typeof r.pitch === 'number') {
      return true;
    }
    if (r.quaternion && typeof r.quaternion === 'object') {
      return true;
    }
  }
  const detections = record.detections;
  if (!Array.isArray(detections)) return false;
  if (typeof record.poseMethod === 'string') return true;
  const stats = record.stats;
  if (stats && typeof stats === 'object') {
    const statsRecord = stats as Record<string, unknown>;
    if ('outputDetections' in statsRecord || 'detections' in statsRecord) {
      return true;
    }
  }
  return false;
}
