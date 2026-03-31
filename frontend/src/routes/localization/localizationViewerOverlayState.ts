import type { FieldMapDocument } from '$lib/features/localization/fieldMaps';
import type {
  LocalizationDetectionPose,
  LocalizationFieldOriginMode,
  LocalizationPoseSpace,
  LocalizationProfile,
  LocalizationSolverOutputs
} from '$lib/features/localization/localizationConfig';
import {
  composeTransforms,
  type PoseQuaternion,
  quaternionToEulerDegreesXYZ,
  type PoseTransform,
  type Vec3
} from '$lib/features/localization/poseMath';
import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
import type { LocalizationMarker } from '$lib/features/localization/viewers/localizationViewerTypes';
import {
  frcOriginDefinition,
  transformFromFieldCenter,
  type PlanarFieldOrigin
} from '$lib/features/localization/fieldOrigins';
import { poseSpaceLabel, profileColorForId, PROFILE_COLORS } from '$lib/features/localization/utils';
import { detectionPosesForSpace } from '$lib/features/localization/markerUtils';
import type { RigCameraInfo } from '$lib/types/rig';
import {
  cameraFramePositionForDetection,
  cameraKeyVariants,
  detectionInsidePovFov,
  rigCameraKeys,
  selectedPovIntrinsics,
  type CameraPovFovMode,
  type CameraPovIntrinsics,
  type CameraPovState
} from './localizationCameraPovUtils';
import { normalizeFieldOriginConfig } from './localizationTuningActions';
import type { LocalizationBaseFrame } from '$lib/features/localization/page/localizationPageViewHelpers';

export type ViewerDetectionPose = LocalizationDetectionPose & { profileId: string; color: string | null };

export type LocalTagPoseOverlayEntry = {
  key: string;
  profileId: string;
  space: LocalizationPoseSpace;
  color: string | null;
  label: string;
  values: string;
  timestampMs: number;
};

export type FieldSpacePoseOverlayEntry = {
  pose: PoseTransform;
  timestampMs: number;
};

type ProfileOutputsResolver = (profile: LocalizationProfile) => LocalizationSolverOutputs | null;

type FieldDetectionsResolver = (
  outputs: LocalizationSolverOutputs | null,
  sourceById: Map<string, LocalizationPipelineSource>
) => LocalizationDetectionPose[];

type TargetSpaceOverlay = {
  header: string;
  rows: Array<{
    key: string;
    color: string | null;
    label: string;
    values: string;
    stale: boolean;
  }>;
} | null;

export const fieldSpacePoseForOutputs = (
  outputs: LocalizationSolverOutputs | null,
  space: LocalizationPoseSpace
): PoseTransform | null => {
  if (!outputs) return null;
  if (space === 'robot_in_field') {
    const pose = outputs.robotInField?.pose ?? null;
    return pose
      ? ({
          position: [pose.translation.x, pose.translation.y, pose.translation.z],
          quaternion: pose.rotation.quaternion
        } as PoseTransform)
      : null;
  }
  if (space === 'camera_in_field') {
    const entry = outputs.cameraInField?.[0] ?? null;
    return entry
      ? ({
          position: [
            entry.pose.translation.x,
            entry.pose.translation.y,
            entry.pose.translation.z
          ],
          quaternion: entry.pose.rotation.quaternion
        } as PoseTransform)
      : null;
  }
  return null;
};

export const poseTransformEquals = (
  a: PoseTransform | null | undefined,
  b: PoseTransform | null | undefined
): boolean => {
  if (!a || !b) return false;
  return (
    a.position[0] === b.position[0] &&
    a.position[1] === b.position[1] &&
    a.position[2] === b.position[2] &&
    a.quaternion.x === b.quaternion.x &&
    a.quaternion.y === b.quaternion.y &&
    a.quaternion.z === b.quaternion.z &&
    a.quaternion.w === b.quaternion.w
  );
};

