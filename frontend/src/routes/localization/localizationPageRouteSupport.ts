import { realtimeUpdateMatchesKind, type RealtimeUpdateEvent } from '$lib/api/realtimeUpdates';
import type { RigCameraInfo, RobotDimensions } from '$lib/types/rig';
import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
import type {
  LocalizationConfig,
  LocalizationCustomFieldOrigin,
  LocalizationDetectionPose,
  LocalizationFieldOriginConfig,
  LocalizationPoseSpace,
  LocalizationProfile,
  LocalizationSolverConfig,
  LocalizationSolverMode,
  LocalizationSolverOutputs,
  LocalizationSolveResponse,
  LocalizationSourceConfig
} from '$lib/features/localization/localizationConfig';
import {
  composeTransforms,
  quaternionToEulerDegreesXYZ,
  type PoseQuaternion,
  type PoseTransform
} from '$lib/features/localization/poseMath';
import { profileColorForId } from '$lib/features/localization/utils';
import { collectKeyVariants, rigCameraKeys, rigPoseToViewerTransform } from './localizationCameraPovUtils';

export type FeedStatus = 'idle' | 'connecting' | 'live' | 'error';

export type LocalizationCoordinateSpace = LocalizationPoseSpace;

export type LocalizationProfileTimingRow = {
  profileId: string;
  label: string;
  active: boolean;
  visible: boolean;
  solverMs: number | null;
  engineMs: number | null;
  totalMs: number | null;
  sourceFetchMs: number | null;
  sourceParseMs: number | null;
  cacheHit: boolean;
};

export type RigLayoutViewState = {
  layout: { robot: RobotDimensions; cameras: RigCameraInfo[] };
  loading: boolean;
  error: string | null;
  initialized: boolean;
};

export type ImuRotationSample = {
  sourceId: string;
  sourceLabel: string;
  outputKey: string;
  roll: number;
  pitch: number;
  yaw: number;
  quaternion: PoseQuaternion | null;
  translation: { x: number; y: number; z: number } | null;
  sampleTimestampMs: number | null;
  receivedAtMs: number;
};

export const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

export const UUID_LIKE_RE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export const DEFAULT_LOCALIZATION_CALIBRATION = {
  fx: 608.2823560574243,
  fy: 610.5489059894065,
  cx: 638.8193727530946,
  cy: 397.5372092494376,
  k1: -0.04766182849174875,
  k2: -0.023041409826196887,
  k3: -0.012694098622574222,
  p1: -0.0027334920956789644,
  p2: 0,
  undistortIters: 5,
  lensModel: 'fisheye'
} as const;

export const LOCAL_TAG_POSE_LINGER_MS = 5000;
export const LOCAL_TAG_POSE_REFRESH_MS = 250;
export const FIELD_POSE_LINGER_MS = 5000;
export const FIELD_POSE_REFRESH_MS = 250;
export const LIVE_UPDATES_REFRESH_DEBOUNCE_MS = 400;
export const LIVE_SOURCES_REFRESH_MIN_INTERVAL_MS = 5_000;
export const BUMPER_ID = '0000';

export const buildSourceConfigFromSource = (
  source: LocalizationPipelineSource,
  existing: LocalizationSourceConfig | null = null
): LocalizationSourceConfig => ({
  id: source.id,
  streamId: source.streamId,
  outputKey: source.outputKey,
  cameraUid: source.cameraUid,
  poseSpace: existing?.poseSpace ?? null,
  inputKey: existing?.inputKey ?? null,
  enabled: true,
  weight: existing?.weight ?? 1
});

export function computeViewProfiles(profiles: LocalizationProfile[]): LocalizationProfile[] {
  if (profiles.length === 0) return [];
  return profiles.filter((profile) => profile.enabled !== false && profile.viewEnabled === true);
}

export function sourceStreamOutputKey(streamId: string | null | undefined, outputKey: string | null | undefined): string {
  const stream = String(streamId ?? '').trim();
  const output = String(outputKey ?? '').trim();
  if (!stream || !output) return '';
  return `${stream}::${output}`;
}

