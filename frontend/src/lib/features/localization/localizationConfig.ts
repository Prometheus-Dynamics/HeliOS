import { apiUrl } from '$lib/api/httpClient';
import { createDomainResource } from '$lib/api/domainResources';
import { apiFetch, apiFetchCachedJson } from '$lib/api/core/http';
import { cacheResourceData, type ResourceCacheContext, type ResourceCacheResult } from '$lib/api/resourceCache';

export type LocalizationPoseSpace =
  | 'tag_in_camera'
  | 'camera_in_tag'
  | 'tag_in_robot'
  | 'robot_in_tag'
  | 'camera_in_field'
  | 'robot_in_field';

export type LocalizationSolverMode = 'group_solve' | 'robust_group_solve' | 'per_camera_merge' | 'triangulate';
export type LocalizationFieldOriginMode = 'center' | 'blue' | 'red' | 'custom';

export type LocalizationCustomFieldOrigin = {
  x: number;
  z: number;
  yawDeg: number;
};

export type LocalizationFieldOriginConfig = {
  mode: LocalizationFieldOriginMode;
  custom?: LocalizationCustomFieldOrigin | null;
};

export type LocalizationConfig = {
  activeProfileId: string | null;
  profiles: LocalizationProfile[];
};

export type LocalizationProfilesExportEnvelope = {
  schema: string;
  exportedAt: string;
  config: LocalizationConfig;
};

export type LocalizationProfile = {
  id: string;
  name: string;
  tagSizeM?: number | null;
  allowedTagIds?: number[];
  excludedTagIds?: number[];
  fieldMapId: string | null;
  fieldOrigin?: LocalizationFieldOriginConfig;
  // Snap the reported field-space height to the ground plane.
  // Naming uses Z (common robotics convention) even though the viewer uses +Y up.
  snapZToGround?: boolean;
  // Snap field-space roll to level (0 deg).
  snapRollToGround?: boolean;
  // Snap field-space pitch to level (0 deg).
  snapPitchToGround?: boolean;
  color?: string | null;
  viewEnabled?: boolean;
  temporalStabilization?: LocalizationTemporalStabilizationConfig;
  sources: LocalizationSourceConfig[];
  solvers: LocalizationSolverConfig[];
};

export type LocalizationSourceConfig = {
  id: string;
  streamId: string;
  outputKey: string;
  cameraUid: string;
  poseSpace?: LocalizationPoseSpace | null;
  inputKey?: string | null;
  enabled: boolean;
  weight: number;
};

export type LocalizationSolverConfig = {
  id: string;
  name: string;
  mode: LocalizationSolverMode;
  outputSpaces: LocalizationPoseSpace[];
  sourceIds: string[];
  color?: string | null;
  runtimeTuning?: LocalizationSolverRuntimeTuningConfig;
  temporalStabilization?: LocalizationTemporalStabilizationConfig | null;
};

export type LocalizationSolverRuntimeTuningConfig = {
  minObservationWeight: number;
  minSingleTagSolveWeight: number;
  minMultiTagTotalWeight: number;
  minMultiTagEffectiveCount: number;
  weakSingleTagMargin: number;
  coplanarHeightDeltaM: number;
  severeObservedHeightDeltaM: number;
  moderateObservedHeightDeltaM: number;
  mildObservedHeightDeltaM: number;
  severePenalty: number;
  moderatePenalty: number;
  mildPenalty: number;
  dtScaleMin: number;
  dtScaleMax: number;
  switchedSingleTagMaxTranslationJumpM: number;
  switchedSingleTagMaxRotationJumpDeg: number;
  droppedMultiToSingleMaxTranslationJumpM: number;
  droppedMultiToSingleMaxRotationJumpDeg: number;
  switchedSingleTagRejectWindowScale: number;
  switchedSingleTagRejectWindowMinMs: number;
  droppedMultiToSingleRejectWindowScale: number;
  droppedMultiToSingleRejectWindowMinMs: number;
  switchedSingleTagGainDamp: number;
  switchedSingleTagMinTranslationGain: number;
  switchedSingleTagMinRotationGain: number;
  droppedMultiToSingleGainDamp: number;
  droppedMultiToSingleMinTranslationGain: number;
  droppedMultiToSingleMinRotationGain: number;
};

export type LocalizationTemporalStabilizationConfig = {
  enabled: boolean;
  singleTagTranslationAlpha: number;
  singleTagRotationAlpha: number;
  multiTagTranslationAlpha: number;
  multiTagRotationAlpha: number;
  maxTranslationJumpM: number;
  maxRotationJumpDeg: number;
  reanchorRejectWindowMs: number;
};

export const DEFAULT_TEMPORAL_STABILIZATION: LocalizationTemporalStabilizationConfig = {
  enabled: true,
  singleTagTranslationAlpha: 0.18,
  singleTagRotationAlpha: 0.16,
  multiTagTranslationAlpha: 0.45,
  multiTagRotationAlpha: 0.38,
  maxTranslationJumpM: 1.2,
  maxRotationJumpDeg: 70,
  reanchorRejectWindowMs: 450
};