export const localTagPoseCacheKey = (
  profileId: string,
  space: LocalizationPoseSpace,
  detection: LocalizationDetectionPose
): string => {
  const cameraUid = (detection.cameraUid ?? '').trim();
  return `${profileId}:${space}:${detection.sourceId}:${cameraUid}:${detection.tagId}`;
};

export const localTagPoseValues = (detection: LocalizationDetectionPose): string => {
  const formatNumber = (value: number, decimals: number): string =>
    Number.isFinite(value) ? value.toFixed(decimals) : '—';
  const pose = detection.pose;
  const tx = pose.translation.x;
  const ty = pose.translation.y;
  const tz = pose.translation.z;
  const roll = pose.rotation.roll;
  const pitch = pose.rotation.pitch;
  const yaw = pose.rotation.yaw;
  const distanceM =
    Number.isFinite(tx) && Number.isFinite(ty) && Number.isFinite(tz)
      ? Math.hypot(tx, ty, tz)
      : Number.NaN;
  return `id:${detection.tagId} (${formatNumber(tx, 3)}, ${formatNumber(ty, 3)}, ${formatNumber(
    tz,
    3
  )}) (${formatNumber(roll, 1)}, ${formatNumber(pitch, 1)}, ${formatNumber(
    yaw,
    1
  )}) d:${formatNumber(distanceM, 3)}m`;
};

export const buildLocalTagPoseCache = (options: {
  coordinateSpace: LocalizationPoseSpace;
  lastLocalTagPoseByKey: Record<string, LocalTagPoseOverlayEntry>;
  nowMs: number;
  lingerMs: number;
  refreshMs: number;
  viewProfilesWithSources: LocalizationProfile[];
  outputsForProfile: ProfileOutputsResolver;
  profiles: LocalizationProfile[];
  profileIndexById: Map<string, number>;
}): Record<string, LocalTagPoseOverlayEntry> => {
  const {
    coordinateSpace,
    lastLocalTagPoseByKey,
    nowMs,
    lingerMs,
    refreshMs,
    viewProfilesWithSources,
    outputsForProfile,
    profiles,
    profileIndexById
  } = options;
  const isFieldSpace =
    coordinateSpace === 'camera_in_field' || coordinateSpace === 'robot_in_field';
  const expireBeforeMs = nowMs - lingerMs;
  const next: Record<string, LocalTagPoseOverlayEntry> = {};
  let changed = false;

  for (const [key, entry] of Object.entries(lastLocalTagPoseByKey)) {
    if (entry.timestampMs >= expireBeforeMs) {
      next[key] = entry;
    } else {
      changed = true;
    }
  }

  if (!isFieldSpace) {
    for (const profile of viewProfilesWithSources) {
      const outputs = outputsForProfile(profile);
      const detections = detectionPosesForSpace(outputs, coordinateSpace);
      const color = profileColorForId(profile.id, profiles, profileIndexById, PROFILE_COLORS);
      const label = profile.name || profile.id;
      for (const detection of detections) {
        const key = localTagPoseCacheKey(profile.id, coordinateSpace, detection);
        const prev = next[key] ?? null;
        const values = localTagPoseValues(detection);
        const needsRefresh =
          !prev ||
          prev.profileId !== profile.id ||
          prev.space !== coordinateSpace ||
          prev.color !== color ||
          prev.label !== label ||
          prev.values !== values ||
          nowMs - prev.timestampMs >= refreshMs;
        if (!needsRefresh) continue;
        next[key] = {
          key,
          profileId: profile.id,
          space: coordinateSpace,
          color,
          label,
          values,
          timestampMs: nowMs
        };
        changed = true;
      }
    }
  }

  if (!changed && Object.keys(next).length !== Object.keys(lastLocalTagPoseByKey).length) {
    changed = true;
  }
  return changed ? next : lastLocalTagPoseByKey;
};

