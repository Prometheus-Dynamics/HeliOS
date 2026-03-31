import {
  composeTransforms,
  identityTransform,
  type PoseQuaternion,
  type PoseTransform,
  type Vec3
} from '$lib/features/localization/poseMath';
import type {
  LocalizationPoseSpace,
  LocalizationProfile,
  LocalizationSolverOutputs
} from '$lib/features/localization/localizationConfig';
import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
import type { RigCameraInfo } from '$lib/types/rig';
import {
  cameraPovOptionId,
  collectKeyVariants,
  intrinsicsEqual,
  rigCameraKeys,
  rigPoseToViewerTransform,
  sourceKeys,
  type CameraPovIntrinsics,
  type CameraPovState
} from './localizationCameraPovUtils';

export type ViewerBaseFrame = 'camera' | 'robot' | 'field';

export type ViewerCameraTransform = { position: Vec3; quaternion?: PoseQuaternion };

export type CameraPovOption = {
  id: string;
  profileId: string;
  sourceId: string;
  groupLabel: string;
  subgroupLabel: string;
  label: string;
  available: boolean;
  ghost: boolean;
};

export type CameraPovOptionBase = Omit<CameraPovOption, 'available' | 'ghost'>;

type CameraIntrinsicsByKey = Record<
  string,
  { undistorted: CameraPovIntrinsics | null; raw: CameraPovIntrinsics | null }
>;

type BuildCameraPovByOptionIdArgs = {
  cameraPovOptionsBase: CameraPovOptionBase[];
  profiles: LocalizationProfile[];
  sources: LocalizationPipelineSource[];
  outputsForProfile: (profile: LocalizationProfile) => LocalizationSolverOutputs | null;
  rigCameras: RigCameraInfo[];
  coordinateSpace: LocalizationPoseSpace;
  cameraIntrinsicsByKey: CameraIntrinsicsByKey;
};

type BuildViewerRenderableCamerasArgs = {
  baseFrame: ViewerBaseFrame;
  viewerCameras: RigCameraInfo[];
  viewerCameraTransforms: Record<string, ViewerCameraTransform> | null;
  viewOverlaySources: LocalizationPipelineSource[];
  selectedSources: LocalizationPipelineSource[];
};

const toPoseTransform = (
  pose:
    | { translation: { x: number; y: number; z: number }; rotation: { quaternion: PoseQuaternion } }
    | null
    | undefined
): PoseTransform | null => {
  if (!pose) return null;
  return {
    position: [pose.translation.x, pose.translation.y, pose.translation.z],
    quaternion: pose.rotation.quaternion
  };
};

const rigCameraForSource = (
  source: LocalizationPipelineSource | null,
  rigCameras: RigCameraInfo[]
): RigCameraInfo | null => {
  if (!source) return null;
  const refs = [source.cameraUid, source.cameraPath, source.streamId, source.streamLabel, source.id]
    .map((value) => (typeof value === 'string' ? value.trim() : ''))
    .filter(Boolean);
  if (!refs.length) return null;

  for (const camera of rigCameras) {
    const candidates = [
      camera.streamId ?? null,
      camera.cameraUid ?? null,
      camera.streamAlias ?? null,
      camera.driverCameraId ?? null,
      camera.hardwareId ?? null,
      camera.uid ?? null
    ]
      .map((value) => (typeof value === 'string' ? value.trim() : ''))
      .filter(Boolean);
    if (refs.some((ref) => candidates.includes(ref))) {
      return camera;
    }
  }
  return null;
};

export const syntheticCameraFromSource = (
  source: LocalizationPipelineSource,
  fallbackKey?: string | null
): RigCameraInfo | null => {
  const key = (source.cameraUid ?? source.id ?? fallbackKey ?? '').trim();
  if (!key) return null;
  const streamId = String(source.streamId ?? '').trim() || null;
  const cameraUid = String(source.cameraUid ?? '').trim() || null;
  const streamAlias = (source.streamLabel ?? '').trim() || null;
  const displayName = streamAlias || cameraUid || streamId || source.id || key;
  const driverCameraId = String(source.cameraPath ?? '').trim() || key;
  return {
    uid: key,
    streamId,
    cameraUid,
    streamAlias,
    driverCameraId,
    displayName,
    backend: 'Localization',
    pose: { translation: { x: 0, y: 0, z: 0 }, rotation: { roll: 0, pitch: 0, yaw: 0 } }
  };
};

