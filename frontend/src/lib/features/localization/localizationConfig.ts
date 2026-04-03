import { apiUrl } from '$lib/api/client';
import { createDomainResource } from '$lib/api/domainResources';
import { apiFetch, apiFetchCachedJson } from '$lib/api/core/http';
import { cacheResourceData, type ResourceCacheContext, type ResourceCacheResult } from '$lib/api/resourceCache';
import type {
  LocalizationDetectionPose as GeneratedLocalizationDetectionPose,
  LocalizationCustomFieldOrigin as GeneratedLocalizationCustomFieldOrigin,
  LocalizationFieldOriginConfig as GeneratedLocalizationFieldOriginConfig,
  LocalizationFieldOriginMode as GeneratedLocalizationFieldOriginMode,
  LocalizationPose as GeneratedLocalizationPose,
  LocalizationPoseSpace as GeneratedLocalizationPoseSpace,
  LocalizationProfilesExportEnvelope as GeneratedLocalizationProfilesExportEnvelope,
  LocalizationSolveResponse as GeneratedLocalizationSolveResponse,
  LocalizationSolveTimings as GeneratedLocalizationSolveTimings,
  LocalizationSolverOutputs as GeneratedLocalizationSolverOutputs,
  LocalizationSolverPose as GeneratedLocalizationSolverPose,
  LocalizationSolverResult as GeneratedLocalizationSolverResult,
  LocalizationSolverMode as GeneratedLocalizationSolverMode,
  LocalizationSolverRuntimeTuningConfig as GeneratedLocalizationSolverRuntimeTuningConfig,
  LocalizationSourcePose as GeneratedLocalizationSourcePose,
  LocalizationSourceSampleStatus as GeneratedLocalizationSourceSampleStatus,
  LocalizationTemporalStabilizationConfig as GeneratedLocalizationTemporalStabilizationConfig
} from '$lib/api/client';

export type LocalizationPoseSpace = GeneratedLocalizationPoseSpace;
export type LocalizationSolverMode = GeneratedLocalizationSolverMode;
export type LocalizationFieldOriginMode = GeneratedLocalizationFieldOriginMode;
export type LocalizationCustomFieldOrigin = GeneratedLocalizationCustomFieldOrigin;
export type LocalizationFieldOriginConfig = GeneratedLocalizationFieldOriginConfig;

export type LocalizationConfig = {
  activeProfileId: string | null;
  profiles: LocalizationProfile[];
};

export type LocalizationProfilesExportEnvelope = GeneratedLocalizationProfilesExportEnvelope;

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
  enabled?: boolean;
  color?: string | null;
  viewEnabled: boolean;
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

export type LocalizationSolverRuntimeTuningConfig = GeneratedLocalizationSolverRuntimeTuningConfig;
export type LocalizationTemporalStabilizationConfig = GeneratedLocalizationTemporalStabilizationConfig;

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

export type LocalizationSolveResponse = GeneratedLocalizationSolveResponse;
export type LocalizationSolveTimings = GeneratedLocalizationSolveTimings;
export type LocalizationSourceSampleStatus = GeneratedLocalizationSourceSampleStatus;
export type LocalizationSolverResult = GeneratedLocalizationSolverResult;
export type LocalizationSolverOutputs = GeneratedLocalizationSolverOutputs;
export type LocalizationDetectionPose = GeneratedLocalizationDetectionPose;
export type LocalizationSourcePose = GeneratedLocalizationSourcePose;
export type LocalizationSolverPose = GeneratedLocalizationSolverPose;
export type LocalizationPose = GeneratedLocalizationPose;

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

export type FetchLocalizationSolveOptions = {
  fieldPosesOnly?: boolean;
};

export async function fetchLocalizationSolve(
  profileId?: string,
  signal?: AbortSignal,
  options: FetchLocalizationSolveOptions = {}
): Promise<LocalizationSolveResponse> {
  const params = new URLSearchParams();
  if (profileId) {
    params.set('profile_id', profileId);
  }
  params.set('apply_field_origin', 'false');
  if (options.fieldPosesOnly) {
    params.set('field_poses_only', 'true');
  }
  const url = apiUrl(`/localization/solve?${params.toString()}`);
  return apiFetch<LocalizationSolveResponse>(url, { method: 'GET', headers: { Accept: 'application/json' }, signal });
}