export const buildFieldSpacePoseCache = (options: {
  coordinateSpace: LocalizationPoseSpace;
  lastFieldSpacePoseByProfileSpace: Record<string, FieldSpacePoseOverlayEntry>;
  nowMs: number;
  lingerMs: number;
  refreshMs: number;
  viewProfilesWithSources: LocalizationProfile[];
  outputsForProfile: ProfileOutputsResolver;
}): Record<string, FieldSpacePoseOverlayEntry> => {
  const {
    coordinateSpace,
    lastFieldSpacePoseByProfileSpace,
    nowMs,
    lingerMs,
    refreshMs,
    viewProfilesWithSources,
    outputsForProfile
  } = options;
  const isFieldSpace =
    coordinateSpace === 'camera_in_field' || coordinateSpace === 'robot_in_field';
  const expireBeforeMs = nowMs - lingerMs;
  const next: Record<string, FieldSpacePoseOverlayEntry> = {};
  let changed = false;

  for (const [key, entry] of Object.entries(lastFieldSpacePoseByProfileSpace)) {
    if (entry.timestampMs >= expireBeforeMs) {
      next[key] = entry;
    } else {
      changed = true;
    }
  }

  if (isFieldSpace) {
    for (const profile of viewProfilesWithSources) {
      const key = `${profile.id}:${coordinateSpace}`;
      const livePose = fieldSpacePoseForOutputs(outputsForProfile(profile), coordinateSpace);
      if (!livePose) continue;
      const prev = next[key] ?? null;
      const needsRefresh =
        !prev || !poseTransformEquals(prev.pose, livePose) || nowMs - prev.timestampMs >= refreshMs;
      if (!needsRefresh) continue;
      next[key] = { pose: livePose, timestampMs: nowMs };
      changed = true;
    }
  }

  if (!changed && Object.keys(next).length !== Object.keys(lastFieldSpacePoseByProfileSpace).length) {
    changed = true;
  }
  return changed ? next : lastFieldSpacePoseByProfileSpace;
};

export const buildDetectedFieldTagDetections = (options: {
  baseFrame: LocalizationBaseFrame;
  coordinateSpace: LocalizationPoseSpace;
  selectedPov: CameraPovState | null;
  cameraPovFovMode: CameraPovFovMode;
  povForwardSign: 1 | -1;
  viewProfiles: LocalizationProfile[];
  outputsForProfile: ProfileOutputsResolver;
  profiles: LocalizationProfile[];
  profileIndexById: Map<string, number>;
  sources: LocalizationPipelineSource[];
  fieldDetectionsForOutputs: FieldDetectionsResolver;
}): ViewerDetectionPose[] => {
  const {
    baseFrame,
    coordinateSpace,
    selectedPov,
    cameraPovFovMode,
    povForwardSign,
    viewProfiles,
    outputsForProfile,
    profiles,
    profileIndexById,
    sources,
    fieldDetectionsForOutputs
  } = options;
  if (baseFrame !== 'field') return [];

  const povIntrinsics = selectedPovIntrinsics(selectedPov, cameraPovFovMode);
  const povProfileId = selectedPov?.profileId ?? null;
  const povSourceId = selectedPov?.sourceId ?? null;
  const sourceById = new Map(sources.map((source) => [source.id, source]));
  const allDetections = viewProfiles.flatMap<ViewerDetectionPose>((profile) => {
    const outputs = outputsForProfile(profile);
    const color = profileColorForId(profile.id, profiles, profileIndexById, PROFILE_COLORS);
    return fieldDetectionsForOutputs(outputs, sourceById).map((detection) => ({
      ...detection,
      profileId: profile.id,
      color
    }));
  });

  if (!povSourceId || !povProfileId) return allDetections;
  const scoped = allDetections.filter(
    (detection) => detection.sourceId === povSourceId && detection.profileId === povProfileId
  );
  return scoped.filter((detection) =>
    detectionInsidePovFov(detection, coordinateSpace, selectedPov, povIntrinsics, povForwardSign)
  );
};