export function computeViewOverlaySources(
  profiles: LocalizationProfile[],
  sources: LocalizationPipelineSource[]
): LocalizationPipelineSource[] {
  const viewProfiles = computeViewProfiles(profiles);
  const enabledIds = new Set<string>();
  const enabledStreamOutputs = new Set<string>();
  for (const profile of viewProfiles) {
    for (const source of profile.sources ?? []) {
      if (!source.enabled) continue;
      const id = String(source.id ?? '').trim();
      if (id) enabledIds.add(id);
      const streamOutput = sourceStreamOutputKey(source.streamId, source.outputKey);
      if (streamOutput) enabledStreamOutputs.add(streamOutput);
    }
  }
  return sources.filter((source) => {
    if (enabledIds.has(source.id)) return true;
    const streamOutput = sourceStreamOutputKey(source.streamId, source.outputKey);
    return streamOutput ? enabledStreamOutputs.has(streamOutput) : false;
  });
}

export function solverPoseToTransform(
  pose: { translation: { x: number; y: number; z: number }; rotation: { quaternion: PoseQuaternion } } | null | undefined
): PoseTransform | null {
  if (!pose) return null;
  return {
    position: [pose.translation.x, pose.translation.y, pose.translation.z],
    quaternion: pose.rotation.quaternion
  };
}

export function detectionPoseToTransform(detection: LocalizationDetectionPose): PoseTransform {
  const pose = detection.pose;
  return {
    position: [pose.translation.x, pose.translation.y, pose.translation.z],
    quaternion: pose.rotation.quaternion
  };
}

export function detectionWithTransform(
  detection: LocalizationDetectionPose,
  transform: PoseTransform
): LocalizationDetectionPose {
  const euler = quaternionToEulerDegreesXYZ(transform.quaternion);
  return {
    ...detection,
    pose: {
      translation: {
        x: transform.position[0],
        y: transform.position[1],
        z: transform.position[2]
      },
      rotation: {
        roll: euler.roll,
        pitch: euler.pitch,
        yaw: euler.yaw,
        quaternion: transform.quaternion
      }
    }
  };
}

export function detectionKey(detection: LocalizationDetectionPose): string {
  return `${detection.sourceId}|${detection.cameraUid}|${detection.tagId}`;
}

function findRigCameraForDetection(
  detection: LocalizationDetectionPose,
  sourceById: Map<string, LocalizationPipelineSource>,
  rigCameras: RigCameraInfo[],
  rigCameraForSource: (source: LocalizationPipelineSource | null) => RigCameraInfo | null
): RigCameraInfo | null {
  const source = sourceById.get(detection.sourceId) ?? null;
  if (source) {
    const direct = rigCameraForSource(source);
    if (direct) return direct;
  }
  const keySet = new Set(
    collectKeyVariants([
      detection.cameraUid,
      detection.sourceId,
      source?.cameraUid,
      source?.streamId,
      source?.cameraPath,
      ...(source?.cameraKeys ?? [])
    ])
  );
  if (!keySet.size) return null;
  for (const camera of rigCameras) {
    if (rigCameraKeys(camera).some((key) => keySet.has(key))) {
      return camera;
    }
  }
  return null;
}

function detectionToFieldTransform(
  detection: LocalizationDetectionPose,
  sourceById: Map<string, LocalizationPipelineSource>,
  fieldFromRobot: PoseTransform | null,
  fieldFromCameraByKey: Record<string, PoseTransform>,
  rigCameras: RigCameraInfo[],
  rigCameraForSource: (source: LocalizationPipelineSource | null) => RigCameraInfo | null
): PoseTransform | null {
  const source = sourceById.get(detection.sourceId) ?? null;
  const keys = collectKeyVariants([
    detection.sourceId,
    detection.cameraUid,
    source?.id,
    source?.cameraUid,
    source?.streamId,
    source?.cameraPath,
    ...(source?.cameraKeys ?? [])
  ]);
  for (const key of keys) {
    const transform = fieldFromCameraByKey[key];
    if (transform) {
      return composeTransforms(transform, detectionPoseToTransform(detection));
    }
  }

  if (!fieldFromRobot) return null;
  const rigCamera = findRigCameraForDetection(detection, sourceById, rigCameras, rigCameraForSource);
  const robotFromCamera = rigPoseToViewerTransform(rigCamera?.pose ?? null);
  if (!robotFromCamera) return null;
  const fieldFromCamera = composeTransforms(fieldFromRobot, robotFromCamera);
  return composeTransforms(fieldFromCamera, detectionPoseToTransform(detection));
}

