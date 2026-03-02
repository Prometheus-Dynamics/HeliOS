import type {
  LocalizationPoseSpace,
  LocalizationProfile,
  LocalizationSourceConfig
} from './localizationConfig';
import type { LocalizationPipelineSource } from './pipelineSources';

type LocalizationSourceLike = {
  id?: string | null;
  streamId?: string | null;
  cameraUid?: string | null;
};

const MEDIA_IMU_STREAM_RE =
  /^external:media-imu-([0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12})$/i;
const MEDIA_IMU_ID_RE =
  /^external:media-imu-([0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}):/i;
const MEDIA_IMU_CAMERA_RE =
  /^media-imu:([0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12})$/i;

export const SOURCE_COLORS = ['#38bdf8', '#f97316', '#a855f7', '#22c55e', '#e11d48', '#facc15', '#0ea5e9', '#10b981'] as const;
export const PROFILE_COLORS = ['#f97316', '#38bdf8', '#22c55e', '#a855f7', '#e11d48', '#facc15', '#0ea5e9', '#10b981'] as const;
export const POSE_SPACE_OPTIONS: LocalizationPoseSpace[] = [
  'tag_in_camera',
  'tag_in_robot',
  'camera_in_field',
  'robot_in_field'
];
// Pose spaces users can explicitly request from the solver.
// (We intentionally do not expose *_in_tag spaces right now; they require different visualization.)
export const SOLVE_POSE_SPACES: LocalizationPoseSpace[] = ['tag_in_camera', 'tag_in_robot', 'camera_in_field', 'robot_in_field'];
export const DERIVED_POSE_SPACES: Record<LocalizationPoseSpace, LocalizationPoseSpace[]> = {
  tag_in_camera: [],
  // camera_in_tag intentionally unsupported in the UI for now.
  camera_in_tag: [],
  tag_in_robot: [],
  // robot_in_tag intentionally unsupported in the UI for now.
  robot_in_tag: [],
  robot_in_field: [],
  camera_in_field: [],
};
export const KNOWN_POSE_SPACES = new Set(
  SOLVE_POSE_SPACES.concat(Object.values(DERIVED_POSE_SPACES).flat())
);
export const SOLVE_POSE_LABELS: Record<LocalizationPoseSpace, string> = {
  tag_in_camera: 'tag in camera',
  tag_in_robot: 'tag in robot',
  robot_in_field: 'robot in field',
  camera_in_field: 'camera in field',
  camera_in_tag: 'camera in tag',
  robot_in_tag: 'robot in tag',
};

export function normalizeInputKey(raw: string | null | undefined): string | null {
  if (typeof raw !== 'string') return null;
  const trimmed = raw.trim();
  return trimmed.length > 0 ? trimmed : null;
}

export function poseSpaceLabel(space: LocalizationPoseSpace): string {
  return SOLVE_POSE_LABELS[space] ?? space;
}

export function profileColorForId(
  profileId: string,
  profiles: LocalizationProfile[],
  profileIndexById: Map<string, number>,
  colors: readonly string[] = PROFILE_COLORS
): string {
  const profile = profiles.find((entry) => entry.id === profileId) ?? null;
  const index = profileIndexById.get(profileId) ?? 0;
  return profile?.color ?? colors[index % colors.length] ?? '#38bdf8';
}

export function isSourceCalibrated(source: LocalizationPipelineSource, calibratedIds: Set<string>): boolean {
  const streamId = String(source.streamId ?? '').trim();
  if (streamId.startsWith('external:')) {
    return true;
  }
  const keyCandidates = Array.isArray(source.cameraKeys)
    ? source.cameraKeys.filter((value): value is string => typeof value === 'string' && value.trim().length > 0)
    : [];
  const rawCandidates = [source.cameraUid, source.streamId, source.cameraPath, ...keyCandidates].filter(
    (value): value is string => typeof value === 'string' && value.trim().length > 0
  );
  const candidates: string[] = [];
  for (const value of rawCandidates) {
    const trimmed = value.trim();
    if (!trimmed) continue;
    candidates.push(trimmed);
    if (trimmed.startsWith('device:')) {
      candidates.push(trimmed.slice('device:'.length));
    } else if (trimmed.startsWith('stream:')) {
      candidates.push(trimmed.slice('stream:'.length));
    }
  }
  return candidates.some((value) => calibratedIds.has(value));
}