export const buildReferenceMarkersForViewer = (options: {
  baseFrame: LocalizationBaseFrame;
  activeFieldMapDoc: FieldMapDocument | null;
  detectedFieldTagDetections: ViewerDetectionPose[];
}): LocalizationMarker[] => {
  const { baseFrame, activeFieldMapDoc, detectedFieldTagDetections } = options;
  if (baseFrame !== 'field') return [];
  if (!activeFieldMapDoc) return [];
  const detectedColorByTagId = new Map<string, string>();
  for (const detection of detectedFieldTagDetections) {
    const tagKey = String(detection.tagId);
    if (!detectedColorByTagId.has(tagKey)) {
      detectedColorByTagId.set(tagKey, detection.color ?? '#38bdf8');
    }
  }
  return activeFieldMapDoc.markers.map((marker) => {
    const tagKey = String(marker.id);
    const highlightColor = detectedColorByTagId.get(tagKey) ?? null;
    return {
      id: `map:${activeFieldMapDoc.id}:${marker.id}`,
      label: `Tag ${marker.id}`,
      targetType: 'aruco-plane',
      tagId: marker.id,
      tagSize: marker.sizeM,
      position: marker.position,
      quaternion: marker.quaternion,
      tagBits: marker.tagBits ?? undefined,
      color: highlightColor ?? '#94a3b8',
      status: highlightColor ? 'tracking' : 'idle'
    } as LocalizationMarker;
  });
};

export const buildViewerTagLineMarkers = (options: {
  baseFrame: LocalizationBaseFrame;
  showTagLines: boolean;
  activeFieldMapDoc: FieldMapDocument | null;
  detectedFieldTagDetections: ViewerDetectionPose[];
  sources: LocalizationPipelineSource[];
  liveMarkers: LocalizationMarker[];
}): LocalizationMarker[] => {
  const {
    baseFrame,
    showTagLines,
    activeFieldMapDoc,
    detectedFieldTagDetections,
    sources,
    liveMarkers
  } = options;
  if (baseFrame !== 'field') return liveMarkers;
  if (!showTagLines) return [];
  if (!activeFieldMapDoc) return [];
  const markerByTagId = new Map<string, (typeof activeFieldMapDoc.markers)[number]>();
  for (const marker of activeFieldMapDoc.markers) {
    markerByTagId.set(String(marker.id), marker);
  }
  const sourceById = new Map(sources.map((source) => [source.id, source]));
  return detectedFieldTagDetections
    .map((detection) => {
      const mapMarker = markerByTagId.get(String(detection.tagId)) ?? null;
      if (!mapMarker) return null;
      const source = sourceById.get(detection.sourceId) ?? null;
      return {
        id: `line:${detection.profileId}:${detection.sourceId}:${detection.cameraUid}:${detection.tagId}`,
        label: `Tag ${detection.tagId}`,
        targetType: 'aruco-plane',
        tagId: mapMarker.id,
        tagSize: mapMarker.sizeM,
        position: mapMarker.position,
        quaternion: mapMarker.quaternion,
        tagBits: mapMarker.tagBits ?? undefined,
        color: detection.color ?? '#38bdf8',
        source: {
          id: detection.sourceId,
          streamId: source?.streamId,
          outputKey: source?.outputKey,
          streamLabel: source?.streamLabel,
          cameraUid: source?.cameraUid ?? detection.cameraUid,
          cameraPath: source?.cameraPath,
          pipelineId: source?.pipelineId,
          pipelineLabel: source?.pipelineLabel,
          profileId: detection.profileId
        }
      } as LocalizationMarker;
    })
    .filter((marker): marker is LocalizationMarker => Boolean(marker));
};