export const syntheticCameraFromKey = (key: string): RigCameraInfo | null => {
  const uid = key.trim();
  if (!uid) return null;
  return {
    uid,
    streamId: uid,
    cameraUid: uid,
    streamAlias: null,
    driverCameraId: uid,
    displayName: uid,
    backend: 'Localization',
    pose: { translation: { x: 0, y: 0, z: 0 }, rotation: { roll: 0, pitch: 0, yaw: 0 } }
  };
};

export const buildViewerRenderableCameras = ({
  baseFrame,
  viewerCameras,
  viewerCameraTransforms,
  viewOverlaySources,
  selectedSources
}: BuildViewerRenderableCamerasArgs): RigCameraInfo[] => {
  if (baseFrame !== 'field') return viewerCameras;
  const transforms = viewerCameraTransforms ?? {};
  const keys = Object.keys(transforms);
  if (keys.length === 0) return [];

  const known = new Set(keys);
  const matched = viewerCameras.filter((camera) => rigCameraKeys(camera).some((key) => known.has(key)));
  const covered = new Set<string>();
  for (const camera of matched) {
    for (const key of rigCameraKeys(camera)) {
      covered.add(key);
    }
  }

  const sourcePool = [...viewOverlaySources, ...selectedSources];
  const synthetic: RigCameraInfo[] = [];
  for (const key of keys) {
    if (covered.has(key)) continue;
    const matchedSource =
      sourcePool.find((source) => sourceKeys(source).some((candidate) => candidate === key)) ?? null;
    const camera = matchedSource
      ? syntheticCameraFromSource(matchedSource, key)
      : syntheticCameraFromKey(key);
    if (!camera) continue;
    synthetic.push(camera);
    for (const variant of rigCameraKeys(camera)) {
      covered.add(variant);
    }
  }

  return matched.concat(synthetic);
};

export const buildCameraPovOptionsBase = (
  viewProfiles: LocalizationProfile[],
  enabledSourcesForProfile: (profile: LocalizationProfile) => LocalizationPipelineSource[]
): CameraPovOptionBase[] => {
  const out: CameraPovOptionBase[] = [];
  for (const profile of viewProfiles) {
    const profileLabel = (profile.name ?? '').trim() || profile.id;
    const enabled = enabledSourcesForProfile(profile);
    for (const source of enabled) {
      const cameraAlias = (source.streamLabel || source.cameraUid || source.streamId || source.id).trim();
      const pipelineAlias = (source.pipelineLabel || source.pipelineId || 'pipeline').trim();
      const portLabel = (source.outputKey || 'port').trim();
      out.push({
        id: cameraPovOptionId(profile.id, source.id),
        profileId: profile.id,
        sourceId: source.id,
        groupLabel: profileLabel,
        subgroupLabel: cameraAlias,
        label: `${pipelineAlias} · ${portLabel}`
      });
    }
  }
  return out;
};

export const buildCameraPovByOptionId = ({
  cameraPovOptionsBase,
  profiles,
  sources,
  outputsForProfile,
  rigCameras,
  coordinateSpace,
  cameraIntrinsicsByKey
}: BuildCameraPovByOptionIdArgs): Record<string, CameraPovState> => {
  const out: Record<string, CameraPovState> = {};
  const profileById = new Map(profiles.map((profile) => [profile.id, profile]));
  const sourceById = new Map(sources.map((source) => [source.id, source]));

  for (const option of cameraPovOptionsBase) {
    const profile = profileById.get(option.profileId) ?? null;
    const source = sourceById.get(option.sourceId) ?? null;
    if (!profile || !source) continue;

    const outputs = outputsForProfile(profile);
    if (!outputs) continue;

    const sourceKeySet = new Set(sourceKeys(source));
    const rigCamera = (() => {
      let best: { camera: RigCameraInfo; score: number } | null = null;
      for (const camera of rigCameras) {
        const score = rigCameraKeys(camera).reduce(
          (count, key) => count + (sourceKeySet.has(key) ? 1 : 0),
          0
        );
        if (score <= 0) continue;
        if (!best || score > best.score) {
          best = { camera, score };
        }
      }
      return best?.camera ?? rigCameraForSource(source, rigCameras);
    })();
    for (const key of rigCameraKeys(rigCamera)) {
      sourceKeySet.add(key);
    }

    const cameraInField = (() => {
      const list = outputs.cameraInField ?? [];
      if (!list.length) return null;
      const picked = list.find((entry) =>
        collectKeyVariants([entry.cameraUid, entry.sourceId]).some((key) => sourceKeySet.has(key))
      );
      return picked ? toPoseTransform(picked.pose) : null;
    })();

    const fieldFromRobot = toPoseTransform(outputs.robotInField?.pose ?? null);
    const rigRobotFromCamera = rigPoseToViewerTransform(rigCamera?.pose ?? null);
    const fieldFromCamera =
      (fieldFromRobot && rigRobotFromCamera ? composeTransforms(fieldFromRobot, rigRobotFromCamera) : null) ??
      cameraInField;

    let transform: PoseTransform | null = null;
    if (coordinateSpace === 'tag_in_camera') {
      transform = identityTransform();
    } else if (coordinateSpace === 'tag_in_robot') {
      transform = rigRobotFromCamera;
    } else {
      transform = fieldFromCamera;
    }
    if (!transform) continue;

    const cameraKey = (source.cameraUid || source.streamId || source.id || rigCamera?.uid || '').trim() || null;
    const allKeys = new Set<string>([...Array.from(sourceKeySet.values()), ...rigCameraKeys(rigCamera)]);
    const intrinsicsEntry =
      Array.from(allKeys.values())
        .map((key) => cameraIntrinsicsByKey[key] ?? null)
        .find(
          (
            entry
          ): entry is { undistorted: CameraPovIntrinsics | null; raw: CameraPovIntrinsics | null } =>
            Boolean(entry)
        ) ?? null;
    const intrinsicsUndistorted = intrinsicsEntry?.undistorted ?? null;
    const intrinsicsRaw = intrinsicsEntry?.raw ?? intrinsicsUndistorted ?? null;

    out[option.id] = {
      optionId: option.id,
      profileId: profile.id,
      sourceId: source.id,
      cameraKey,
      transform,
      intrinsicsUndistorted,
      intrinsicsRaw
    };
  }

  return out;
};