export const DEFAULT_SOLVER_RUNTIME_TUNING: LocalizationSolverRuntimeTuningConfig = {
  minObservationWeight: 0.03,
  minSingleTagSolveWeight: 0.34,
  minMultiTagTotalWeight: 0.58,
  minMultiTagEffectiveCount: 1.2,
  weakSingleTagMargin: 0.08,
  coplanarHeightDeltaM: 0.08,
  severeObservedHeightDeltaM: 0.45,
  moderateObservedHeightDeltaM: 0.25,
  mildObservedHeightDeltaM: 0.15,
  severePenalty: 0.1,
  moderatePenalty: 0.3,
  mildPenalty: 0.6,
  dtScaleMin: 0.4,
  dtScaleMax: 2.5,
  switchedSingleTagMaxTranslationJumpM: 0.38,
  switchedSingleTagMaxRotationJumpDeg: 24,
  droppedMultiToSingleMaxTranslationJumpM: 0.58,
  droppedMultiToSingleMaxRotationJumpDeg: 36,
  switchedSingleTagRejectWindowScale: 1.8,
  switchedSingleTagRejectWindowMinMs: 700,
  droppedMultiToSingleRejectWindowScale: 1.3,
  droppedMultiToSingleRejectWindowMinMs: 520,
  switchedSingleTagGainDamp: 0.35,
  switchedSingleTagMinTranslationGain: 0.04,
  switchedSingleTagMinRotationGain: 0.04,
  droppedMultiToSingleGainDamp: 0.5,
  droppedMultiToSingleMinTranslationGain: 0.06,
  droppedMultiToSingleMinRotationGain: 0.06
};

export const DEFAULT_FIELD_ORIGIN: LocalizationFieldOriginConfig = {
  mode: 'blue',
  custom: null
};

export type LocalizationSolveResponse = {
  profileId: string;
  solvers: LocalizationSolverResult[];
  sources: LocalizationSourceSampleStatus[];
};

export type LocalizationSourceSampleStatus = {
  sourceId: string;
  streamId: string;
  outputKey: string;
  cameraUid: string;
  detections: number;
  pollMs: number;
  tagSize?: number | null;
  error?: string | null;
};

export type LocalizationSolverResult = {
  id: string;
  name: string;
  mode: LocalizationSolverMode;
  outputSpaces: LocalizationPoseSpace[];
  outputs: LocalizationSolverOutputs;
  errors: string[];
};

export type LocalizationSolverOutputs = {
  tagInCamera?: LocalizationDetectionPose[];
  cameraInTag?: LocalizationDetectionPose[];
  tagInRobot?: LocalizationDetectionPose[];
  robotInTag?: LocalizationDetectionPose[];
  cameraInField?: LocalizationSourcePose[];
  robotInField?: LocalizationSolverPose;
};

export type LocalizationDetectionPose = {
  sourceId: string;
  cameraUid: string;
  tagId: number;
  pose: LocalizationPose;
  tagSize?: number | null;
  codeRotation?: number | null;
  tagBits?: { width: number; border: number; rows: string[] } | null;
};

export type LocalizationSourcePose = {
  sourceId: string;
  cameraUid: string;
  weight: number;
  pose: LocalizationPose;
};

export type LocalizationSolverPose = {
  pose: LocalizationPose;
  sourceIds: string[];
};

export type LocalizationPose = {
  translation: { x: number; y: number; z: number };
  rotation: {
    roll: number;
    pitch: number;
    yaw: number;
    quaternion: { x: number; y: number; z: number; w: number };
  };
};

export async function fetchLocalizationConfig(): Promise<LocalizationConfig>;
export async function fetchLocalizationConfig(
  context: ResourceCacheContext<LocalizationConfig>
): Promise<LocalizationConfig | ResourceCacheResult<LocalizationConfig>>;
export async function fetchLocalizationConfig(
  context?: ResourceCacheContext<LocalizationConfig>
): Promise<LocalizationConfig | ResourceCacheResult<LocalizationConfig>> {
  const payload = await apiFetchCachedJson<LocalizationConfig>(apiUrl('/localization/config'), context ?? {}, {
    method: 'GET',
    headers: { Accept: 'application/json' }
  });
  if (typeof context === 'undefined') {
    if (payload.status === 'not_modified') {
      if (typeof payload.data !== 'undefined') {
        return payload.data;
      }
      throw new Error('Localization config was not modified but no cached payload was available.');
    }
    return payload.data;
  }
  if (payload.status === 'not_modified') {
    return payload;
  }
  return cacheResourceData(payload.data, {
    etag: payload.etag ?? null,
    revision: payload.revision ?? null
  });
}

export const localizationConfigResource = createDomainResource({
  key: 'localization:config:v1',
  loader: fetchLocalizationConfig,
  staleMs: 30_000,
  maxAgeMs: 180_000,
  kinds: ['localization.config', 'localization.profiles', 'localization.sources', 'streams.lifecycle', 'streams.pipeline', 'device.hardware']
});

export async function updateLocalizationConfig(config: LocalizationConfig): Promise<LocalizationConfig> {
  return apiFetch<LocalizationConfig>(apiUrl('/localization/config'), {
    method: 'PUT',
    headers: { Accept: 'application/json' },
    body: config
  });
}

export async function fetchLocalizationProfilesExport(): Promise<LocalizationProfilesExportEnvelope> {
  return apiFetch<LocalizationProfilesExportEnvelope>(apiUrl('/localization/profiles/export'), {
    method: 'GET',
    headers: { Accept: 'application/json' }
  });
}

export async function importLocalizationProfiles(
  payload: LocalizationProfilesExportEnvelope | LocalizationConfig
): Promise<LocalizationConfig> {
  return apiFetch<LocalizationConfig>(apiUrl('/localization/profiles/import'), {
    method: 'POST',
    headers: { Accept: 'application/json' },
    body: payload
  });
}

export async function fetchLocalizationSolve(profileId?: string, signal?: AbortSignal): Promise<LocalizationSolveResponse> {
  const url = profileId
    ? apiUrl(`/localization/solve?profile_id=${encodeURIComponent(profileId)}&apply_field_origin=false`)
    : apiUrl('/localization/solve?apply_field_origin=false');
  return apiFetch<LocalizationSolveResponse>(url, { method: 'GET', headers: { Accept: 'application/json' }, signal });
}