export const buildMinimapPoseDot = (options: {
  hasAnyViewsEnabled: boolean;
  viewerAccentColor: string | null;
  coordinateSpace: LocalizationPoseSpace;
  viewerRobotTransform: { position: Vec3; quaternion?: PoseQuaternion } | null;
  solverRobotTransform: PoseTransform | null;
  selectedLiveCameraPov: CameraPovState | null;
  viewerCameraTransforms: Record<string, { position: Vec3; quaternion?: PoseQuaternion }> | null;
  primaryCameraKey: string | null;
  viewerRenderableCameras: RigCameraInfo[];
  baseFrame: LocalizationBaseFrame;
}): { position: Vec3; color: string | null } | null => {
  const {
    hasAnyViewsEnabled,
    viewerAccentColor,
    coordinateSpace,
    viewerRobotTransform,
    solverRobotTransform,
    selectedLiveCameraPov,
    viewerCameraTransforms,
    primaryCameraKey,
    viewerRenderableCameras,
    baseFrame
  } = options;
  if (!hasAnyViewsEnabled) return null;
  const color = viewerAccentColor;
  const toGround = (position: Vec3): Vec3 => [position[0], 0.04, position[2]];

  if (coordinateSpace === 'robot_in_field') {
    const transform = viewerRobotTransform ?? solverRobotTransform;
    if (!transform) return null;
    return { position: toGround(transform.position), color };
  }

  if (coordinateSpace === 'camera_in_field') {
    const povTransform = selectedLiveCameraPov?.transform ?? null;
    if (povTransform) {
      return { position: toGround(povTransform.position), color };
    }

    const transforms = viewerCameraTransforms ?? {};
    if (!Object.keys(transforms).length) return null;

    const keyCandidates = new Set<string>();
    const addKeys = (value: string | null | undefined) => {
      for (const key of cameraKeyVariants(value)) {
        keyCandidates.add(key);
      }
    };
    addKeys(primaryCameraKey);
    const firstViewerCamera = viewerRenderableCameras[0] ?? null;
    if (firstViewerCamera) {
      for (const key of rigCameraKeys(firstViewerCamera)) {
        keyCandidates.add(key);
      }
    }

    for (const key of keyCandidates) {
      const transform = transforms[key];
      if (transform) {
        return { position: toGround(transform.position), color };
      }
    }

    const fallback = Object.values(transforms)[0] ?? null;
    if (!fallback) return null;
    return { position: toGround(fallback.position), color };
  }

  if (baseFrame === 'camera' || baseFrame === 'robot') {
    return { position: [0, 0.04, 0], color };
  }

  return null;
};