export const buildCameraPovOptions = (
  cameraPovOptionsBase: CameraPovOptionBase[],
  cameraPovByOptionId: Record<string, CameraPovState>,
  lastCameraPovByOptionId: Record<string, CameraPovState>
): CameraPovOption[] =>
  cameraPovOptionsBase.map((option) => {
    const hasLivePose = Boolean(cameraPovByOptionId[option.id]);
    const hasGhostPose = !hasLivePose && Boolean(lastCameraPovByOptionId[option.id]);
    return {
      ...option,
      available: hasLivePose || hasGhostPose,
      ghost: hasGhostPose
    };
  });

export const resolveSelectedCameraPov = (
  selectionId: string,
  robotFollowOptionId: string,
  cameraPovByOptionId: Record<string, CameraPovState>,
  lastCameraPovByOptionId: Record<string, CameraPovState>
): CameraPovState | null => {
  const selected = selectionId.trim();
  if (!selected || selected === robotFollowOptionId) return null;
  return cameraPovByOptionId[selected] ?? lastCameraPovByOptionId[selected] ?? null;
};

export const resolveSelectedLiveCameraPov = (
  selectionId: string,
  robotFollowOptionId: string,
  cameraPovByOptionId: Record<string, CameraPovState>
): CameraPovState | null => {
  const selected = selectionId.trim();
  if (!selected || selected === robotFollowOptionId) return null;
  return cameraPovByOptionId[selected] ?? null;
};

export const mergeCameraPovStateCache = (
  current: Record<string, CameraPovState>,
  previous: Record<string, CameraPovState>,
  validOptionIds: string[]
): Record<string, CameraPovState> => {
  const next: Record<string, CameraPovState> = { ...previous };
  let changed = false;

  for (const [optionId, state] of Object.entries(current)) {
    const prev = next[optionId] ?? null;
    const sameTransform =
      prev &&
      prev.transform.position[0] === state.transform.position[0] &&
      prev.transform.position[1] === state.transform.position[1] &&
      prev.transform.position[2] === state.transform.position[2] &&
      prev.transform.quaternion.x === state.transform.quaternion.x &&
      prev.transform.quaternion.y === state.transform.quaternion.y &&
      prev.transform.quaternion.z === state.transform.quaternion.z &&
      prev.transform.quaternion.w === state.transform.quaternion.w;
    const sameIntrinsics =
      intrinsicsEqual(prev?.intrinsicsUndistorted ?? null, state.intrinsicsUndistorted) &&
      intrinsicsEqual(prev?.intrinsicsRaw ?? null, state.intrinsicsRaw);
    if (sameTransform && sameIntrinsics && prev?.cameraKey === state.cameraKey) {
      continue;
    }
    next[optionId] = state;
    changed = true;
  }

  const validOptions = new Set(validOptionIds);
  for (const key of Object.keys(next)) {
    if (!validOptions.has(key)) {
      delete next[key];
      changed = true;
    }
  }

  if (!changed && Object.keys(next).length !== Object.keys(previous).length) {
    changed = true;
  }
  return changed ? next : previous;
};