export function buildSourceConfig(
  source: LocalizationPipelineSource,
  enabled: boolean,
  existing?: LocalizationSourceConfig | null
): LocalizationSourceConfig {
  return {
    id: source.id,
    streamId: source.streamId,
    outputKey: source.outputKey,
    cameraUid: source.cameraUid,
    poseSpace: existing?.poseSpace ?? null,
    inputKey: normalizeInputKey(existing?.inputKey ?? null),
    enabled,
    weight: existing?.weight ?? 1
  };
}

export function updateProfileSources(
  profile: LocalizationProfile,
  enabledIds: string[],
  availableSources: LocalizationPipelineSource[]
): LocalizationProfile {
  const sourceMap = new Map(profile.sources.map((entry) => [entry.id, entry]));
  const nextSources: LocalizationSourceConfig[] = [];

  for (const source of availableSources) {
    const enabled = enabledIds.includes(source.id);
    if (!enabled && !sourceMap.has(source.id)) {
      continue;
    }
    nextSources.push(buildSourceConfig(source, enabled, sourceMap.get(source.id)));
    sourceMap.delete(source.id);
  }

  for (const entry of sourceMap.values()) {
    nextSources.push({ ...entry, enabled: enabledIds.includes(entry.id) });
  }

  return { ...profile, sources: nextSources };
}

export function updateProfilePipelineTemplate(profile: LocalizationProfile, templateId: string | null): LocalizationProfile {
  return { ...profile, pipelineTemplateId: templateId };
}

export function updateProfileSourceInputKey(
  profile: LocalizationProfile,
  sourceId: string,
  inputKey: string | null
): LocalizationProfile {
  const nextSources = profile.sources.map((source) =>
    source.id === sourceId ? { ...source, inputKey } : source
  );
  return { ...profile, sources: nextSources };
}

export function updateSolverOutputSpaces(
  profile: LocalizationProfile,
  solverId: string,
  outputSpaces: LocalizationPoseSpace[]
): LocalizationProfile {
  const solvers = profile.solvers.map((solver) =>
    solver.id === solverId ? { ...solver, outputSpaces } : solver
  );
  return { ...profile, solvers };
}

export function buildOutputSpacesFromSolve(
  outputs: LocalizationPoseSpace[],
  solveSpaces: LocalizationPoseSpace[],
  options: { knownPoseSpaces?: Set<LocalizationPoseSpace>; derivedPoseSpaces?: Record<LocalizationPoseSpace, LocalizationPoseSpace[]> } = {}
): LocalizationPoseSpace[] {
  const known = options.knownPoseSpaces ?? KNOWN_POSE_SPACES;
  const derived = options.derivedPoseSpaces ?? DERIVED_POSE_SPACES;
  const next = new Set<LocalizationPoseSpace>();
  for (const space of outputs) {
    if (!known.has(space)) {
      next.add(space);
    }
  }
  for (const space of solveSpaces) {
    next.add(space);
    for (const derivedSpace of derived[space] ?? []) {
      next.add(derivedSpace);
    }
  }
  return Array.from(next.values());
}

export function mediaImuParentStreamIdForSource(source: LocalizationSourceLike | null | undefined): string | null {
  if (!source) return null;
  const streamId = String(source.streamId ?? '').trim();
  const id = String(source.id ?? '').trim();
  const cameraUid = String(source.cameraUid ?? '').trim();

  const byStream = streamId.match(MEDIA_IMU_STREAM_RE)?.[1];
  if (byStream) return byStream;

  const byId = id.match(MEDIA_IMU_ID_RE)?.[1];
  if (byId) return byId;

  const byCamera = cameraUid.match(MEDIA_IMU_CAMERA_RE)?.[1];
  if (byCamera) return byCamera;

  return null;
}

export function cameraKeyForSource(source: LocalizationSourceLike | null | undefined): string | null {
  if (!source) return null;
  const parentStreamId = mediaImuParentStreamIdForSource(source);
  if (parentStreamId) return parentStreamId;
  const cameraUid = String(source.cameraUid ?? '').trim();
  if (cameraUid) return cameraUid;
  const streamId = String(source.streamId ?? '').trim();
  return streamId || null;
}