export const buildTargetSpaceOverlay = (options: {
  showOutputsOverlay: boolean;
  coordinateSpace: LocalizationPoseSpace;
  baseFrame: LocalizationBaseFrame;
  viewProfilesWithSources: LocalizationProfile[];
  outputsForProfile: ProfileOutputsResolver;
  lastLocalTagPoseByKey: Record<string, LocalTagPoseOverlayEntry>;
  lastFieldSpacePoseByProfileSpace: Record<string, FieldSpacePoseOverlayEntry>;
  profiles: LocalizationProfile[];
  profileIndexById: Map<string, number>;
  activeFieldDimensions: { width: number; depth: number } | null;
  fieldMapDocs: Record<string, FieldMapDocument>;
}): TargetSpaceOverlay => {
  const {
    showOutputsOverlay,
    coordinateSpace,
    baseFrame,
    viewProfilesWithSources,
    outputsForProfile,
    lastLocalTagPoseByKey,
    lastFieldSpacePoseByProfileSpace,
    profiles,
    profileIndexById,
    activeFieldDimensions,
    fieldMapDocs
  } = options;
  if (showOutputsOverlay) return null;

  const isFieldSpace =
    coordinateSpace === 'camera_in_field' || coordinateSpace === 'robot_in_field';

  if (!isFieldSpace) {
    const liveKeys = new Set<string>();
    for (const profile of viewProfilesWithSources) {
      const outputs = outputsForProfile(profile);
      const detections = detectionPosesForSpace(outputs, coordinateSpace);
      for (const detection of detections) {
        liveKeys.add(localTagPoseCacheKey(profile.id, coordinateSpace, detection));
      }
    }
    const profileIdSet = new Set(viewProfilesWithSources.map((profile) => profile.id));
    const rows = Object.values(lastLocalTagPoseByKey)
      .filter((entry) => entry.space === coordinateSpace && profileIdSet.has(entry.profileId))
      .sort((a, b) => {
        if (a.label !== b.label) return a.label.localeCompare(b.label);
        return a.key.localeCompare(b.key);
      })
      .map((entry) => ({
        key: entry.key,
        color: entry.color,
        label: entry.label,
        values: entry.values,
        stale: !liveKeys.has(entry.key)
      }));

    return {
      header: `${poseSpaceLabel(coordinateSpace)} · tag poses`,
      rows
    };
  }

  if (baseFrame !== 'field') return null;

  const defaultDims = activeFieldDimensions ?? { width: 8.2296, depth: 16.4592 };
  const originLabelForMode = (mode: LocalizationFieldOriginMode): string =>
    mode === 'blue' ? 'wpiblue' : mode === 'red' ? 'wpired' : mode === 'center' ? 'center' : 'custom';
  const originForProfile = (profile: LocalizationProfile): { transform: PoseTransform; label: string } => {
    const normalized = normalizeFieldOriginConfig(profile.fieldOrigin);
    const mapIdRaw = typeof profile.fieldMapId === 'string' ? profile.fieldMapId.trim() : '';
    const doc = mapIdRaw ? fieldMapDocs[mapIdRaw] ?? null : null;
    const dims = doc ? { width: doc.widthM, depth: doc.depthM } : defaultDims;
    if (normalized.mode === 'custom' && normalized.custom) {
      const origin: PlanarFieldOrigin = {
        id: 'custom',
        name: 'Custom origin',
        x: normalized.custom.x,
        z: normalized.custom.z,
        yawDeg: normalized.custom.yawDeg
      };
      return { transform: transformFromFieldCenter(origin), label: originLabelForMode('custom') };
    }
    const frcMode = normalized.mode === 'red' ? 'red' : normalized.mode === 'center' ? 'center' : 'blue';
    return {
      transform: transformFromFieldCenter(frcOriginDefinition(frcMode, dims)),
      label: originLabelForMode(frcMode)
    };
  };

  const num = (value: number | null, width: number, decimals: number) => {
    if (typeof value !== 'number' || !Number.isFinite(value)) return '—'.padStart(width, ' ');
    return value.toFixed(decimals).padStart(width, ' ');
  };

  const viewerPositionToWpilib = (position: Vec3): Vec3 => [position[2], position[0], position[1]];

  const fmtValues = (pose: PoseTransform | null, originFromCenter: PoseTransform): string => {
    if (!pose) {
      return `x:${num(null, 8, 3)} y:${num(null, 8, 3)} z:${num(null, 8, 3)}  r:${num(
        null,
        7,
        1
      )} p:${num(null, 7, 1)} y:${num(null, 7, 1)}`;
    }
    const resolved = composeTransforms(originFromCenter, pose);
    const pos = viewerPositionToWpilib(resolved.position);
    const euler = quaternionToEulerDegreesXYZ(resolved.quaternion);
    return `x:${num(pos[0], 8, 3)} y:${num(pos[1], 8, 3)} z:${num(pos[2], 8, 3)}  r:${num(
      euler.roll,
      7,
      1
    )} p:${num(euler.pitch, 7, 1)} y:${num(euler.yaw, 7, 1)}`;
  };

  const rows = viewProfilesWithSources.map((profile) => {
    const outputs = outputsForProfile(profile);
    const key = `${profile.id}:${coordinateSpace}`;
    const livePose = fieldSpacePoseForOutputs(outputs, coordinateSpace);
    const cached = lastFieldSpacePoseByProfileSpace[key] ?? null;
    const pose = livePose ?? cached?.pose ?? null;

    const color = profileColorForId(profile.id, profiles, profileIndexById, PROFILE_COLORS);
    const origin = originForProfile(profile);
    return {
      key,
      color,
      label: profile.name || profile.id,
      values: fmtValues(pose, origin.transform),
      stale: !livePose && Boolean(cached),
      originLabel: origin.label
    };
  });

  if (rows.length === 0) return null;
  const headerOriginLabel = rows.length === 1 ? rows[0]?.originLabel ?? 'field' : 'mixed-origins';
  const header = `${headerOriginLabel} · ${poseSpaceLabel(coordinateSpace)}`;
  return {
    header,
    rows: rows.map((row) => {
      const { originLabel, ...rest } = row;
      void originLabel;
      return rest;
    })
  };
};