export function fieldDetectionsForOutputs(
  outputs: LocalizationSolverOutputs | null,
  sourceById: Map<string, LocalizationPipelineSource>,
  rigCameras: RigCameraInfo[],
  rigCameraForSource: (source: LocalizationPipelineSource | null) => RigCameraInfo | null
): LocalizationDetectionPose[] {
  if (!outputs) return [];

  const fieldFromRobot = solverPoseToTransform(outputs.robotInField?.pose ?? null);
  const fieldFromCameraByKey: Record<string, PoseTransform> = {};
  for (const entry of outputs.cameraInField ?? []) {
    const transform = solverPoseToTransform(entry.pose);
    if (!transform) continue;
    const source = sourceById.get(entry.sourceId) ?? null;
    const keys = collectKeyVariants([
      entry.sourceId,
      entry.cameraUid,
      source?.id,
      source?.cameraUid,
      source?.streamId,
      source?.cameraPath,
      ...(source?.cameraKeys ?? [])
    ]);
    for (const key of keys) {
      fieldFromCameraByKey[key] = transform;
    }
  }

  const out: LocalizationDetectionPose[] = [];
  const seen = new Set<string>();
  for (const detection of outputs.tagInCamera ?? []) {
    const fieldFromTag = detectionToFieldTransform(
      detection,
      sourceById,
      fieldFromRobot,
      fieldFromCameraByKey,
      rigCameras,
      rigCameraForSource
    );
    if (!fieldFromTag) continue;
    out.push(detectionWithTransform(detection, fieldFromTag));
    seen.add(detectionKey(detection));
  }

  if (fieldFromRobot) {
    for (const detection of outputs.tagInRobot ?? []) {
      const key = detectionKey(detection);
      if (seen.has(key)) continue;
      const fieldFromTag = composeTransforms(fieldFromRobot, detectionPoseToTransform(detection));
      out.push(detectionWithTransform(detection, fieldFromTag));
    }
  }

  return out;
}

export function shouldApplyLiveUpdate(event: RealtimeUpdateEvent): boolean {
  if (
    event.path.startsWith('/v1/localization') ||
    event.path.startsWith('/v1/streams') ||
    event.path.startsWith('/v1/pipelines') ||
    event.path.startsWith('/v1/media') ||
    event.path.startsWith('/v1/device')
  ) {
    return true;
  }
  if (realtimeUpdateMatchesKind(event, 'api')) {
    return false;
  }
  return (
    realtimeUpdateMatchesKind(event, 'localization') ||
    realtimeUpdateMatchesKind(event, 'streams') ||
    realtimeUpdateMatchesKind(event, 'pipelines') ||
    realtimeUpdateMatchesKind(event, 'media') ||
    realtimeUpdateMatchesKind(event, 'imu') ||
    realtimeUpdateMatchesKind(event, 'device') ||
    realtimeUpdateMatchesKind(event, 'settings')
  );
}

export function shouldRefreshSourcesForLiveUpdate(event: RealtimeUpdateEvent): boolean {
  const path = event.path;
  if (path.startsWith('/v1/localization/solve')) {
    return false;
  }
  if (
    path.startsWith('/v1/streams') ||
    path.startsWith('/v1/pipelines') ||
    path.startsWith('/v1/localization/config') ||
    path.startsWith('/v1/localization/profile') ||
    path.startsWith('/v1/localization/source')
  ) {
    return true;
  }
  if (realtimeUpdateMatchesKind(event, 'streams') || realtimeUpdateMatchesKind(event, 'pipelines')) {
    return true;
  }
  return false;
}

export function computeActiveSolverConfig(
  profile: LocalizationProfile | null,
  requestedSolverId: string
): LocalizationSolverConfig | null {
  const solvers = profile?.solvers ?? [];
  if (solvers.length === 0) return null;
  const requested = requestedSolverId.trim();
  if (requested) {
    const match = solvers.find((solver) => solver.id === requested);
    if (match) return match;
  }
  return solvers[0] ?? null;
}

export function computeProfileIndexById(profiles: LocalizationProfile[]): Map<string, number> {
  const map = new Map<string, number>();
  profiles.forEach((profile, index) => {
    map.set(profile.id, index);
  });
  return map;
}

export function computeActiveProfileColor(
  activeProfile: LocalizationProfile | null,
  profiles: LocalizationProfile[],
  profileIndexById: Map<string, number>,
  profileColors: readonly string[]
): string {
  if (!activeProfile) return profileColors[0] ?? '#38bdf8';
  return profileColorForId(activeProfile.id, profiles, profileIndexById, profileColors);
}
