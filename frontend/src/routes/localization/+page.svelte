<script lang="ts">
  import { browser } from '$app/environment';
  import { onDestroy, onMount } from 'svelte';
  import { toaster } from '$lib';
  import { openStreamMetricsSocket } from '$lib/api/streamMetrics';
  import { StreamsApi } from '$lib/api/streamsApi';
  import { connectRealtimeUpdatesStream, type RealtimeUpdateEvent } from '$lib/api/realtimeUpdates';
  import { imuQuaternionToThree } from '$lib/utils/imuFrames';
  import type { StreamInfo, StreamMetrics } from '$lib/ts-bindings/http/client';
  import type { LocalizationMarker, LocalizationViewMode } from '$lib';
  import type { PipelineTemplateSummary } from '$lib/types/pipeline';
  import type { RigCameraInfo, RobotDimensions } from '$lib/types/rig';
  import { DEFAULT_ROBOT_DIMENSIONS } from '$lib/3d/rig';
  import { rigLayoutStore } from '$lib/stores/rigLayout';
  import { formatMeters, parseLengthToMeters } from '$lib/utils/units';
  import SidebarSearchSection from '$lib/components/filters/SidebarSearchSection.svelte';
  import {
    composeTransforms,
    identityTransform,
    eulerDegreesToQuaternionXYZ,
    invertTransform,
    normalizeQuaternion,
    quaternionToEulerDegreesXYZ,
    yawDegreesToQuaternion,
    type PoseQuaternion,
    type PoseTransform,
    type Vec3
  } from '$lib/features/localization/poseMath';
  import { frcOriginDefinition, transformFromFieldCenter, type PlanarFieldOrigin } from '$lib/features/localization/fieldOrigins';
  import {
    fetchPipelineOutputSample,
    type LocalizationPipelineSource
  } from '$lib/features/localization/pipelineSources';
  import {
    type LocalizationPipelineStatus
  } from '$lib/features/localization/localizationPipeline';
  import {
    DEFAULT_FIELD_ORIGIN,
    DEFAULT_SOLVER_RUNTIME_TUNING,
    DEFAULT_TEMPORAL_STABILIZATION,
    fetchLocalizationSolve,
    type LocalizationCustomFieldOrigin,
    type LocalizationDetectionPose,
    type LocalizationFieldOriginConfig,
    type LocalizationFieldOriginMode,
    type LocalizationProfile,
    type LocalizationPoseSpace,
    type LocalizationSolveResponse,
    type LocalizationSolverConfig,
    type LocalizationSolverRuntimeTuningConfig,
    type LocalizationSolverOutputs,
    type LocalizationSolverResult,
    type LocalizationSourceSampleStatus,
    type LocalizationTemporalStabilizationConfig
  } from '$lib/features/localization/localizationConfig';
  import { createLocalizationProfileStore } from '$lib/features/localization/profileStore';
  import {
    fetchFieldMap,
    listFieldMaps,
    uploadLimelightFmap,
    type FieldMapDocument,
    type FieldMapSummary
  } from '$lib/features/localization/fieldMaps';
  import type { CameraExtrinsics, CustomField, CustomFieldOrigin } from '$lib/features/localization/types';
  import { createLocalizationStorageStore } from '$lib/features/localization/storage';
  import { createFeedPoller, pollIntervalMs } from '$lib/features/localization/feedPoller';
  import LocalizationWorkspace from '$lib/features/localization/page/LocalizationWorkspace.svelte';
  import {
    SOURCE_COLORS,
    PROFILE_COLORS,
    POSE_SPACE_OPTIONS,
    SOLVE_POSE_SPACES,
    DERIVED_POSE_SPACES,
    poseSpaceLabel,
    profileColorForId,
    isSourceCalibrated,
    cameraKeyForSource
  } from '$lib/features/localization/utils';
  import {
    toNumber,
    detectionPosesForSpace,
    markersFromDetections
  } from '$lib/features/localization/markerUtils';
  import { applyDeviceSeparation } from '$lib/features/localization/page/localizationPageUtils';
  import { removeStreamMetrics } from '$lib/features/localization/page/localizationMetricsUtils';
  import { createLocalizationPoseHelpers } from './localizationPoseHelpers';
  import { createLocalizationProfileActions } from './localizationProfileActions';
  import { createLocalizationSourceSelection } from './localizationSourceSelection';
  import { createLocalizationFieldMapActions } from './localizationFieldMapActions';
  import { createLocalizationFeedRuntime } from './localizationFeedRuntime';
  import { createLocalizationPageState } from '$lib/features/localization/page/LocalizationPageState';
  import { createLocalizationPageActions } from '$lib/features/localization/page/localizationPageActions';
  import {
    buildActiveCustomField,
    buildActiveFieldDimensions,
    buildActiveFieldMapBitsStatus,
    buildActiveFieldOrigin,
    buildSourceStatusRows,
    buildFieldSceneTransform,
    buildFieldSpaceLabel,
    buildViewProfileOverlays,
    buildViewerCameraTransforms,
    buildViewerCameras,
    buildViewerRobotTransform,
    buildViewerShowRobot,
    buildOriginFromFieldCenterForEditor,
    groupLocalizationSources,
    selectPrimarySource,
    type LocalizationBaseFrame
  } from '$lib/features/localization/page/localizationPageViewHelpers';
  import { estimateCalibrationFovDegs } from '$lib/features/devices/camera/cameraCalibrationUtils';

  type FeedStatus = 'idle' | 'connecting' | 'live' | 'error';

  type LocalizationCoordinateSpace = LocalizationPoseSpace;

  type RigLayoutViewState = {
    layout: { robot: RobotDimensions; cameras: RigCameraInfo[] };
    loading: boolean;
    error: string | null;
    initialized: boolean;
  };

  type CameraPovIntrinsics = {
    fx: number;
    fy: number;
    cx: number;
    cy: number;
    width: number;
    height: number;
  };

  type CameraPovFovMode = 'undistorted' | 'raw' | 'none';

  type CameraPovState = {
    optionId: string;
    profileId: string;
    sourceId: string;
    cameraKey: string | null;
    transform: PoseTransform;
    intrinsicsUndistorted: CameraPovIntrinsics | null;
    intrinsicsRaw: CameraPovIntrinsics | null;
  };

  type CameraPovOption = {
    id: string;
    profileId: string;
    sourceId: string;
    groupLabel: string;
    subgroupLabel: string;
    label: string;
    available: boolean;
    ghost: boolean;
  };

  type ViewerDetectionPose = LocalizationDetectionPose & { profileId: string; color: string | null };
  type LocalTagPoseOverlayEntry = {
    key: string;
    profileId: string;
    space: LocalizationPoseSpace;
    color: string | null;
    label: string;
    values: string;
    timestampMs: number;
  };
  type FieldSpacePoseOverlayEntry = {
    pose: PoseTransform;
    timestampMs: number;
  };
  type ImuRotationSample = {
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
  type ParsedImuRotation = {
    roll: number;
    pitch: number;
    yaw: number;
    quaternion: PoseQuaternion | null;
    translation: { x: number; y: number; z: number } | null;
    sampleTimestampMs: number | null;
  };
  const LOCAL_TAG_POSE_LINGER_MS = 5000;
  const LOCAL_TAG_POSE_REFRESH_MS = 250;
  const FIELD_POSE_LINGER_MS = 5000;
  const FIELD_POSE_REFRESH_MS = 250;
  const LIVE_UPDATES_RECONNECT_MS = 1_500;
  const LIVE_UPDATES_REFRESH_DEBOUNCE_MS = 400;
  const LIVE_SOURCES_REFRESH_MIN_INTERVAL_MS = 5_000;

  const bumperId = '0000';
  let sources = $state<LocalizationPipelineSource[]>([]);
  let sourcesLoading = $state(false);
  let sourcesError = $state<string | null>(null);
  let sourceCompatibility = $state<Record<string, boolean>>({});
  let selectedSourceIds = $state<string[]>([]);
  let primarySourceId = $state<string | null>(null);
  const localizationProfiles = createLocalizationProfileStore();
  const localizationConfig = localizationProfiles.config;
  const localizationConfigLoading = localizationProfiles.loading;
  const localizationConfigError = localizationProfiles.error;
  const activeProfileId = localizationProfiles.activeProfileId;
  const profiles = localizationProfiles.profiles;
  const activeProfile = localizationProfiles.activeProfile;
  let solveResponse = $state<LocalizationSolveResponse | null>(null);
  let solveResponsesByProfile = $state<Record<string, LocalizationSolveResponse>>({});
  let pipelineTemplates = $state<PipelineTemplateSummary[]>([]);
  let pipelineTemplatesLoading = $state(false);
  let pipelineTemplatesError = $state<string | null>(null);
  let pipelineStatus = $state<LocalizationPipelineStatus | null>(null);
  let pipelineStatusLoading = $state(false);
  let pipelineStatusError = $state<string | null>(null);
  let pipelineOutputs = $state<string[]>([]);
  let pipelineOutputsLoading = $state(false);
  let pipelineOutputsError = $state<string | null>(null);

  let feedStatus = $state<FeedStatus>('idle');
  let feedMessage = $state<string | null>(null);
  let pollHz = $state(30);
  let lastPollMs = $state<number | null>(null);
  let rawMarkers = $state<LocalizationMarker[]>([]);
  let liveMarkers = $state<LocalizationMarker[]>([]);
  let pollResults = $state<LocalizationSourceSampleStatus[]>([]);

  let viewMode = $state<LocalizationViewMode>('isolated');
  let fieldOriginMode = $state<LocalizationFieldOriginMode>('blue');
  // Prefer robot-space poses by default so IMU-leveling is visible in the 3D viewer.
  // If the active solver doesn't provide robot-space outputs, the selection effect below
  // will fall back to the first available pose space.
  let coordinateSpace = $state<LocalizationCoordinateSpace>('tag_in_robot');
  const separateCameras = true;
  const showRobotContext = true;
  let showOriginAxes = $state(true);
  let showTagLines = $state(false);
  let showFieldImage = $state(true);
  let showMinimapTrail = $state(true);
  let showCameraPoseOverlay = $state(false);
  let showCustomFieldsOverlay = $state(false);
  let showImuRotationOverlay = $state(false);
  let showOutputsOverlay = $state(false);
  let showMetricsOverlay = $state(false);
  let profileSearch = $state('');
  let pendingProfileDelete = $state<{ id: string; name: string } | null>(null);
  let profileDeleteBusy = $state(false);
  let profileDeleteError = $state<string | null>(null);

  const localizationStorage = createLocalizationStorageStore();
  const cameraExtrinsics = localizationStorage.cameraExtrinsics;
  let cameraPoseEditorError = $state<string | null>(null);
  let cameraPoseXInput = $state('');
  let cameraPoseYInput = $state('');
  let cameraPoseZInput = $state('');
  let cameraPoseRollDeg = $state('0');
  let cameraPosePitchDeg = $state('0');
  let cameraPoseYawDeg = $state('0');

  const {
    persistLocalizationConfig,
    setProfileColor,
    setProfileViewEnabled,
    persistProfileUpdate,
    setActiveProfile,
    addProfile,
    removeActiveProfile,
    commitProfileName,
    commitTagSize
  } = createLocalizationProfileActions({
    profiles: () => $profiles,
    activeProfile: () => $activeProfile ?? null,
    profileNameInput: () => profileNameInput,
    tagSizeInput: () => tagSizeInput,
    setTagSizeError: (message) => {
      tagSizeError = message;
    },
    parseLengthToMeters,
    localizationProfiles
  });

  const removeProfileById = async (profileId: string): Promise<void> => {
    if ($profiles.length <= 1) return;
    const currentActiveId = $activeProfile?.id ?? $activeProfileId ?? null;
    if (currentActiveId !== profileId) {
      await setActiveProfile(profileId);
    }
    await removeActiveProfile();
  };

  const openDeleteProfileModal = (profile: LocalizationProfile): void => {
    if ($profiles.length <= 1) return;
    pendingProfileDelete = {
      id: profile.id,
      name: (profile.name ?? '').trim() || profile.id
    };
    profileDeleteError = null;
  };

  const closeDeleteProfileModal = (): void => {
    if (profileDeleteBusy) return;
    pendingProfileDelete = null;
    profileDeleteError = null;
  };

  const confirmDeleteProfile = async (): Promise<void> => {
    const target = pendingProfileDelete;
    if (!target || profileDeleteBusy) return;
    profileDeleteBusy = true;
    profileDeleteError = null;
    try {
      await removeProfileById(target.id);
      pendingProfileDelete = null;
    } catch (error) {
      const description = error instanceof Error ? error.message : 'Profile deletion failed';
      profileDeleteError = description;
      toaster.error({
        title: 'Unable to remove profile',
        description
      });
    } finally {
      profileDeleteBusy = false;
    }
  };

  const normalizeFieldOriginConfig = (
    value?: LocalizationFieldOriginConfig | null
  ): LocalizationFieldOriginConfig => {
    const fallback = { ...DEFAULT_FIELD_ORIGIN };
    if (!value || typeof value !== 'object') return fallback;
    const mode = value.mode ?? fallback.mode;
    const validMode: LocalizationFieldOriginMode =
      mode === 'center' || mode === 'blue' || mode === 'red' || mode === 'custom'
        ? mode
        : fallback.mode;
    const custom = value.custom;
    const hasFiniteCustom =
      custom &&
      Number.isFinite(custom.x) &&
      Number.isFinite(custom.z) &&
      Number.isFinite(custom.yawDeg);
    return {
      mode: validMode,
      custom: hasFiniteCustom
        ? {
            x: Number(custom.x),
            z: Number(custom.z),
            yawDeg: Number(custom.yawDeg)
          }
        : null
    };
  };

  const setProfileFieldOriginMode = (mode: LocalizationFieldOriginMode): void => {
    const profile = $activeProfile ?? null;
    if (!profile) return;
    const current = normalizeFieldOriginConfig(profile.fieldOrigin);
    const nextCustom =
      mode === 'custom'
        ? current.custom ?? { x: 0, z: 0, yawDeg: 0 }
        : current.custom ?? null;
    const sameCustom =
      (current.custom == null && nextCustom == null) ||
      (current.custom != null &&
        nextCustom != null &&
        current.custom.x === nextCustom.x &&
        current.custom.z === nextCustom.z &&
        current.custom.yawDeg === nextCustom.yawDeg);
    if (current.mode === mode && sameCustom) return;
    void persistProfileUpdate({
      ...profile,
      fieldOrigin: { mode, custom: nextCustom }
    });
  };

  const setProfileFieldOriginCustomNumeric = (
    field: keyof LocalizationCustomFieldOrigin,
    rawValue: string
  ): void => {
    const profile = $activeProfile ?? null;
    if (!profile) return;
    const parsed = Number(rawValue);
    if (!Number.isFinite(parsed)) return;
    const current = normalizeFieldOriginConfig(profile.fieldOrigin);
    const currentCustom = current.custom ?? { x: 0, z: 0, yawDeg: 0 };
    if (currentCustom[field] === parsed && current.mode === 'custom') return;
    void persistProfileUpdate({
      ...profile,
      fieldOrigin: {
        mode: 'custom',
        custom: { ...currentCustom, [field]: parsed }
      }
    });
  };

  const setSnapZToGround = (enabled: boolean): void => {
    const profile = $activeProfile ?? null;
    if (!profile) return;
    if (Boolean(profile.snapZToGround) === enabled) return;
    void persistProfileUpdate({ ...profile, snapZToGround: enabled });
  };

  const setSnapRollToGround = (enabled: boolean): void => {
    const profile = $activeProfile ?? null;
    if (!profile) return;
    if (Boolean(profile.snapRollToGround) === enabled) return;
    void persistProfileUpdate({ ...profile, snapRollToGround: enabled });
  };

  const setSnapPitchToGround = (enabled: boolean): void => {
    const profile = $activeProfile ?? null;
    if (!profile) return;
    if (Boolean(profile.snapPitchToGround) === enabled) return;
    void persistProfileUpdate({ ...profile, snapPitchToGround: enabled });
  };

  const temporalFieldParsers = {
    singleTagTranslationAlpha: { min: 0, max: 1 },
    singleTagRotationAlpha: { min: 0, max: 1 },
    multiTagTranslationAlpha: { min: 0, max: 1 },
    multiTagRotationAlpha: { min: 0, max: 1 },
    maxTranslationJumpM: { min: 0.01, max: 50 },
    maxRotationJumpDeg: { min: 0.1, max: 180 },
    reanchorRejectWindowMs: { min: 50, max: 5000, integer: true }
  } as const;
  type TemporalNumericField = keyof typeof temporalFieldParsers;

  const isTemporalNumericField = (field: string): field is TemporalNumericField =>
    Object.prototype.hasOwnProperty.call(temporalFieldParsers, field);

  const normalizeTemporalSettings = (
    settings?: LocalizationTemporalStabilizationConfig | null
  ): LocalizationTemporalStabilizationConfig => {
    const merged = { ...DEFAULT_TEMPORAL_STABILIZATION, ...(settings ?? {}) };
    const clamp = (value: number, min: number, max: number): number =>
      Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : min;
    return {
      enabled: Boolean(merged.enabled),
      singleTagTranslationAlpha: clamp(merged.singleTagTranslationAlpha, 0, 1),
      singleTagRotationAlpha: clamp(merged.singleTagRotationAlpha, 0, 1),
      multiTagTranslationAlpha: clamp(merged.multiTagTranslationAlpha, 0, 1),
      multiTagRotationAlpha: clamp(merged.multiTagRotationAlpha, 0, 1),
      maxTranslationJumpM: clamp(merged.maxTranslationJumpM, 0.01, 50),
      maxRotationJumpDeg: clamp(merged.maxRotationJumpDeg, 0.1, 180),
      reanchorRejectWindowMs: Math.round(clamp(merged.reanchorRejectWindowMs, 50, 5000))
    };
  };

  const parseTemporalNumericValue = (field: TemporalNumericField, rawValue: string): number | null => {
    const parsed = Number(rawValue);
    if (!Number.isFinite(parsed)) return null;
    const bounds = temporalFieldParsers[field];
    const { min, max } = bounds;
    const integer = 'integer' in bounds ? Boolean(bounds.integer) : false;
    const clamped = Math.min(max, Math.max(min, parsed));
    return integer ? Math.round(clamped) : clamped;
  };

  const setProfileTemporalEnabled = (enabled: boolean): void => {
    const profile = $activeProfile ?? null;
    if (!profile) return;
    const current = normalizeTemporalSettings(profile.temporalStabilization);
    if (current.enabled === enabled) return;
    void persistProfileUpdate({
      ...profile,
      temporalStabilization: { ...current, enabled }
    });
  };

  const setProfileTemporalNumeric = (field: string, rawValue: string): void => {
    if (!isTemporalNumericField(field)) return;
    const profile = $activeProfile ?? null;
    if (!profile) return;
    const nextValue = parseTemporalNumericValue(field, rawValue);
    if (nextValue == null) return;
    const current = normalizeTemporalSettings(profile.temporalStabilization);
    if (current[field] === nextValue) return;
    void persistProfileUpdate({
      ...profile,
      temporalStabilization: { ...current, [field]: nextValue }
    });
  };

  const setSolverTemporalOverrideEnabled = (enabled: boolean): void => {
    const profile = $activeProfile ?? null;
    const solver = activeSolverConfig ?? null;
    if (!profile || !solver) return;

    const nextSolvers = profile.solvers.map((entry) => {
      if (entry.id !== solver.id) return entry;
      if (enabled) {
        const base = normalizeTemporalSettings(entry.temporalStabilization ?? profile.temporalStabilization);
        return { ...entry, temporalStabilization: base };
      }
      return { ...entry, temporalStabilization: null };
    });
    void persistProfileUpdate({ ...profile, solvers: nextSolvers });
  };

  const setSolverTemporalEnabled = (enabled: boolean): void => {
    const profile = $activeProfile ?? null;
    const solver = activeSolverConfig ?? null;
    if (!profile || !solver) return;
    if (!solver.temporalStabilization) return;
    const current = normalizeTemporalSettings(solver.temporalStabilization);
    if (current.enabled === enabled) return;
    const nextSolvers = profile.solvers.map((entry) =>
      entry.id === solver.id ? { ...entry, temporalStabilization: { ...current, enabled } } : entry
    );
    void persistProfileUpdate({ ...profile, solvers: nextSolvers });
  };

  const setSolverTemporalNumeric = (field: string, rawValue: string): void => {
    if (!isTemporalNumericField(field)) return;
    const profile = $activeProfile ?? null;
    const solver = activeSolverConfig ?? null;
    if (!profile || !solver) return;
    if (!solver.temporalStabilization) return;
    const nextValue = parseTemporalNumericValue(field, rawValue);
    if (nextValue == null) return;
    const current = normalizeTemporalSettings(solver.temporalStabilization);
    if (current[field] === nextValue) return;
    const nextSolvers = profile.solvers.map((entry) =>
      entry.id === solver.id
        ? { ...entry, temporalStabilization: { ...current, [field]: nextValue } }
        : entry
    );
    void persistProfileUpdate({ ...profile, solvers: nextSolvers });
  };

  const solverRuntimeFieldParsers = {
    minObservationWeight: { min: 0, max: 5 },
    minSingleTagSolveWeight: { min: 0, max: 5 },
    minMultiTagTotalWeight: { min: 0, max: 20 },
    minMultiTagEffectiveCount: { min: 0, max: 20 },
    weakSingleTagMargin: { min: 0, max: 5 },
    coplanarHeightDeltaM: { min: 0, max: 5 },
    severeObservedHeightDeltaM: { min: 0, max: 10 },
    moderateObservedHeightDeltaM: { min: 0, max: 10 },
    mildObservedHeightDeltaM: { min: 0, max: 10 },
    severePenalty: { min: 0, max: 1 },
    moderatePenalty: { min: 0, max: 1 },
    mildPenalty: { min: 0, max: 1 },
    dtScaleMin: { min: 0.01, max: 20 },
    dtScaleMax: { min: 0.01, max: 20 },
    switchedSingleTagMaxTranslationJumpM: { min: 0.001, max: 50 },
    switchedSingleTagMaxRotationJumpDeg: { min: 0.01, max: 180 },
    droppedMultiToSingleMaxTranslationJumpM: { min: 0.001, max: 50 },
    droppedMultiToSingleMaxRotationJumpDeg: { min: 0.01, max: 180 },
    switchedSingleTagRejectWindowScale: { min: 0.1, max: 10 },
    switchedSingleTagRejectWindowMinMs: { min: 0, max: 20000, integer: true },
    droppedMultiToSingleRejectWindowScale: { min: 0.1, max: 10 },
    droppedMultiToSingleRejectWindowMinMs: { min: 0, max: 20000, integer: true },
    switchedSingleTagGainDamp: { min: 0, max: 1 },
    switchedSingleTagMinTranslationGain: { min: 0, max: 1 },
    switchedSingleTagMinRotationGain: { min: 0, max: 1 },
    droppedMultiToSingleGainDamp: { min: 0, max: 1 },
    droppedMultiToSingleMinTranslationGain: { min: 0, max: 1 },
    droppedMultiToSingleMinRotationGain: { min: 0, max: 1 }
  } as const;
  type SolverRuntimeNumericField = keyof typeof solverRuntimeFieldParsers;

  const isSolverRuntimeNumericField = (field: string): field is SolverRuntimeNumericField =>
    Object.prototype.hasOwnProperty.call(solverRuntimeFieldParsers, field);

  const normalizeSolverRuntimeTuning = (
    settings?: LocalizationSolverRuntimeTuningConfig | null
  ): LocalizationSolverRuntimeTuningConfig => {
    const merged = { ...DEFAULT_SOLVER_RUNTIME_TUNING, ...(settings ?? {}) };
    const clamp = (value: number, min: number, max: number): number =>
      Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : min;
    const dtScaleMin = clamp(merged.dtScaleMin, 0.01, 20);
    const dtScaleMax = Math.max(dtScaleMin, clamp(merged.dtScaleMax, 0.01, 20));
    return {
      minObservationWeight: clamp(merged.minObservationWeight, 0, 5),
      minSingleTagSolveWeight: clamp(merged.minSingleTagSolveWeight, 0, 5),
      minMultiTagTotalWeight: clamp(merged.minMultiTagTotalWeight, 0, 20),
      minMultiTagEffectiveCount: clamp(merged.minMultiTagEffectiveCount, 0, 20),
      weakSingleTagMargin: clamp(merged.weakSingleTagMargin, 0, 5),
      coplanarHeightDeltaM: clamp(merged.coplanarHeightDeltaM, 0, 5),
      severeObservedHeightDeltaM: clamp(merged.severeObservedHeightDeltaM, 0, 10),
      moderateObservedHeightDeltaM: clamp(merged.moderateObservedHeightDeltaM, 0, 10),
      mildObservedHeightDeltaM: clamp(merged.mildObservedHeightDeltaM, 0, 10),
      severePenalty: clamp(merged.severePenalty, 0, 1),
      moderatePenalty: clamp(merged.moderatePenalty, 0, 1),
      mildPenalty: clamp(merged.mildPenalty, 0, 1),
      dtScaleMin,
      dtScaleMax,
      switchedSingleTagMaxTranslationJumpM: clamp(merged.switchedSingleTagMaxTranslationJumpM, 0.001, 50),
      switchedSingleTagMaxRotationJumpDeg: clamp(merged.switchedSingleTagMaxRotationJumpDeg, 0.01, 180),
      droppedMultiToSingleMaxTranslationJumpM: clamp(merged.droppedMultiToSingleMaxTranslationJumpM, 0.001, 50),
      droppedMultiToSingleMaxRotationJumpDeg: clamp(merged.droppedMultiToSingleMaxRotationJumpDeg, 0.01, 180),
      switchedSingleTagRejectWindowScale: clamp(merged.switchedSingleTagRejectWindowScale, 0.1, 10),
      switchedSingleTagRejectWindowMinMs: Math.round(clamp(merged.switchedSingleTagRejectWindowMinMs, 0, 20000)),
      droppedMultiToSingleRejectWindowScale: clamp(merged.droppedMultiToSingleRejectWindowScale, 0.1, 10),
      droppedMultiToSingleRejectWindowMinMs: Math.round(clamp(merged.droppedMultiToSingleRejectWindowMinMs, 0, 20000)),
      switchedSingleTagGainDamp: clamp(merged.switchedSingleTagGainDamp, 0, 1),
      switchedSingleTagMinTranslationGain: clamp(merged.switchedSingleTagMinTranslationGain, 0, 1),
      switchedSingleTagMinRotationGain: clamp(merged.switchedSingleTagMinRotationGain, 0, 1),
      droppedMultiToSingleGainDamp: clamp(merged.droppedMultiToSingleGainDamp, 0, 1),
      droppedMultiToSingleMinTranslationGain: clamp(merged.droppedMultiToSingleMinTranslationGain, 0, 1),
      droppedMultiToSingleMinRotationGain: clamp(merged.droppedMultiToSingleMinRotationGain, 0, 1)
    };
  };

  const parseSolverRuntimeNumericValue = (field: SolverRuntimeNumericField, rawValue: string): number | null => {
    const parsed = Number(rawValue);
    if (!Number.isFinite(parsed)) return null;
    const bounds = solverRuntimeFieldParsers[field];
    const { min, max } = bounds;
    const integer = 'integer' in bounds ? Boolean(bounds.integer) : false;
    const clamped = Math.min(max, Math.max(min, parsed));
    return integer ? Math.round(clamped) : clamped;
  };

  const setSolverRuntimeTuningNumeric = (field: string, rawValue: string): void => {
    if (!isSolverRuntimeNumericField(field)) return;
    const profile = $activeProfile ?? null;
    const solver = activeSolverConfig ?? null;
    if (!profile || !solver) return;
    const nextValue = parseSolverRuntimeNumericValue(field, rawValue);
    if (nextValue == null) return;
    const current = normalizeSolverRuntimeTuning(solver.runtimeTuning);
    const nextRuntime = normalizeSolverRuntimeTuning({ ...current, [field]: nextValue });
    if (current[field] === nextRuntime[field]) return;
    const nextSolvers = profile.solvers.map((entry) =>
      entry.id === solver.id
        ? { ...entry, runtimeTuning: nextRuntime }
        : entry
    );
    void persistProfileUpdate({ ...profile, solvers: nextSolvers });
  };

  const {
    createCustomField,
    loadFieldMapList,
    ensureFieldMapLoaded,
    createCustomFieldFromMap,
    assignMapToSelectedField,
    handleMapUploadFile,
    uploadSelectedMapFile,
    addOriginToSelectedField
  } = createLocalizationFieldMapActions({
    getCustomFields: () => $customFields,
    setCustomFields: (next) => localizationStorage.setCustomFields(next),
    getSelectedCustomFieldId: () => selectedCustomFieldId,
    setSelectedCustomFieldId: (next) => {
      selectedCustomFieldId = next;
    },
    setSelectedCustomFieldOriginId: (next) => {
      selectedCustomFieldOriginId = next;
    },
    getSelectedCustomField: () => selectedCustomField,
    setNewCustomFieldError: (message) => {
      newCustomFieldError = message;
    },
    getNewCustomFieldName: () => newCustomFieldName,
    getNewCustomFieldWidth: () => newCustomFieldWidth,
    getNewCustomFieldDepth: () => newCustomFieldDepth,
    parseLengthToMeters,
    setViewMode: (mode) => {
      viewMode = mode;
    },
    getActiveProfile: () => $activeProfile ?? null,
    persistProfileUpdate,
    updateProfileFieldMapId: (profile, mapId) => ({ ...profile, fieldMapId: mapId }),
    fetchFieldMap,
    listFieldMaps,
    uploadLimelightFmap,
    getFieldMaps: () => fieldMaps,
    setFieldMaps: (next) => {
      fieldMaps = next;
    },
    getFieldMapDocs: () => fieldMapDocs,
    setFieldMapDocs: (next) => {
      fieldMapDocs = next;
    },
    getFieldMapDocErrors: () => fieldMapDocErrors,
    setFieldMapDocErrors: (next) => {
      fieldMapDocErrors = next;
    },
    setFieldMapsLoading: (loading) => {
      fieldMapsLoading = loading;
    },
    setFieldMapsError: (message) => {
      fieldMapsError = message;
    },
    getMapUploadFile: () => mapUploadFile,
    setMapUploadFile: (file) => {
      mapUploadFile = file;
    },
    setMapUploadBusy: (busy) => {
      mapUploadBusy = busy;
    },
    setMapUploadError: (message) => {
      mapUploadError = message;
    },
    setMapUploadSuccess: (summary) => {
      fieldMaps = [...fieldMaps, summary];
    },
    toaster,
    getNewOriginName: () => newOriginName,
    getNewOriginX: () => newOriginX,
    getNewOriginZ: () => newOriginZ,
    getNewOriginYaw: () => newOriginYaw,
    setNewOriginError: (message) => {
      newOriginError = message;
    },
    toNumber
  });

  const { runFeedPoll } = createLocalizationFeedRuntime({
    getActiveProfile: () => $activeProfile ?? null,
    getHasAnyFeedSources: () => hasAnyFeedSources,
    getActiveHasSources: () => activeHasSources,
    getViewProfilesWithSources: () => viewProfilesWithSources,
    fetchLocalizationSolve,
    setSolveResponsesByProfile: (next) => {
      solveResponsesByProfile = next;
    },
    setSolveResponse: (next) => {
      solveResponse = next;
    },
    setPollResults: (next) => {
      pollResults = next;
    },
    setFeedStatus: (next) => {
      feedStatus = next;
    },
    setFeedMessage: (next) => {
      feedMessage = next;
    },
    setLastPollMs: (next) => {
      lastPollMs = next;
    }
  });

  const {
    markerQuaternion,
    cameraExtrinsicsTransform,
    rigCameraForSource,
    applyPrimaryCameraPose,
    resetPrimaryCameraPoseInputs
  } = createLocalizationPoseHelpers({
    getCameraExtrinsics: () => $cameraExtrinsics,
    setCameraExtrinsics: (next) => localizationStorage.setCameraExtrinsics(next),
    getPrimaryCameraKey: () => primaryCameraKey,
    getRigCameras: () => rigLayoutState.layout.cameras,
    getOriginFromFieldCenterForEditor: () => originFromFieldCenterForEditor,
    getCameraPoseInputs: () => ({
      x: cameraPoseXInput,
      y: cameraPoseYInput,
      z: cameraPoseZInput,
      roll: cameraPoseRollDeg,
      pitch: cameraPosePitchDeg,
      yaw: cameraPoseYawDeg
    }),
    setCameraPoseInputs: (next) => {
      cameraPoseXInput = next.x;
      cameraPoseYInput = next.y;
      cameraPoseZInput = next.z;
      cameraPoseRollDeg = next.roll;
      cameraPosePitchDeg = next.pitch;
      cameraPoseYawDeg = next.yaw;
    },
    setCameraPoseEditorError: (message) => {
      cameraPoseEditorError = message;
    },
    parseLengthToMeters,
    formatMeters,
    toNumber,
    normalizeQuaternion,
    yawDegreesToQuaternion,
    eulerDegreesToQuaternionXYZ,
    quaternionToEulerDegreesXYZ,
    invertTransform,
    composeTransforms
  });
  let applySourceSelectionImpl: ((nextIds: string[]) => void) | null = null;
  const applySourceSelection = (nextIds: string[]): void => {
    applySourceSelectionImpl?.(nextIds);
  };

  const {
    loadLocalizationConfig,
    loadPipelineTemplates,
    loadPipelineStatus,
    loadPipelineOutputs,
    loadSources,
    loadLocalizationViewers
  } = createLocalizationPageState({
    localizationProfiles,
    setFeedStatus: (next) => {
      feedStatus = next;
    },
    setFeedMessage: (next) => {
      feedMessage = next;
    },
    setPipelineTemplates: (next) => {
      pipelineTemplates = next;
    },
    setPipelineTemplatesLoading: (next) => {
      pipelineTemplatesLoading = next;
    },
    setPipelineTemplatesError: (next) => {
      pipelineTemplatesError = next;
    },
    setPipelineStatus: (next) => {
      pipelineStatus = next;
    },
    setPipelineStatusLoading: (next) => {
      pipelineStatusLoading = next;
    },
    setPipelineStatusError: (next) => {
      pipelineStatusError = next;
    },
    setPipelineOutputs: (next) => {
      pipelineOutputs = next;
    },
    setPipelineOutputsLoading: (next) => {
      pipelineOutputsLoading = next;
    },
    setPipelineOutputsError: (next) => {
      pipelineOutputsError = next;
    },
    setSources: (next) => {
      sources = next;
    },
    setSourcesLoading: (next) => {
      sourcesLoading = next;
    },
    setSourcesError: (next) => {
      sourcesError = next;
    },
    setSourceCompatibility: (next) => {
      sourceCompatibility = next;
    },
    getSelectedSourceIds: () => selectedSourceIds,
    applySourceSelection,
    toaster,
    isBrowser: browser,
    getLocalizationViewersComponent: () => LocalizationViewersComponent,
    setLocalizationViewersComponent: (next) => {
      LocalizationViewersComponent =
        next as (typeof import('$lib/components/LocalizationViewers.svelte'))['default'];
    }
  });
  let cameraPoseInputsKey: string | null = null;

  const customFields = localizationStorage.customFields;
  let selectedCustomFieldId = $state<string | null>(null);
  let selectedCustomFieldOriginId = $state<string | null>(null);
  let newCustomFieldName = $state('Custom field');
  let newCustomFieldWidth = $state('');
  let newCustomFieldDepth = $state('');
  let newCustomFieldError = $state<string | null>(null);
  let newOriginName = $state('Origin');
  let newOriginX = $state('0m');
  let newOriginZ = $state('0m');
  let newOriginYaw = $state('0');
  let newOriginError = $state<string | null>(null);

  let fieldMaps = $state<FieldMapSummary[]>([]);
  let fieldMapsLoading = $state(false);
  let fieldMapsError = $state<string | null>(null);
  let fieldMapDocs = $state<Record<string, FieldMapDocument>>({});
  let fieldMapDocErrors = $state<Record<string, string>>({});
  let mapUploadFile: File | null = $state(null);
  let mapUploadError = $state<string | null>(null);
  let mapUploadBusy = $state(false);
  let mapAssignId = $state<string>('');
  let profileNameInput = $state('');
  let profileNameTargetId = $state<string | null>(null);
  let solverNameInput = $state('');
  let solverNameTargetKey = $state<string | null>(null);
  let tagSizeInput = $state('');
  let tagSizeTargetId = $state<string | null>(null);
  let tagSizeError = $state<string | null>(null);
  let fieldMapSelection = $state('');
  let openSourceGroups = $state<string[]>([]);
  let streamInfos = $state<StreamInfo[]>([]);
  let cameraPovSelectionId = $state('');
  let cameraPovFovMode = $state<CameraPovFovMode>('undistorted');
  const ROBOT_FOLLOW_POV_OPTION_ID = '__robot_follow__';
  let lastCameraPovByOptionId = $state<Record<string, CameraPovState>>({});
  let lastFieldSpacePoseByProfileSpace = $state<Record<string, FieldSpacePoseOverlayEntry>>({});
  let lastLocalTagPoseByKey = $state<Record<string, LocalTagPoseOverlayEntry>>({});
  let lastViewerCameraTransforms = $state<Record<string, { position: Vec3; quaternion?: PoseQuaternion }> | null>(null);
  let lastViewerRenderableCameras = $state<RigCameraInfo[]>([]);
  let imuRotationSample = $state<ImuRotationSample | null>(null);
  let imuRotationError = $state<string | null>(null);

  let streamMetricsById = $state<Record<string, StreamMetrics>>({});
  let streamMetricsUpdatedAtById = $state<Record<string, number>>({});
  let streamMetricsErrorById = $state<Record<string, string>>({});
  const streamMetricsCleanup = new Map<string, () => void>();

  let pollVisibilityPaused = false;
  let visibilityHandler: (() => void) | null = null;
  let liveUpdatesCleanup: (() => void) | null = null;
  let liveUpdatesReconnectHandle: number | null = null;
  let liveUpdatesRefreshHandle: number | null = null;
  let liveUpdatesRefreshSourcesPending = false;
  let lastLiveSourcesRefreshAtMs = 0;
  let liveUpdatesNonce = 0;

  let LocalizationViewersComponent = $state<
    (typeof import('$lib/components/LocalizationViewers.svelte'))['default'] | null
  >(null);

  let rigLayoutState = $state<RigLayoutViewState>({
    layout: { robot: { ...DEFAULT_ROBOT_DIMENSIONS }, cameras: [] },
    loading: false,
    error: null,
    initialized: false
  });

  const rigLayoutUnsubscribe = rigLayoutStore.subscribe((state) => {
    rigLayoutState = state;
  });

  function computeViewProfiles(profiles: LocalizationProfile[], activeProfile: LocalizationProfile | null): LocalizationProfile[] {
    if (profiles.length === 0) return [];
    const hasExplicit = profiles.some((profile) => typeof profile.viewEnabled === 'boolean');
    if (!hasExplicit) {
      // Legacy configs may not have `viewEnabled`; default to showing the active profile until persisted.
      return activeProfile ? [activeProfile] : [];
    }
    // Strict: only explicitly enabled profiles drive 3D marker visibility.
    return profiles.filter((profile) => profile.viewEnabled === true);
  }

  function sourceStreamOutputKey(streamId: string | null | undefined, outputKey: string | null | undefined): string {
    const stream = String(streamId ?? '').trim();
    const output = String(outputKey ?? '').trim();
    if (!stream || !output) return '';
    return `${stream}::${output}`;
  }

  function computeViewOverlaySources(
    profiles: LocalizationProfile[],
    activeProfile: LocalizationProfile | null,
    sources: LocalizationPipelineSource[]
  ): LocalizationPipelineSource[] {
    const viewProfiles = computeViewProfiles(profiles, activeProfile);
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

  function recordFromUnknown(value: unknown): Record<string, unknown> | null {
    return value && typeof value === 'object' ? (value as Record<string, unknown>) : null;
  }

  function finiteNumber(value: unknown): number | null {
    const parsed = toNumber(value, Number.NaN);
    return Number.isFinite(parsed) ? parsed : null;
  }

  function parseQuaternionLike(value: unknown): PoseQuaternion | null {
    const record = recordFromUnknown(value);
    if (!record) return null;
    const x = finiteNumber(record.x);
    const y = finiteNumber(record.y);
    const z = finiteNumber(record.z);
    const w = finiteNumber(record.w);
    if (x == null || y == null || z == null || w == null) return null;
    return { x, y, z, w };
  }

  function parseEulerLike(value: unknown): { roll: number; pitch: number; yaw: number } | null {
    const record = recordFromUnknown(value);
    if (!record) return null;
    const roll = finiteNumber(record.roll);
    const pitch = finiteNumber(record.pitch);
    const yaw = finiteNumber(record.yaw);
    if (roll != null && pitch != null && yaw != null) {
      return { roll, pitch, yaw };
    }

    // Some external mappings emit xyz Euler values instead of roll/pitch/yaw.
    if (!('w' in record)) {
      const x = finiteNumber(record.x);
      const y = finiteNumber(record.y);
      const z = finiteNumber(record.z);
      if (x != null && y != null && z != null) {
        return { roll: x, pitch: y, yaw: z };
      }
    }
    return null;
  }

  function parseTranslationLike(value: unknown): { x: number; y: number; z: number } | null {
    const record = recordFromUnknown(value);
    if (!record) return null;
    const x = finiteNumber(record.x);
    const y = finiteNumber(record.y);
    const z = finiteNumber(record.z);
    if (x == null || y == null || z == null) return null;
    return { x, y, z };
  }

  function parseImuRotationSample(value: unknown): ParsedImuRotation | null {
    const root = recordFromUnknown(value);
    if (!root) return null;
    const pose = recordFromUnknown(root.pose);
    const imu = recordFromUnknown(root.imu);

    const rotationCandidates: unknown[] = [
      root.rotation,
      root.orientation,
      pose?.rotation,
      pose?.orientation,
      imu?.rotation,
      imu?.orientation,
      root
    ];

    let roll: number | null = null;
    let pitch: number | null = null;
    let yaw: number | null = null;
    let quaternion: PoseQuaternion | null = null;
    for (const candidate of rotationCandidates) {
      const euler = parseEulerLike(candidate);
      const candidateRecord = recordFromUnknown(candidate);
      const quat = parseQuaternionLike(candidateRecord?.quaternion ?? candidate);
      if (euler) {
        roll = euler.roll;
        pitch = euler.pitch;
        yaw = euler.yaw;
        quaternion = quat;
        break;
      }
      if (quat) {
        const eulerFromQuat = quaternionToEulerDegreesXYZ(quat);
        roll = eulerFromQuat.roll;
        pitch = eulerFromQuat.pitch;
        yaw = eulerFromQuat.yaw;
        quaternion = quat;
        break;
      }
    }
    if (roll == null || pitch == null || yaw == null) return null;

    const translation =
      parseTranslationLike(root.translation) ??
      parseTranslationLike(root.position) ??
      parseTranslationLike(pose?.translation) ??
      parseTranslationLike(imu?.translation);

    const meta = recordFromUnknown(root.meta);
    const sampleTimestampMs =
      finiteNumber(root.timestampMs) ??
      finiteNumber(root.timestamp_ms) ??
      finiteNumber(root.sampleTimestampMs) ??
      finiteNumber(root.sample_timestamp_ms) ??
      finiteNumber(root.t_ms) ??
      finiteNumber(root.time_ms) ??
      finiteNumber(meta?.timestampMs) ??
      finiteNumber(meta?.timestamp_ms) ??
      null;

    return {
      roll,
      pitch,
      yaw,
      quaternion,
      translation,
      sampleTimestampMs
    };
  }

  function hasImuToken(value: string | null | undefined): boolean {
    const normalized = typeof value === 'string' ? value.trim().toLowerCase() : '';
    if (!normalized) return false;
    return normalized.split(/[^a-z0-9]+/).some((token) => token === 'imu');
  }

  function isImuSource(source: LocalizationPipelineSource): boolean {
    return (
      source.streamId.startsWith('external:media-imu-') ||
      hasImuToken(source.streamId) ||
      hasImuToken(source.streamLabel) ||
      hasImuToken(source.cameraUid) ||
      hasImuToken(source.cameraPath) ||
      hasImuToken(source.pipelineLabel) ||
      hasImuToken(source.outputKey)
    );
  }

  function imuSourcePriority(source: LocalizationPipelineSource): number {
    if (source.streamId.startsWith('external:media-imu-')) return 0;
    if (source.streamId === 'external:imu') return 1;
    if (source.streamId.startsWith('external:') && hasImuToken(source.streamLabel)) return 2;
    return 3;
  }

  function imuSampleQuaternion(sample: ImuRotationSample | null): PoseQuaternion | null {
    if (!sample) return null;
    const fromSample = sample.quaternion;
    if (
      fromSample &&
      [fromSample.x, fromSample.y, fromSample.z, fromSample.w].every(
        (value) => typeof value === 'number' && Number.isFinite(value)
      )
    ) {
      const viewerQuat = imuQuaternionToThree({
        w: fromSample.w,
        x: fromSample.x,
        y: fromSample.y,
        z: fromSample.z
      });
      return normalizeQuaternion({
        x: viewerQuat.x,
        y: viewerQuat.y,
        z: viewerQuat.z,
        w: viewerQuat.w
      });
    }
    if (![sample.roll, sample.pitch, sample.yaw].every((value) => Number.isFinite(value))) {
      return null;
    }
    return normalizeQuaternion(
      eulerDegreesToQuaternionXYZ({
        pitch: sample.pitch,
        yaw: sample.yaw,
        roll: sample.roll
      })
    );
  }

  function cameraKeyVariants(value: string | null | undefined): string[] {
    const trimmed = typeof value === 'string' ? value.trim() : '';
    if (!trimmed) return [];
    const out = new Set<string>([trimmed]);
    const stripped = trimmed.startsWith('device:')
      ? trimmed.slice('device:'.length)
      : trimmed.startsWith('stream:')
        ? trimmed.slice('stream:'.length)
        : trimmed;
    if (stripped) {
      out.add(stripped);
      out.add(`device:${stripped}`);
      out.add(`stream:${stripped}`);
    }
    return Array.from(out.values());
  }

  function collectKeyVariants(values: Array<string | null | undefined>): string[] {
    const out = new Set<string>();
    for (const value of values) {
      for (const key of cameraKeyVariants(value)) {
        out.add(key);
      }
    }
    return Array.from(out.values());
  }

  function cameraPovOptionId(profileId: string, sourceId: string): string {
    return `${profileId}::${sourceId}`;
  }

  function rigPoseToViewerTransform(
    pose:
      | {
          translation?: { x?: number; y?: number; z?: number };
          rotation?: { roll?: number; pitch?: number; yaw?: number };
        }
      | null
      | undefined
  ): PoseTransform | null {
    if (!pose) return null;
    const tx = pose.translation?.x;
    const ty = pose.translation?.y;
    const tz = pose.translation?.z;
    const roll = pose.rotation?.roll;
    const pitch = pose.rotation?.pitch;
    const yaw = pose.rotation?.yaw;
    if (![tx, ty, tz, roll, pitch, yaw].every((value) => typeof value === 'number' && Number.isFinite(value))) {
      return null;
    }

    // Keep quaternion construction aligned with backend localization rig conversion.
    const quaternion = eulerDegreesToQuaternionXYZ({ pitch: -pitch, yaw, roll });
    return {
      position: [ty, tz, tx] as Vec3,
      quaternion
    };
  }

  function solverPoseToTransform(
    pose: { translation: { x: number; y: number; z: number }; rotation: { quaternion: PoseQuaternion } } | null | undefined
  ): PoseTransform | null {
    if (!pose) return null;
    return {
      position: [pose.translation.x, pose.translation.y, pose.translation.z],
      quaternion: pose.rotation.quaternion
    };
  }

  function cameraFramePositionForDetection(
    detection: LocalizationDetectionPose,
    coordinateSpace: LocalizationPoseSpace,
    selectedPov: CameraPovState | null
  ): Vec3 | null {
    const pose = detection.pose;
    const tagFromParent: PoseTransform = {
      position: [pose.translation.x, pose.translation.y, pose.translation.z],
      quaternion: pose.rotation.quaternion
    };
    if (coordinateSpace === 'tag_in_camera') {
      return tagFromParent.position;
    }
    if (coordinateSpace === 'tag_in_robot') {
      const robotFromCamera = selectedPov?.transform ?? null;
      if (!robotFromCamera) return null;
      const cameraFromRobot = invertTransform(robotFromCamera);
      const cameraFromTag = composeTransforms(cameraFromRobot, tagFromParent);
      return cameraFromTag.position;
    }
    if (coordinateSpace === 'camera_in_field' || coordinateSpace === 'robot_in_field') {
      const fieldFromCamera = selectedPov?.transform ?? null;
      if (!fieldFromCamera) return null;
      const cameraFromField = invertTransform(fieldFromCamera);
      const cameraFromTag = composeTransforms(cameraFromField, tagFromParent);
      return cameraFromTag.position;
    }
    return null;
  }

  function detectionInsidePovFov(
    detection: LocalizationDetectionPose,
    coordinateSpace: LocalizationPoseSpace,
    selectedPov: CameraPovState | null,
    intrinsics: CameraPovIntrinsics | null,
    forwardSign: 1 | -1 = 1
  ): boolean {
    if (!intrinsics) return true;
    const position = cameraFramePositionForDetection(detection, coordinateSpace, selectedPov);
    if (!position) return true;
    const [x, y, z] = position;
    if (![x, y, z].every((value) => Number.isFinite(value))) return false;
    const depth = z * forwardSign;
    if (depth <= 0) return false;

    const { fx, fy, cx, cy, width, height } = intrinsics;
    if (![fx, fy, width, height].every((value) => Number.isFinite(value) && value > 0)) return true;
    if (![cx, cy].every((value) => Number.isFinite(value))) return true;

    const nx = (x * forwardSign) / depth;
    const ny = y / depth;
    const u = fx * nx + cx;
    const vDown = fy * ny + cy;
    const vUp = fy * -ny + cy;
    const insideX = u >= 0 && u <= width;
    const insideY = (vDown >= 0 && vDown <= height) || (vUp >= 0 && vUp <= height);
    return insideX && insideY;
  }

  function detectionPoseToTransform(detection: LocalizationDetectionPose): PoseTransform {
    const pose = detection.pose;
    return {
      position: [pose.translation.x, pose.translation.y, pose.translation.z],
      quaternion: pose.rotation.quaternion
    };
  }

  function detectionWithTransform(
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

  function detectionKey(detection: LocalizationDetectionPose): string {
    return `${detection.sourceId}|${detection.cameraUid}|${detection.tagId}`;
  }

  function findRigCameraForDetection(
    detection: LocalizationDetectionPose,
    sourceById: Map<string, LocalizationPipelineSource>
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
    for (const camera of rigLayoutState.layout.cameras) {
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
    fieldFromCameraByKey: Record<string, PoseTransform>
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
    const rigCamera = findRigCameraForDetection(detection, sourceById);
    const robotFromCamera = rigPoseToViewerTransform(rigCamera?.pose ?? null);
    if (!robotFromCamera) return null;
    const fieldFromCamera = composeTransforms(fieldFromRobot, robotFromCamera);
    return composeTransforms(fieldFromCamera, detectionPoseToTransform(detection));
  }

  function fieldDetectionsForOutputs(
    outputs: LocalizationSolverOutputs | null,
    sourceById: Map<string, LocalizationPipelineSource>
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
      const fieldFromTag = detectionToFieldTransform(detection, sourceById, fieldFromRobot, fieldFromCameraByKey);
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

  function toFinite(value: unknown): number | null {
    const parsed = typeof value === 'number' ? value : typeof value === 'string' ? Number(value) : NaN;
    return Number.isFinite(parsed) ? parsed : null;
  }

  function extractResolutionCandidate(value: unknown): { width: number; height: number } | null {
    if (!value || typeof value !== 'object') return null;
    const record = value as any;
    const width = toFinite(record?.width ?? record?.w ?? record?.cols);
    const height = toFinite(record?.height ?? record?.h ?? record?.rows);
    if (width == null || height == null) return null;
    if (width <= 0 || height <= 0) return null;
    return { width, height };
  }

  function resolvePovResolution(
    manifest: any,
    calibration: any,
    cx: number,
    cy: number
  ): { width: number; height: number } | null {
    const candidates = [
      manifest?.capture?.mode?.format?.resolution,
      manifest?.capture?.mode?.resolution,
      manifest?.capture?.mode?.format,
      manifest?.capture?.format?.resolution,
      manifest?.capture?.format,
      manifest?.capture?.resolution,
      manifest?.encoder_settings?.output_resolution,
      manifest?.encoder_settings?.resolution,
      manifest?.encoder?.output?.resolution,
      manifest?.encoder?.resolution,
      calibration?.resolution,
      calibration?.imageSize,
      calibration?.frameSize,
      calibration
    ];
    for (const candidate of candidates) {
      const parsed = extractResolutionCandidate(candidate);
      if (parsed) return parsed;
    }

    // Fallback when only principal point is present: assume centered intrinsics.
    const width = cx * 2;
    const height = cy * 2;
    if (Number.isFinite(width) && Number.isFinite(height) && width > 16 && height > 16) {
      return { width, height };
    }
    return null;
  }

  function fovDegToFocalPx(sensorPx: number, fovDeg: number | null): number | null {
    if (!Number.isFinite(sensorPx) || sensorPx <= 0) return null;
    if (!Number.isFinite(fovDeg) || fovDeg == null || fovDeg <= 0 || fovDeg >= 179.999) return null;
    const half = (fovDeg * Math.PI) / 360;
    const tan = Math.tan(half);
    if (!Number.isFinite(tan) || tan <= 0) return null;
    const focal = sensorPx / (2 * tan);
    if (!Number.isFinite(focal) || focal <= 0) return null;
    return focal;
  }

  function normalizeRay(x: number, y: number, z: number): [number, number, number] | null {
    const mag = Math.hypot(x, y, z);
    if (!Number.isFinite(mag) || mag <= 1e-12) return null;
    return [x / mag, y / mag, z / mag];
  }

  function angleBetweenDeg(
    a: [number, number, number] | null,
    b: [number, number, number] | null
  ): number | null {
    if (!a || !b) return null;
    const dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    const clamped = Math.max(-1, Math.min(1, dot));
    const radians = Math.acos(clamped);
    if (!Number.isFinite(radians)) return null;
    return (radians * 180) / Math.PI;
  }

  function rayFromPixelPinholeDistorted(
    u: number,
    v: number,
    intrinsics: { fx: number; fy: number; cx: number; cy: number; k1: number; k2: number; p1: number; p2: number; k3: number }
  ): [number, number, number] | null {
    const { fx, fy, cx, cy, k1, k2, p1, p2, k3 } = intrinsics;
    if (!Number.isFinite(fx) || !Number.isFinite(fy) || fx <= 0 || fy <= 0) return null;
    const xd = (u - cx) / fx;
    const yd = (v - cy) / fy;
    if (!Number.isFinite(xd) || !Number.isFinite(yd)) return null;

    let xu = xd;
    let yu = yd;
    for (let i = 0; i < 12; i += 1) {
      const r2 = xu * xu + yu * yu;
      const r4 = r2 * r2;
      const r6 = r4 * r2;
      const radial = 1 + k1 * r2 + k2 * r4 + k3 * r6;
      if (!Number.isFinite(radial) || Math.abs(radial) < 1e-10) break;
      const deltaX = 2 * p1 * xu * yu + p2 * (r2 + 2 * xu * xu);
      const deltaY = p1 * (r2 + 2 * yu * yu) + 2 * p2 * xu * yu;
      const nextXu = (xd - deltaX) / radial;
      const nextYu = (yd - deltaY) / radial;
      if (!Number.isFinite(nextXu) || !Number.isFinite(nextYu)) break;
      if (Math.abs(nextXu - xu) < 1e-10 && Math.abs(nextYu - yu) < 1e-10) {
        xu = nextXu;
        yu = nextYu;
        break;
      }
      xu = nextXu;
      yu = nextYu;
    }

    return normalizeRay(xu, yu, 1);
  }

  function estimatePinholeDistortedFovDegs(
    resolution: { width: number; height: number },
    intrinsics: { fx: number; fy: number; cx: number; cy: number; k1: number; k2: number; p1: number; p2: number; k3: number }
  ): { hfov: number | null; vfov: number | null } {
    const width = resolution.width;
    const height = resolution.height;
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
      return { hfov: null, vfov: null };
    }

    const uL = 0.5;
    const uR = width - 0.5;
    const vT = 0.5;
    const vB = height - 0.5;
    const uM = width * 0.5;
    const vM = height * 0.5;

    const left = rayFromPixelPinholeDistorted(uL, vM, intrinsics);
    const right = rayFromPixelPinholeDistorted(uR, vM, intrinsics);
    const top = rayFromPixelPinholeDistorted(uM, vT, intrinsics);
    const bottom = rayFromPixelPinholeDistorted(uM, vB, intrinsics);
    const hfov = angleBetweenDeg(left, right);
    const vfov = angleBetweenDeg(top, bottom);
    const sane = (value: number | null): number | null =>
      value != null && Number.isFinite(value) && value > 0 && value < 179.999 ? value : null;
    return { hfov: sane(hfov), vfov: sane(vfov) };
  }

  function extractPovIntrinsicsSet(
    stream: StreamInfo
  ): { undistorted: CameraPovIntrinsics | null; raw: CameraPovIntrinsics | null } | null {
    const manifest = stream.manifest as any;
    const calibration =
      manifest?.calibration ??
      manifest?.camera?.calibration ??
      manifest?.camera?.intrinsics ??
      manifest?.intrinsics ??
      null;
    if (!calibration || typeof calibration !== 'object') return null;

    const fx = toFinite((calibration as any).fx);
    const fy = toFinite((calibration as any).fy);
    const cx = toFinite((calibration as any).cx);
    const cy = toFinite((calibration as any).cy);
    if ([fx, fy].some((value) => value == null || value <= 0)) return null;
    if ([cx, cy].some((value) => value == null)) return null;

    const resolution = resolvePovResolution(manifest, calibration, cx, cy);
    if (!resolution) return null;
    const { width, height } = resolution;
    const undistorted: CameraPovIntrinsics = { fx, fy, cx, cy, width, height };

    const k1 = toFinite((calibration as any).k1) ?? 0;
    const k2 = toFinite((calibration as any).k2) ?? 0;
    const p1 = toFinite((calibration as any).p1) ?? 0;
    const p2 = toFinite((calibration as any).p2) ?? 0;
    const k3 = toFinite((calibration as any).k3) ?? 0;
    const lensModelRaw = String((calibration as any).lensModel ?? (calibration as any).lens_model ?? 'pinhole')
      .trim()
      .toLowerCase();
    const lensModel = lensModelRaw === 'fisheye' ? 'fisheye' : 'pinhole';
    const hasPinholeDistortion = [k1, k2, p1, p2, k3].some((value) => Math.abs(value) > 1e-7);
    const fovEstimate =
      lensModel === 'pinhole' && hasPinholeDistortion
        ? estimatePinholeDistortedFovDegs({ width, height }, { fx, fy, cx, cy, k1, k2, p1, p2, k3 })
        : estimateCalibrationFovDegs(
            { width, height },
            {
              fx,
              fy,
              cx,
              cy,
              k1,
              k2,
              p1,
              p2,
              k3,
              lensModel
            }
          );
    const rawFx = fovDegToFocalPx(width, fovEstimate.hfov) ?? fx;
    const rawFy = fovDegToFocalPx(height, fovEstimate.vfov) ?? fy;
    const raw: CameraPovIntrinsics = { fx: rawFx, fy: rawFy, cx, cy, width, height };

    return { undistorted, raw };
  }

  function selectedPovIntrinsics(
    selectedPov: CameraPovState | null,
    mode: CameraPovFovMode
  ): CameraPovIntrinsics | null {
    if (!selectedPov || mode === 'none') return null;
    if (mode === 'raw') {
      return selectedPov.intrinsicsRaw ?? selectedPov.intrinsicsUndistorted ?? null;
    }
    return selectedPov.intrinsicsUndistorted ?? selectedPov.intrinsicsRaw ?? null;
  }

  function intrinsicsEqual(a: CameraPovIntrinsics | null, b: CameraPovIntrinsics | null): boolean {
    if (a == null && b == null) return true;
    if (a == null || b == null) return false;
    return (
      a.fx === b.fx &&
      a.fy === b.fy &&
      a.cx === b.cx &&
      a.cy === b.cy &&
      a.width === b.width &&
      a.height === b.height
    );
  }

  function streamIdentityKeys(stream: StreamInfo): string[] {
    const manifest = stream.manifest as any;
    const identity = manifest?.identity ?? null;
    const identityKeys = Array.isArray(identity?.keys) ? identity.keys.map((value: unknown) => String(value ?? '').trim()) : [];
    const captureKeys = Array.isArray(manifest?.capture?.device_keys)
      ? manifest.capture.device_keys.map((value: unknown) => String(value ?? '').trim())
      : [];
    return collectKeyVariants([
      stream.id,
      identity?.display ?? null,
      identity?.hardware_id ?? null,
      identity?.alias ?? null,
      identity?.id ?? null,
      ...identityKeys,
      ...captureKeys
    ]);
  }

  const loadStreamsSnapshot = async (): Promise<void> => {
    try {
      streamInfos = await StreamsApi.listStreams({ cacheMs: 0, forceRefresh: true });
    } catch {
      streamInfos = [];
    }
  };

  function shouldApplyLiveUpdate(event: RealtimeUpdateEvent): boolean {
    if (
      event.path.startsWith('/v1/localization') ||
      event.path.startsWith('/v1/streams') ||
      event.path.startsWith('/v1/pipelines') ||
      event.path.startsWith('/v1/media') ||
      event.path.startsWith('/v1/device')
    ) {
      return true;
    }
    if (event.kind === 'api') {
      return false;
    }
    return (
      event.kind === 'localization' ||
      event.kind === 'streams' ||
      event.kind === 'pipelines' ||
      event.kind === 'media' ||
      event.kind === 'imu' ||
      event.kind === 'device' ||
      event.kind === 'settings'
    );
  }

  function shouldRefreshSourcesForLiveUpdate(event: RealtimeUpdateEvent): boolean {
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
    if (event.kind === 'streams' || event.kind === 'pipelines') {
      return true;
    }
    return false;
  }

  function refreshLocalizationLiveState(options: { refreshSources?: boolean } = {}): void {
    const profileId = $activeProfile?.id ?? $localizationConfig?.activeProfileId ?? null;
    void rigLayoutStore.refresh({ force: true });
    void loadLocalizationConfig();
    if (options.refreshSources) {
      const now = Date.now();
      if (now - lastLiveSourcesRefreshAtMs >= LIVE_SOURCES_REFRESH_MIN_INTERVAL_MS) {
        lastLiveSourcesRefreshAtMs = now;
        void loadSources();
      }
    }
    void loadStreamsSnapshot();
    void loadPipelineTemplates();
    void loadPipelineStatus(profileId);
    if (profileId) {
      void loadPipelineOutputs(profileId);
    }
    void loadFieldMapList();
  }

  function scheduleLiveUpdatesRefresh(event?: RealtimeUpdateEvent): void {
    if (!browser) return;
    if (event && shouldRefreshSourcesForLiveUpdate(event)) {
      liveUpdatesRefreshSourcesPending = true;
    }
    if (liveUpdatesRefreshHandle != null) return;
    liveUpdatesRefreshHandle = window.setTimeout(() => {
      liveUpdatesRefreshHandle = null;
      if (document.hidden) return;
      const refreshSources = liveUpdatesRefreshSourcesPending;
      liveUpdatesRefreshSourcesPending = false;
      refreshLocalizationLiveState({ refreshSources });
    }, LIVE_UPDATES_REFRESH_DEBOUNCE_MS);
  }

  function scheduleLiveUpdatesReconnect(): void {
    if (!browser) return;
    if (liveUpdatesReconnectHandle != null) return;
    liveUpdatesReconnectHandle = window.setTimeout(() => {
      liveUpdatesReconnectHandle = null;
      connectLiveUpdates();
    }, LIVE_UPDATES_RECONNECT_MS);
  }

  function disconnectLiveUpdates(): void {
    liveUpdatesNonce += 1;
    if (liveUpdatesReconnectHandle != null) {
      clearTimeout(liveUpdatesReconnectHandle);
      liveUpdatesReconnectHandle = null;
    }
    if (liveUpdatesRefreshHandle != null) {
      clearTimeout(liveUpdatesRefreshHandle);
      liveUpdatesRefreshHandle = null;
    }
    liveUpdatesRefreshSourcesPending = false;
    liveUpdatesCleanup?.();
    liveUpdatesCleanup = null;
  }

  function connectLiveUpdates(): void {
    if (!browser) return;
    const nonce = (liveUpdatesNonce += 1);
    if (liveUpdatesReconnectHandle != null) {
      clearTimeout(liveUpdatesReconnectHandle);
      liveUpdatesReconnectHandle = null;
    }
    liveUpdatesCleanup?.();
    liveUpdatesCleanup = null;
    liveUpdatesCleanup = connectRealtimeUpdatesStream({
      onChange: (event) => {
        if (nonce !== liveUpdatesNonce) return;
        if (!shouldApplyLiveUpdate(event)) return;
        scheduleLiveUpdatesRefresh(event);
      },
      onClose: () => {
        if (nonce !== liveUpdatesNonce) return;
        scheduleLiveUpdatesReconnect();
      },
      onError: () => {
        if (nonce !== liveUpdatesNonce) return;
        scheduleLiveUpdatesReconnect();
      }
    });
  }

  function computeActiveSolverConfig(profile: LocalizationProfile | null, requestedSolverId: string): LocalizationSolverConfig | null {
    const solvers = profile?.solvers ?? [];
    if (solvers.length === 0) return null;
    const requested = requestedSolverId.trim();
    if (requested) {
      const match = solvers.find((solver) => solver.id === requested);
      if (match) return match;
    }
    return solvers[0] ?? null;
  }

  function computeProfileIndexById(profiles: LocalizationProfile[]): Map<string, number> {
    const map = new Map<string, number>();
    profiles.forEach((profile, index) => {
      map.set(profile.id, index);
    });
    return map;
  }

  function computeActiveProfileColor(
    activeProfile: LocalizationProfile | null,
    profiles: LocalizationProfile[],
    profileIndexById: Map<string, number>
  ): string {
    if (!activeProfile) return PROFILE_COLORS[0] ?? '#38bdf8';
    return profileColorForId(activeProfile.id, profiles, profileIndexById, PROFILE_COLORS);
  }

  const activeProfileSourceStreamId = $derived.by(() => {
    const profileId = ($activeProfile?.id ?? $activeProfileId ?? '').trim();
    return profileId ? `profile:${profileId}` : '';
  });

  const compatibleSources = $derived.by<LocalizationPipelineSource[]>(() => {
    const selfProfileStreamId = activeProfileSourceStreamId;
    return sources.filter((entry) => {
      if (!(sourceCompatibility[entry.id] ?? false)) return false;
      if (selfProfileStreamId && entry.streamId.trim() === selfProfileStreamId) return false;
      return true;
    });
  });

  const localizationTemplates = $derived.by(() => {
    const tagged = pipelineTemplates.filter((entry) => entry.tags?.includes('localization'));
    return tagged.length > 0 ? tagged : pipelineTemplates;
  });

  const profileFieldOrigin = $derived.by<LocalizationFieldOriginConfig>(() =>
    normalizeFieldOriginConfig(($activeProfile ?? null)?.fieldOrigin)
  );
  const profileFieldOriginCustom = $derived.by<LocalizationCustomFieldOrigin>(() =>
    profileFieldOrigin.custom ?? { x: 0, z: 0, yawDeg: 0 }
  );

  const sourceIdById = $derived.by<Map<string, string>>(() => new Map(sources.map((source) => [source.id, source.id])));
  const sourceIdByStreamOutput = $derived.by<Map<string, string>>(() => {
    const map = new Map<string, string>();
    for (const source of sources) {
      const key = sourceStreamOutputKey(source.streamId, source.outputKey);
      if (!key) continue;
      map.set(key, source.id);
    }
    return map;
  });
  const compatibleSourceIdById = $derived.by<Map<string, string>>(
    () => new Map(compatibleSources.map((source) => [source.id, source.id]))
  );
  const compatibleSourceIdByStreamOutput = $derived.by<Map<string, string>>(() => {
    const map = new Map<string, string>();
    for (const source of compatibleSources) {
      const key = sourceStreamOutputKey(source.streamId, source.outputKey);
      if (!key) continue;
      map.set(key, source.id);
    }
    return map;
  });
  const resolveProfileSourceId = (
    source: { id?: string | null; streamId?: string | null; outputKey?: string | null },
    options: { compatibleOnly?: boolean } = {}
  ): string | null => {
    const compatibleOnly = options.compatibleOnly ?? false;
    const id = String(source.id ?? '').trim();
    const streamOutput = sourceStreamOutputKey(source.streamId, source.outputKey);
    const byId = compatibleOnly ? compatibleSourceIdById : sourceIdById;
    const byStreamOutput = compatibleOnly ? compatibleSourceIdByStreamOutput : sourceIdByStreamOutput;
    if (id && byId.has(id)) {
      return byId.get(id) ?? null;
    }
    if (streamOutput) {
      return byStreamOutput.get(streamOutput) ?? null;
    }
    return null;
  };

  const selectedSources = $derived.by<LocalizationPipelineSource[]>(() =>
    selectedSourceIds
      .map((id) => sources.find((entry) => entry.id === id))
      .filter((entry): entry is LocalizationPipelineSource => Boolean(entry))
  );
  const sourceWeightsById = $derived.by<Record<string, number>>(() => {
    const out: Record<string, number> = {};
    for (const source of $activeProfile?.sources ?? []) {
      const weight = Number(source.weight);
      const resolvedId = resolveProfileSourceId(source);
      const nextWeight = Number.isFinite(weight) ? weight : 1;
      const id = String(source.id ?? '').trim();
      if (id) out[id] = nextWeight;
      if (resolvedId) out[resolvedId] = nextWeight;
    }
    return out;
  });
  const sourceUsedByProfilesById = $derived.by<Record<string, string[]>>(() => {
    const out: Record<string, string[]> = {};
    const activeId = $activeProfile?.id ?? '';
    for (const profile of $profiles) {
      if (profile.id === activeId) continue;
      const label = (profile.name ?? '').trim() || profile.id;
      for (const source of profile.sources ?? []) {
        if (!source.enabled) continue;
        const id = resolveProfileSourceId(source, { compatibleOnly: true });
        if (!id) continue;
        const list = out[id] ?? [];
        if (!list.includes(label)) {
          out[id] = [...list, label];
        }
      }
    }
    return out;
  });
  const cameraIntrinsicsByKey = $derived.by<
    Record<string, { undistorted: CameraPovIntrinsics | null; raw: CameraPovIntrinsics | null }>
  >(() => {
    const out: Record<string, { undistorted: CameraPovIntrinsics | null; raw: CameraPovIntrinsics | null }> = {};
    for (const stream of streamInfos) {
      const intrinsics = extractPovIntrinsicsSet(stream);
      if (!intrinsics) continue;
      for (const key of streamIdentityKeys(stream)) {
        out[key] = intrinsics;
      }
    }
    return out;
  });

  // Sources from profiles with `viewEnabled=true` (the "Views" overlay list).
  // These are the sources that should drive what markers are visible in the 3D viewer.
  const viewOverlaySources = $derived(computeViewOverlaySources($profiles, $activeProfile ?? null, sources));

  // Multiple solver configs can exist per profile (solo sources and/or grouped solves).
  // Keep a local "active solver id" for the active profile so the UI can inspect/edit
  // one solver at a time.
  let activeSolverId = $state<string>('');

  const activeSolverConfig = $derived(computeActiveSolverConfig($activeProfile ?? null, activeSolverId));
  const profileTemporalStabilization = $derived(
    normalizeTemporalSettings(($activeProfile ?? null)?.temporalStabilization)
  );
  const activeSolverTemporalOverride = $derived(
    activeSolverConfig?.temporalStabilization
      ? normalizeTemporalSettings(activeSolverConfig.temporalStabilization)
      : null
  );
  const activeSolverTemporalEffective = $derived(
    activeSolverTemporalOverride ?? profileTemporalStabilization
  );
  const activeSolverRuntimeTuning = $derived(
    normalizeSolverRuntimeTuning(activeSolverConfig?.runtimeTuning)
  );

  $effect(() => {
    const mode = profileFieldOrigin.mode;
    if (fieldOriginMode !== mode) {
      fieldOriginMode = mode;
    }
  });

  const activeSolverResult = $derived.by<LocalizationSolverResult | null>(() => {
    const solvers = solveResponse?.solvers ?? [];
    const requested = activeSolverConfig?.id ?? activeSolverId;
    if (requested) {
      return solvers.find((solver) => solver.id === requested) ?? solvers[0] ?? null;
    }
    return solvers[0] ?? null;
  });

  const activeSolverOutputs = $derived.by<LocalizationSolverOutputs | null>(() => activeSolverResult?.outputs ?? null);

  function outputsForProfile(profile: LocalizationProfile): LocalizationSolverOutputs | null {
    const resp = solveResponsesByProfile[profile.id] ?? (profile.id === solveResponse?.profileId ? solveResponse : null);
    const solvers = resp?.solvers ?? [];
    if (profile.id === ($activeProfile?.id ?? null)) {
      const requested = activeSolverConfig?.id ?? activeSolverId;
      if (requested) {
        return solvers.find((solver) => solver.id === requested)?.outputs ?? solvers[0]?.outputs ?? null;
      }
    }
    return solvers[0]?.outputs ?? null;
  }

  function enabledSourcesForProfile(profile: LocalizationProfile): LocalizationPipelineSource[] {
    const enabledIds = new Set(
      (profile.sources ?? [])
        .filter((source) => source.enabled)
        .map((source) => source.id)
    );
    return sources.filter((source) => enabledIds.has(source.id));
  }

  function sourceKeys(source: LocalizationPipelineSource | null): string[] {
    if (!source) return [];
    return collectKeyVariants([
      source.id,
      source.cameraUid,
      source.streamId,
      source.cameraPath,
      ...(source.cameraKeys ?? [])
    ]);
  }

  function rigCameraKeys(camera: RigCameraInfo | null): string[] {
    if (!camera) return [];
    return collectKeyVariants([
      camera.uid,
      camera.cameraUid ?? null,
      camera.streamId ?? null,
      camera.driverCameraId ?? null,
      camera.hardwareId ?? null,
      camera.streamAlias ?? null
    ]);
  }

  function syntheticCameraFromSource(
    source: LocalizationPipelineSource,
    fallbackKey?: string | null
  ): RigCameraInfo | null {
    const key = (cameraKeyForSource(source) ?? source.id ?? fallbackKey ?? '').trim();
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
  }

  function syntheticCameraFromKey(key: string): RigCameraInfo | null {
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
  }

  $effect(() => {
    const profile = $activeProfile;
    const solvers = profile?.solvers ?? [];
    if (solvers.length === 0) {
      activeSolverId = '';
      return;
    }
    if (activeSolverId && solvers.some((solver) => solver.id === activeSolverId)) {
      return;
    }
    activeSolverId = solvers[0]?.id ?? '';
  });

  const profileIndexById = $derived(computeProfileIndexById($profiles));

  const activeProfileColor = $derived(computeActiveProfileColor($activeProfile ?? null, $profiles, profileIndexById));

  const viewProfiles = $derived(computeViewProfiles($profiles, $activeProfile ?? null));
  const filteredProfiles = $derived.by<LocalizationProfile[]>(() => {
    const q = profileSearch.trim().toLowerCase();
    if (!q) return $profiles;
    return $profiles.filter((profile) => {
      const name = (profile.name ?? '').toLowerCase();
      const id = (profile.id ?? '').toLowerCase();
      return name.includes(q) || id.includes(q);
    });
  });

  // Viewer accent color should follow what you're actually looking at (enabled "Views"),
  // not whichever profile happens to be active for editing.
	  const viewerAccentColor = $derived.by<string | null>(() => {
	    if (viewProfiles.length === 1) {
	      const id = viewProfiles[0]?.id ?? '';
	      return id ? profileColorForId(id, $profiles, profileIndexById, PROFILE_COLORS) : activeProfileColor;
	    }
	    if (viewProfiles.length === 0) {
	      return activeProfileColor;
	    }
	    // When multiple views are enabled, keep a stable accent for the single robot/camera meshes.
	    return activeProfileColor;
	  });

  const viewProfilesWithSources = $derived.by<LocalizationProfile[]>(() =>
    viewProfiles.filter((profile) => profile.sources.some((source) => source.enabled))
  );

  const activeHasSources = $derived.by(() => selectedSourceIds.length > 0);

  const hasAnyFeedSources = $derived.by(() => activeHasSources || viewProfilesWithSources.length > 0);
  const activeImuSource = $derived.by<LocalizationPipelineSource | null>(() => {
    const unique = new Map<string, LocalizationPipelineSource>();
    for (const source of [...viewOverlaySources, ...selectedSources]) {
      if (!isImuSource(source)) continue;
      unique.set(source.id, source);
    }
    const candidates = Array.from(unique.values());
    if (candidates.length === 0) return null;
    candidates.sort((left, right) => {
      const delta = imuSourcePriority(left) - imuSourcePriority(right);
      if (delta !== 0) return delta;
      const labelDelta = left.streamLabel.localeCompare(right.streamLabel);
      if (labelDelta !== 0) return labelDelta;
      return left.id.localeCompare(right.id);
    });
    return candidates[0] ?? null;
  });
  const imuRotationOverlayData = $derived.by(() => {
    const sample = imuRotationSample;
    if (!sample) return null;
    return {
      sourceId: sample.sourceId,
      sourceLabel: sample.sourceLabel,
      outputKey: sample.outputKey,
      roll: sample.roll,
      pitch: sample.pitch,
      yaw: sample.yaw,
      quaternion: sample.quaternion,
      translation: sample.translation,
      sampleTimestampMs: sample.sampleTimestampMs,
      ageMs: Math.max(0, Date.now() - sample.receivedAtMs)
    };
  });
  const imuRotationStatusMessage = $derived.by<string | null>(() => {
    if (!activeImuSource) return 'No IMU source selected for this view';
    if (imuRotationError) return imuRotationError;
    if (!imuRotationSample || imuRotationSample.sourceId !== activeImuSource.id) {
      return 'Waiting for IMU sample...';
    }
    return null;
  });

  const solverOutputSpaces = $derived.by<LocalizationPoseSpace[]>(() => activeSolverConfig?.outputSpaces ?? []);
  const solvePoseSpaces = $derived.by<LocalizationPoseSpace[]>(() => {
    return SOLVE_POSE_SPACES.filter((space) => solverOutputSpaces.includes(space));
  });
  const derivedPoseSpaces = $derived.by<LocalizationPoseSpace[]>(() => {
    const derived = new Set<LocalizationPoseSpace>();
    // Derived outputs should be discoverable from whichever pose spaces the solver emits.
    // Example: when `tag_in_camera` is enabled, the API can also provide `tag_in_robot` + `robot_in_tag`.
    for (const space of solverOutputSpaces) {
      for (const next of DERIVED_POSE_SPACES[space] ?? []) {
        derived.add(next);
      }
    }
    return Array.from(derived.values()).filter((space) => POSE_SPACE_OPTIONS.includes(space));
  });
  const availableCoordinateSpaces = $derived.by<LocalizationPoseSpace[]>(() => {
    const next: LocalizationPoseSpace[] = [];
    const seen = new Set<string>();
    for (const space of solverOutputSpaces.concat(derivedPoseSpaces)) {
      if (!POSE_SPACE_OPTIONS.includes(space)) continue;
      if (seen.has(space)) continue;
      seen.add(space);
      next.push(space);
    }
    return next;
  });

  const profileSupportedSpacesById = $derived.by<Record<string, LocalizationPoseSpace[]>>(() => {
    const out: Record<string, LocalizationPoseSpace[]> = {};
    for (const profile of $profiles) {
      // Compute support from the persisted config (stable even when a profile isn't being polled).
      const outputSpaces = (profile.solvers ?? [])
        .flatMap((solver) => solver.outputSpaces ?? [])
        .filter((space) => POSE_SPACE_OPTIONS.includes(space));
      const supported = new Set<LocalizationPoseSpace>(outputSpaces);
      // Field spaces require a field map selection on the profile.
      if (!profile.fieldMapId) {
        supported.delete('robot_in_field');
        supported.delete('camera_in_field');
      }
      out[profile.id] = Array.from(supported.values());
    }
    return out;
  });

  const feedPoller = createFeedPoller({
    getIntervalMs: () => pollIntervalMs(pollHz),
    hasSources: () => hasAnyFeedSources,
    onPoll: runFeedPoll
  });

  const localizationActions = createLocalizationPageActions({
    getActiveProfile: () => $activeProfile ?? null,
    getActiveSolverConfig: () => activeSolverConfig,
    getSolvePoseSpaces: () => solvePoseSpaces,
    setFieldMapSelection: (next) => {
      fieldMapSelection = next;
    },
    assignMapToSelectedField,
    persistProfileUpdate,
    feedPoller,
    getSelectedSourceIds: () => selectedSourceIds,
    setSelectedSourceIds: (next) => {
      selectedSourceIds = next;
    },
    getPrimarySourceId: () => primarySourceId,
    setPrimarySourceId: (next) => {
      primarySourceId = next;
    },
    getSources: () => sources,
    setSolveResponse: (next) => {
      solveResponse = next as LocalizationSolveResponse | null;
    },
    setRawMarkers: (next) => {
      rawMarkers = next as LocalizationMarker[];
    },
    setPollResults: (next) => {
      pollResults = next as LocalizationSourceSampleStatus[];
    },
    setLastPollMs: (next) => {
      lastPollMs = next;
    },
    hasAnyFeedSources: () => hasAnyFeedSources,
    setFeedStatus: (next) => {
      feedStatus = next;
    },
    setFeedMessage: (next) => {
      feedMessage = next;
    }
  });
  applySourceSelectionImpl = localizationActions.applySourceSelection;
  const {
    setFieldMapSelection,
    toggleSolverOutputSpace,
    setActiveSolverMode,
    setSolveSpaceEnabled,
    setPipelineTemplateId,
    setSourceInputKey
  } = localizationActions;

  const setActiveSolverIdForUi = (nextId: string): void => {
    activeSolverId = nextId;
  };

  const commitSolverName = (): void => {
    const profile = $activeProfile;
    const solver = activeSolverConfig;
    if (!profile || !solver) return;
    const nextName = solverNameInput.trim();
    if (!nextName || nextName === solver.name) return;
    const nextSolvers = profile.solvers.map((entry) => (entry.id === solver.id ? { ...entry, name: nextName } : entry));
    void persistProfileUpdate({ ...profile, solvers: nextSolvers });
  };

  const addSolver = (): void => {
    const profile = $activeProfile;
    if (!profile) return;
    const existing = new Set(profile.solvers.map((solver) => solver.id));
    const base = 'group';
    let id = `${base}-${Date.now().toString(36)}`;
    let counter = 0;
    while (existing.has(id)) {
      counter += 1;
      id = `${base}-${Date.now().toString(36)}-${counter}`;
    }

    const template = activeSolverConfig ?? profile.solvers[0] ?? null;
    const nextSolver: LocalizationSolverConfig = {
      id,
      name: `Group ${profile.solvers.length + 1}`,
      mode: template?.mode ?? 'group_solve',
      outputSpaces: template?.outputSpaces?.length ? [...template.outputSpaces] : ['tag_in_camera', 'robot_in_field'],
      sourceIds: [],
      color: null,
      runtimeTuning: normalizeSolverRuntimeTuning(template?.runtimeTuning),
      temporalStabilization: template?.temporalStabilization
        ? normalizeTemporalSettings(template.temporalStabilization)
        : null
    };

    void persistProfileUpdate({ ...profile, solvers: [...profile.solvers, nextSolver] });
    activeSolverId = id;
  };

  const removeActiveSolver = (): void => {
    const profile = $activeProfile;
    const solver = activeSolverConfig;
    if (!profile || !solver) return;
    if (profile.solvers.length <= 1) return;
    const nextSolvers = profile.solvers.filter((entry) => entry.id !== solver.id);
    void persistProfileUpdate({ ...profile, solvers: nextSolvers });
    activeSolverId = nextSolvers[0]?.id ?? '';
  };

  const setActiveSolverSourceIds = (nextIds: string[]): void => {
    const profile = $activeProfile;
    const solver = activeSolverConfig;
    if (!profile || !solver) return;
    const unique = Array.from(new Set(nextIds.map((id) => id.trim()).filter(Boolean)));
    const current = solver.sourceIds ?? [];
    const same = current.length === unique.length && current.every((id) => unique.includes(id));
    if (same) return;
    const nextSolvers = profile.solvers.map((entry) => (entry.id === solver.id ? { ...entry, sourceIds: unique } : entry));
    void persistProfileUpdate({ ...profile, solvers: nextSolvers });
  };

  const setActiveSolverUseAllSources = (useAll: boolean): void => {
    if (useAll) {
      setActiveSolverSourceIds([]);
      return;
    }
    // Start from all currently-enabled inputs, then let the user prune.
    setActiveSolverSourceIds([...selectedSourceIds]);
  };

  const toggleActiveSolverSource = (sourceId: string, enabled: boolean): void => {
    const solver = activeSolverConfig;
    if (!solver) return;
    const current = solver.sourceIds ?? [];
    // Empty means "all sources" in the backend; expand to the current selection when customizing.
    const expanded = current.length === 0 ? [...selectedSourceIds] : [...current];
    const next = enabled ? Array.from(new Set([...expanded, sourceId])) : expanded.filter((id) => id !== sourceId);
    setActiveSolverSourceIds(next);
  };

  const setSourceWeight = (sourceId: string, rawValue: string): void => {
    const profile = $activeProfile;
    if (!profile) return;
    const parsed = Number(rawValue);
    if (!Number.isFinite(parsed)) return;
    const nextWeight = Math.max(0, Math.min(10, parsed));
    const nextSources = profile.sources.map((source) =>
      source.id === sourceId ? { ...source, weight: nextWeight } : source
    );
    const hadSource = nextSources.some((source) => source.id === sourceId);
    if (!hadSource) {
      const available = sources.find((source) => source.id === sourceId);
      if (!available) return;
      nextSources.push({
        id: available.id,
        streamId: available.streamId,
        outputKey: available.outputKey,
        cameraUid: available.cameraUid,
        poseSpace: null,
        inputKey: null,
        enabled: selectedSourceIds.includes(sourceId),
        weight: nextWeight
      });
    }
    const currentWeight = profile.sources.find((source) => source.id === sourceId)?.weight;
    if (currentWeight != null && Math.abs(currentWeight - nextWeight) < 1e-6) return;
    void persistProfileUpdate({ ...profile, sources: nextSources });
  };

  const { toggleSourceGroup, toggleSource } = createLocalizationSourceSelection({
    getOpenSourceGroups: () => openSourceGroups,
    setOpenSourceGroups: (next) => {
      openSourceGroups = next;
    },
    applySourceSelection,
    getSelectedSourceIds: () => selectedSourceIds
  });

  $effect(() => {
    const profile = $activeProfile;
    if (!profile) {
      selectedSourceIds = [];
      return;
    }
    if (!$activeProfileId || $activeProfileId !== profile.id) {
      $activeProfileId = profile.id;
    }
    const selfProfileStreamId = `profile:${profile.id}`;
    const enabled = Array.from(new Set(profile.sources
      .filter((source) => source.enabled)
      .map((source) => resolveProfileSourceId(source))
      .filter((id): id is string => Boolean(id))
      .filter((id) => {
        const resolved = sources.find((entry) => entry.id === id);
        if (!resolved) return false;
        return resolved.streamId.trim() !== selfProfileStreamId;
      })));
    selectedSourceIds = enabled;
    primarySourceId = primarySourceId && enabled.includes(primarySourceId) ? primarySourceId : enabled[0] ?? null;
  });

  $effect(() => {
    const profile = $activeProfile;
    if (!profile) {
      profileNameInput = '';
      profileNameTargetId = null;
      return;
    }
    if (profileNameTargetId !== profile.id) {
      profileNameInput = profile.name ?? '';
      profileNameTargetId = profile.id;
    }
  });

  $effect(() => {
    const profile = $activeProfile;
    const solver = activeSolverConfig;
    if (!profile || !solver) {
      solverNameInput = '';
      solverNameTargetKey = null;
      return;
    }
    const key = `${profile.id}:${solver.id}`;
    if (solverNameTargetKey !== key) {
      solverNameInput = solver.name ?? '';
      solverNameTargetKey = key;
    }
  });

  $effect(() => {
    const profile = $activeProfile;
    if (!profile) {
      tagSizeInput = '';
      tagSizeTargetId = null;
      tagSizeError = null;
      return;
    }
    if (tagSizeTargetId !== profile.id) {
      const next = typeof profile.tagSizeM === 'number' && Number.isFinite(profile.tagSizeM)
        ? formatMeters(profile.tagSizeM, 'm', 4)
        : '';
      tagSizeInput = next;
      tagSizeTargetId = profile.id;
      tagSizeError = null;
    }
  });

  $effect(() => {
    const config = $localizationConfig;
    if (!config || config.profiles.length === 0) return;
    const hasExplicit = config.profiles.some((profile) => typeof profile.viewEnabled === 'boolean');
    if (hasExplicit) return;
    const activeId = config.activeProfileId ?? config.profiles[0]?.id ?? null;
    if (!activeId) return;
    const nextProfiles = config.profiles.map((profile) =>
      profile.id === activeId ? { ...profile, viewEnabled: true } : profile
    );
    void localizationProfiles.persist({ ...config, profiles: nextProfiles });
  });

  $effect(() => {
    void hasAnyFeedSources;
    if (!hasAnyFeedSources) {
      feedPoller.stop();
      feedStatus = 'idle';
      feedMessage = null;
      return;
    }
    if (!feedPoller.isBusy()) {
      feedStatus = 'connecting';
      feedMessage = null;
      feedPoller.schedule(0);
    }
  });

  $effect(() => {
    void lastPollMs;
    const source = activeImuSource;
    if (!browser || !hasAnyFeedSources || !source) {
      if (!source) {
        imuRotationSample = null;
      }
      imuRotationError = null;
      return;
    }
    if (imuRotationSample?.sourceId !== source.id) {
      imuRotationSample = null;
    }

    const controller = new AbortController();
    void (async () => {
      try {
        const sample = await fetchPipelineOutputSample(
          source.streamId,
          source.pipelineId,
          source.outputKey,
          controller.signal
        );
        if (controller.signal.aborted) return;
        if (!sample) {
          imuRotationError = `${source.streamLabel}: no sample available`;
          return;
        }
        const parsed = parseImuRotationSample(sample.value);
        if (!parsed) {
          imuRotationError = `${source.streamLabel}: sample has no rotation`;
          return;
        }
        imuRotationSample = {
          sourceId: source.id,
          sourceLabel: source.streamLabel || source.cameraUid || source.id,
          outputKey: source.outputKey,
          roll: parsed.roll,
          pitch: parsed.pitch,
          yaw: parsed.yaw,
          quaternion: parsed.quaternion,
          translation: parsed.translation,
          sampleTimestampMs: parsed.sampleTimestampMs,
          receivedAtMs: Date.now()
        };
        imuRotationError = null;
      } catch (error) {
        if (controller.signal.aborted) return;
        imuRotationError = error instanceof Error ? error.message : 'Failed to fetch IMU sample';
      }
    })();

    return () => controller.abort();
  });

  $effect(() => {
    const next = selectedFieldMapId ?? '';
    if (fieldMapSelection !== next) {
      fieldMapSelection = next;
    }
  });

  $effect(() => {
    if (availableCoordinateSpaces.length === 0) return;
    if (!availableCoordinateSpaces.includes(coordinateSpace)) {
      // Prefer spaces that show IMU-leveled results, then fall back to whatever the solver provides.
      if (fieldSpaceAllowed && availableCoordinateSpaces.includes('camera_in_field')) {
        coordinateSpace = 'camera_in_field';
      } else if (fieldSpaceAllowed && availableCoordinateSpaces.includes('robot_in_field')) {
        coordinateSpace = 'robot_in_field';
      } else if (availableCoordinateSpaces.includes('tag_in_robot')) {
        coordinateSpace = 'tag_in_robot';
      } else if (availableCoordinateSpaces.includes('tag_in_camera')) {
        coordinateSpace = 'tag_in_camera';
      } else {
        coordinateSpace = availableCoordinateSpaces[0] ?? 'tag_in_camera';
      }
    }
    if (
      (coordinateSpace === 'camera_in_field' || coordinateSpace === 'robot_in_field') &&
      !fieldSpaceAllowed
    ) {
      coordinateSpace = availableCoordinateSpaces.find((space) => space === 'tag_in_camera') ?? 'tag_in_camera';
    }
  });

  const groupedSources = $derived.by(() => groupLocalizationSources(compatibleSources));

  $effect(() => {
    void sources;
    if (!browser) return;
    if (sources.length === 0) {
      streamInfos = [];
      return;
    }
    void loadStreamsSnapshot();
  });

  const primarySource = $derived.by<LocalizationPipelineSource | null>(() =>
    selectPrimarySource(primarySourceId, sources, selectedSources)
  );

  const primaryCameraKey = $derived.by<string | null>(() => cameraKeyForSource(primarySource));

  const baseFrame = $derived.by<LocalizationBaseFrame>(() => {
    if (coordinateSpace === 'tag_in_camera' || coordinateSpace === 'camera_in_tag') {
      return 'camera';
    }
    if (coordinateSpace === 'tag_in_robot' || coordinateSpace === 'robot_in_tag') {
      return 'robot';
    }
    return 'field';
  });

  const selectedCustomField = $derived.by<CustomField | null>(() => {
    if (viewMode !== 'custom-field') return null;
    const id = selectedCustomFieldId;
    return id ? $customFields.find((entry) => entry.id === id) ?? null : null;
  });

  const selectedFieldMapId = $derived.by<string | null>(() => {
    const mapId = $activeProfile?.fieldMapId ?? null;
    return typeof mapId === 'string' && mapId.trim() ? mapId.trim() : null;
  });

  const calibratedCameraIds = $derived.by(() => {
    const ids = new Set<string>();
    for (const camera of rigLayoutState.layout.cameras ?? []) {
      if (!camera.pose) continue;
      if (camera.uid) ids.add(camera.uid);
      if (camera.cameraUid) ids.add(camera.cameraUid);
      if (camera.hardwareId) ids.add(camera.hardwareId);
      if (camera.streamId) ids.add(camera.streamId);
      if (camera.streamAlias) ids.add(camera.streamAlias);
      if (camera.driverCameraId) ids.add(camera.driverCameraId);
    }
    return ids;
  });

  const uncalibratedSources = $derived.by<LocalizationPipelineSource[]>(() => {
    if (selectedSources.length === 0) return [];
    return selectedSources.filter((source) => !isSourceCalibrated(source, calibratedCameraIds));
  });

  const calibrationReady = $derived.by(() => selectedSources.length > 0 && uncalibratedSources.length === 0);
  const fieldSpaceAllowed = $derived.by(() => Boolean(selectedFieldMapId) && calibrationReady);

  const activeFieldMapDoc = $derived.by<FieldMapDocument | null>(() => {
    const id = selectedFieldMapId;
    return id ? fieldMapDocs[id] ?? null : null;
  });

  const activeFieldMapBitsStatus = $derived.by(() => buildActiveFieldMapBitsStatus(activeFieldMapDoc));

  const activeCustomField = $derived.by<{ name: string; width: number; depth: number } | null>(() =>
    buildActiveCustomField({ viewMode, selectedCustomField, activeFieldMapDoc })
  );

  const activeFieldDimensions = $derived.by<{ width: number; depth: number } | null>(() =>
    buildActiveFieldDimensions({ viewMode, activeCustomField })
  );

  const activeFieldOrigin = $derived.by<PlanarFieldOrigin | null>(() =>
    buildActiveFieldOrigin({
      viewMode,
      profileFieldOrigin,
      selectedCustomField,
      selectedCustomFieldOriginId,
      activeFieldDimensions
    })
  );

  const fieldSpaceLabel = $derived.by(() =>
    buildFieldSpaceLabel({ viewMode, activeFieldOrigin, selectedCustomField })
  );

  const fieldSceneTransform = $derived.by<PoseTransform | null>(() =>
    buildFieldSceneTransform({ baseFrame, activeFieldOrigin })
  );

  const originFromFieldCenterForEditor = $derived.by<PoseTransform>(() =>
    buildOriginFromFieldCenterForEditor(activeFieldOrigin)
  );

  const viewerSceneTransform = $derived.by<{ position: Vec3; quaternion: PoseQuaternion } | null>(() => {
    if (baseFrame === 'field') {
      return fieldSceneTransform ?? null;
    }
    return null;
  });

  $effect(() => {
    if (viewMode !== 'custom-field') return;
    const field = selectedCustomFieldId
      ? $customFields.find((entry) => entry.id === selectedCustomFieldId) ?? null
      : $customFields[0] ?? null;
    if (!field) {
      selectedCustomFieldId = null;
      selectedCustomFieldOriginId = null;
      return;
    }
    if (field.id !== selectedCustomFieldId) {
      selectedCustomFieldId = field.id;
    }
    const origin = selectedCustomFieldOriginId ? field.origins.find((entry) => entry.id === selectedCustomFieldOriginId) ?? null : null;
    if (!origin) {
      selectedCustomFieldOriginId = field.origins[0]?.id ?? null;
    }
  });

  $effect(() => {
    if (!browser) return;
    const streamIds = Array.from(
      new Set(
        selectedSources
          .filter((source) => !isImuSource(source))
          .map((source) => source.streamId)
          .filter(Boolean)
      )
    );

    for (const [streamId, cleanup] of Array.from(streamMetricsCleanup.entries())) {
      if (streamIds.includes(streamId)) continue;
      cleanup();
      streamMetricsCleanup.delete(streamId);
      ({
        streamMetricsById,
        streamMetricsUpdatedAtById,
        streamMetricsErrorById
      } = removeStreamMetrics({
        streamId,
        streamMetricsById,
        streamMetricsUpdatedAtById,
        streamMetricsErrorById
      }));
    }

    for (const streamId of streamIds) {
      if (streamMetricsCleanup.has(streamId)) continue;
      const cleanup = openStreamMetricsSocket(
        streamId,
        {
          onMetrics: (event) => {
            streamMetricsById = { ...streamMetricsById, [streamId]: event.metrics };
            streamMetricsUpdatedAtById = { ...streamMetricsUpdatedAtById, [streamId]: Date.now() };
            if (streamMetricsErrorById[streamId]) {
              const { [streamId]: _, ...restErr } = streamMetricsErrorById;
              streamMetricsErrorById = restErr;
            }
          },
          onError: (error) => {
            streamMetricsErrorById = { ...streamMetricsErrorById, [streamId]: error.error };
          }
        },
        { intervalMs: 200 }
      );
      streamMetricsCleanup.set(streamId, cleanup);
    }
  });

  $effect(() => {
    if (!browser) return;
    const key = primaryCameraKey;
    if (!key) {
      cameraPoseInputsKey = null;
      cameraPoseEditorError = null;
      return;
    }
    const origin = activeFieldOrigin;
    const originSignature = origin ? `${origin.id}:${origin.x}:${origin.z}:${origin.yawDeg}` : 'none';
    const inputsKey = `${key}|${originSignature}`;
    if (cameraPoseInputsKey === inputsKey) {
      return;
    }
    cameraPoseInputsKey = inputsKey;

    const centerFromCamera = cameraExtrinsicsTransform(key);
    const originFromCenter = originFromFieldCenterForEditor;
    const originFromCamera = composeTransforms(originFromCenter, centerFromCamera);
    const euler = quaternionToEulerDegreesXYZ(originFromCamera.quaternion);

    cameraPoseXInput = formatMeters(originFromCamera.position[0], 'm', 3);
    cameraPoseYInput = formatMeters(originFromCamera.position[1], 'm', 3);
    cameraPoseZInput = formatMeters(originFromCamera.position[2], 'm', 3);
    cameraPoseRollDeg = euler.roll.toFixed(2);
    cameraPosePitchDeg = euler.pitch.toFixed(2);
    cameraPoseYawDeg = euler.yaw.toFixed(2);
    cameraPoseEditorError = null;
  });

  $effect(() => {
    let next = rawMarkers;
    if (baseFrame === 'camera') {
      next = applyDeviceSeparation({ markers: next, separateCameras, selectedSources: viewOverlaySources, primaryCameraKey });
    }
    liveMarkers = next;
  });

  const detectionsBySourceId = $derived.by<Record<string, number>>(() => {
    const counts: Record<string, number> = {};
    for (const entry of pollResults) {
      counts[entry.sourceId] = entry.detections;
    }
    return counts;
  });

  const sourceStatusRows = $derived.by(() =>
    buildSourceStatusRows({
      pollResults,
      sources,
      streamMetricsById,
      streamMetricsUpdatedAtById,
      streamMetricsErrorById,
      detectionsBySourceId,
      toNumber
    })
  );

  const hasAnyViewsEnabled = $derived.by(() => viewProfiles.length > 0);

  const viewerCameras = $derived.by<RigCameraInfo[]>(() => {
    const cameras = hasAnyViewsEnabled
      ? buildViewerCameras({
          baseFrame,
          rigCameras: rigLayoutState.layout.cameras,
          selectedSources: viewOverlaySources
        })
      : [];

    const imuSource = activeImuSource;
    const imuCamera = imuSource ? syntheticCameraFromSource(imuSource) : null;
    if (imuCamera) {
      const imuKeys = new Set(sourceKeys(imuSource));
      const hasImuCamera = cameras.some((camera) => rigCameraKeys(camera).some((key) => imuKeys.has(key)));
      if (!hasImuCamera) {
        cameras.push(imuCamera);
      }
    }

    if (cameras.length > 0 || baseFrame !== 'field') {
      return cameras;
    }

    // Fallback: field-space solves can include camera poses even when no view-enabled source
    // is explicitly selected in the current profile list.
    const cameraInField = activeSolverOutputs?.cameraInField ?? [];
    if (!cameraInField.length) return cameras;

    const seen = new Set(cameras.map((camera) => camera.uid));
    const fallback = [...cameras];
    for (const entry of cameraInField) {
      const key = (entry.cameraUid || entry.sourceId || '').trim();
      if (!key || seen.has(key)) continue;
      seen.add(key);
      fallback.push({
        uid: key,
        streamId: null,
        cameraUid: entry.cameraUid?.trim() || null,
        streamAlias: null,
        driverCameraId: key,
        displayName: key,
        backend: 'Localization',
        pose: { translation: { x: 0, y: 0, z: 0 }, rotation: { roll: 0, pitch: 0, yaw: 0 } }
      });
    }
    return fallback;
  });

  const solverCameraTransforms = $derived.by<Record<string, { position: Vec3; quaternion?: PoseQuaternion }> | null>(() => {
    // Camera poses in field space should follow the view overlay when it is unambiguous.
    // This prevents the "Views" toggles from showing markers for one profile while the camera
    // model is still driven by the active profile's solve result.
    const outputsForCamera =
      viewProfiles.length === 1
        ? (() => {
            const profile = viewProfiles[0];
            const resp =
              solveResponsesByProfile[profile.id] ??
              (profile.id === solveResponse?.profileId ? solveResponse : null);
            const solvers = resp?.solvers ?? [];
            const activeProfileId = $activeProfile?.id ?? null;
            const activeSolverKey = activeSolverConfig?.id ?? activeSolverId;
            if (profile.id === activeProfileId && activeSolverKey) {
              return (
                solvers.find((solver) => solver.id === activeSolverKey)?.outputs ??
                solvers[0]?.outputs ??
                null
              );
            }
            return solvers[0]?.outputs ?? null;
          })()
        : activeSolverOutputs ?? null;

    const list = outputsForCamera?.cameraInField ?? null;
    if (!list || list.length === 0) return null;
    const out: Record<string, { position: Vec3; quaternion?: PoseQuaternion }> = {};
    for (const entry of list) {
      const pose = entry.pose;
      const transform: { position: Vec3; quaternion: PoseQuaternion } = {
        position: [pose.translation.x, pose.translation.y, pose.translation.z],
        quaternion: pose.rotation.quaternion
      };
      const keys = new Set<string>();
      const addKeys = (value: string | null | undefined) => {
        for (const key of cameraKeyVariants(value)) keys.add(key);
      };

      addKeys(entry.cameraUid);
      addKeys(entry.sourceId);

      const source = sources.find((candidate) => candidate.id === entry.sourceId) ?? null;
      if (source) {
        addKeys(source.id);
        addKeys(source.cameraUid);
        addKeys(source.streamId);
        addKeys(source.cameraPath);
        for (const key of source.cameraKeys ?? []) addKeys(key);
      }

      const sourceKeys = new Set(keys);
      const rigMatch = rigLayoutState.layout.cameras.find((camera) => {
        const candidates = [
          camera.uid,
          camera.cameraUid ?? null,
          camera.streamId ?? null,
          camera.driverCameraId ?? null,
          camera.hardwareId ?? null,
          camera.streamAlias ?? null
        ].flatMap((value) => cameraKeyVariants(value));
        return candidates.some((candidate) => sourceKeys.has(candidate));
      });
      if (rigMatch) {
        addKeys(rigMatch.uid);
        addKeys(rigMatch.cameraUid ?? null);
        addKeys(rigMatch.streamId ?? null);
        addKeys(rigMatch.driverCameraId ?? null);
        addKeys(rigMatch.hardwareId ?? null);
        addKeys(rigMatch.streamAlias ?? null);
      }

      for (const key of keys) {
        out[key] = transform;
      }
    }
    return out;
  });

  // When viewing in robot-space, drive the camera model pose from the active solver outputs so IMU-leveling
  // is visible (and matches how `tag_in_robot` was produced).
  const solverRobotCameraTransforms = $derived.by<Record<string, { position: Vec3; quaternion?: PoseQuaternion }> | null>(() => {
    // Preferred: derive robot_from_camera by inverting the field solve:
    //   field_from_camera = field_from_robot ∘ robot_from_camera
    // => robot_from_camera = robot_from_field ∘ field_from_camera
    const list = activeSolverOutputs?.cameraInField ?? null;
    const robotPose = activeSolverOutputs?.robotInField?.pose ?? null;
    if (list && list.length > 0 && robotPose) {
      const fieldFromRobot: PoseTransform = {
        position: [robotPose.translation.x, robotPose.translation.y, robotPose.translation.z],
        quaternion: robotPose.rotation.quaternion
      };
      const robotFromField = invertTransform(fieldFromRobot);

      const out: Record<string, { position: Vec3; quaternion?: PoseQuaternion }> = {};
      for (const entry of list) {
        const pose = entry.pose;
        const fieldFromCamera: PoseTransform = {
          position: [pose.translation.x, pose.translation.y, pose.translation.z],
          quaternion: pose.rotation.quaternion
        };
        const robotFromCamera = composeTransforms(robotFromField, fieldFromCamera);
        const transform: { position: Vec3; quaternion: PoseQuaternion } = {
          position: robotFromCamera.position,
          quaternion: robotFromCamera.quaternion
        };
        const keys = new Set<string>();
        const addKeys = (value: string | null | undefined) => {
          for (const key of cameraKeyVariants(value)) keys.add(key);
        };

        addKeys(entry.cameraUid);
        addKeys(entry.sourceId);
        const source = sources.find((candidate) => candidate.id === entry.sourceId) ?? null;
        if (source) {
          addKeys(source.id);
          addKeys(source.cameraUid);
          addKeys(source.streamId);
          addKeys(source.cameraPath);
          for (const key of source.cameraKeys ?? []) addKeys(key);
        }

        for (const key of keys) {
          out[key] = transform;
        }
      }
      return out;
    }

    // Fallback: derive robot_from_camera from paired tag poses:
    //   robot_from_tag = robot_from_camera ∘ camera_from_tag
    // => robot_from_camera = robot_from_tag ∘ tag_from_camera
    //
    // This works for "Live Tag Poses" profiles that output `tag_in_camera` + `tag_in_robot` but do not
    // provide field-space camera poses.
    const tagInCamera = activeSolverOutputs?.tagInCamera ?? null;
    const tagInRobot = activeSolverOutputs?.tagInRobot ?? null;
    if (!tagInCamera || !tagInRobot) return null;
    if (tagInCamera.length === 0 || tagInRobot.length === 0) return null;

    const cameraByKey = new Map<string, (typeof tagInCamera)[number]>();
    for (const det of tagInCamera) {
      cameraByKey.set(`${det.sourceId}|${det.cameraUid}|${det.tagId}`, det);
    }

    const best: Record<string, { dist2: number; transform: PoseTransform }> = {};
    for (const detRobot of tagInRobot) {
      const detCam = cameraByKey.get(`${detRobot.sourceId}|${detRobot.cameraUid}|${detRobot.tagId}`) ?? null;
      if (!detCam) continue;

      const camPose = detCam.pose;
      const robotPose = detRobot.pose;
      const cameraFromTag: PoseTransform = { position: [camPose.translation.x, camPose.translation.y, camPose.translation.z], quaternion: camPose.rotation.quaternion };
      const robotFromTag: PoseTransform = { position: [robotPose.translation.x, robotPose.translation.y, robotPose.translation.z], quaternion: robotPose.rotation.quaternion };
      const tagFromCamera = invertTransform(cameraFromTag);
      const robotFromCamera = composeTransforms(robotFromTag, tagFromCamera);

      const dist2 =
        cameraFromTag.position[0] * cameraFromTag.position[0] +
        cameraFromTag.position[1] * cameraFromTag.position[1] +
        cameraFromTag.position[2] * cameraFromTag.position[2];

      const key = (detRobot.cameraUid || detRobot.sourceId).trim();
      if (!key) continue;
      const current = best[key];
      if (!current || dist2 < current.dist2) {
        best[key] = { dist2, transform: robotFromCamera };
      }
    }

    const out: Record<string, { position: Vec3; quaternion?: PoseQuaternion }> = {};
    for (const [key, entry] of Object.entries(best)) {
      const transform = { position: entry.transform.position, quaternion: entry.transform.quaternion };
      for (const variant of cameraKeyVariants(key)) {
        out[variant] = transform;
      }
    }
    return Object.keys(out).length ? out : null;
  });

  const solverCameraTransformsForViewer = $derived.by<Record<string, { position: Vec3; quaternion?: PoseQuaternion }> | null>(() => {
    if (baseFrame === 'field') return solverCameraTransforms;
    if (baseFrame === 'robot') return solverRobotCameraTransforms;
    return null;
  });

  const solverRobotTransform = $derived.by<PoseTransform | null>(() => {
    const pose = activeSolverOutputs?.robotInField?.pose ?? null;
    if (!pose) return null;
    return {
      position: [pose.translation.x, pose.translation.y, pose.translation.z],
      quaternion: pose.rotation.quaternion
    };
  });

  const viewProfileOverlays = $derived.by(() =>
    coordinateSpace === 'camera_in_field'
      ? []
      : buildViewProfileOverlays({
          baseFrame,
          showRobotContext,
          coordinateSpace,
          viewProfiles,
          solveResponsesByProfile,
          profiles: $profiles,
          profileIndexById,
          profileColors: PROFILE_COLORS
        })
  );

  const viewerCameraTransforms = $derived.by<Record<string, { position: Vec3; quaternion?: PoseQuaternion }> | null>(() => {
    const baseTransforms = buildViewerCameraTransforms({
      baseFrame,
      solverCameraTransforms: solverCameraTransformsForViewer,
      viewerCameras,
      separateCameras,
      primaryCameraKey
    });

    // For IMU-only/local-space workflows, rotate the camera model from live IMU orientation so
    // tag_in_camera/tag_in_robot views visibly track motion even without tag detections.
    if (baseFrame !== 'field') {
      const imuSource = activeImuSource;
      const imuQuaternion = imuSampleQuaternion(imuRotationSample);
      if (!imuSource || !imuQuaternion) {
        return baseTransforms;
      }

      const out: Record<string, { position: Vec3; quaternion?: PoseQuaternion }> = {
        ...(baseTransforms ?? {})
      };
      const keys = sourceKeys(imuSource);
      const existing =
        keys
          .map((key) => out[key])
          .find(
            (entry): entry is { position: Vec3; quaternion?: PoseQuaternion } =>
              Boolean(entry && Array.isArray(entry.position))
          ) ?? null;
      const transform: { position: Vec3; quaternion: PoseQuaternion } = {
        position: existing?.position ?? ([0, 0, 0] as Vec3),
        quaternion: imuQuaternion
      };
      for (const key of keys) {
        out[key] = transform;
      }
      return out;
    }

    // In field views, never leave cameras at the origin when a robot field pose exists.
    // If solve output does not include explicit camera_in_field poses for a camera, fall back
    // to composing robot_in_field with that camera's rig extrinsics.
    if (!solverRobotTransform) {
      return baseTransforms;
    }

    const out: Record<string, { position: Vec3; quaternion?: PoseQuaternion }> = { ...(baseTransforms ?? {}) };

    const keysForRigCamera = (camera: RigCameraInfo): string[] => {
      const set = new Set<string>();
      const add = (value: string | null | undefined) => {
        for (const key of cameraKeyVariants(value)) {
          set.add(key);
        }
      };
      add(camera.uid);
      add(camera.cameraUid ?? null);
      add(camera.streamId ?? null);
      add(camera.driverCameraId ?? null);
      add(camera.hardwareId ?? null);
      add(camera.streamAlias ?? null);
      return Array.from(set.values());
    };

    const keysForViewerCamera = (camera: RigCameraInfo): string[] => {
      const set = new Set<string>(keysForRigCamera(camera));
      for (const source of viewOverlaySources) {
        const sourceKeys = new Set<string>();
        const addSource = (value: string | null | undefined) => {
          for (const key of cameraKeyVariants(value)) sourceKeys.add(key);
        };
        addSource(source.id);
        addSource(source.cameraUid);
        addSource(source.streamId);
        addSource(source.cameraPath);
        for (const key of source.cameraKeys ?? []) addSource(key);

        const matches = Array.from(sourceKeys.values()).some((key) => set.has(key));
        if (matches) {
          for (const key of sourceKeys.values()) set.add(key);
        }
      }
      return Array.from(set.values());
    };

    const findRigCamera = (keys: string[]): RigCameraInfo | null => {
      const lookup = new Set(keys);
      for (const camera of rigLayoutState.layout.cameras) {
        const candidates = keysForRigCamera(camera);
        if (candidates.some((candidate) => lookup.has(candidate))) {
          return camera;
        }
      }
      return null;
    };

    for (const camera of viewerCameras) {
      const keys = keysForViewerCamera(camera);
      if (keys.some((key) => Boolean(out[key]))) continue;

      const rigCamera = findRigCamera(keys);
      const pose = rigCamera?.pose ?? null;
      const robotFromCamera = rigPoseToViewerTransform(pose);
      let fieldFromCamera: PoseTransform | null = null;
      if (robotFromCamera) {
        fieldFromCamera = composeTransforms(solverRobotTransform, robotFromCamera);
      } else {
        const lookup = new Set(keys);
        const sourcePool = [...viewOverlaySources, ...selectedSources];
        const matchedSource =
          sourcePool.find((source) =>
            sourceKeys(source).some((key) => lookup.has(key))
          ) ?? null;
        // IMU-only feeds can produce robot_in_field without any camera extrinsics.
        // Render camera-at-robot-origin so field views still show camera motion.
        if (matchedSource && isImuSource(matchedSource)) {
          fieldFromCamera = solverRobotTransform;
        }
      }
      if (!fieldFromCamera) continue;
      const transform = { position: fieldFromCamera.position, quaternion: fieldFromCamera.quaternion };
      for (const key of keys) {
        out[key] = transform;
      }
    }

    return Object.keys(out).length > 0 ? out : baseTransforms;
  });

  const viewerRenderableCameras = $derived.by<RigCameraInfo[]>(() => {
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
        sourcePool.find((source) =>
          sourceKeys(source).some((candidate) => candidate === key)
        ) ?? null;
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
  });

  $effect(() => {
    if (baseFrame !== 'field') {
      lastViewerCameraTransforms = null;
      lastViewerRenderableCameras = [];
      return;
    }
    const transforms = viewerCameraTransforms ?? null;
    if (transforms && Object.keys(transforms).length > 0) {
      lastViewerCameraTransforms = transforms;
    }
    if (viewerRenderableCameras.length > 0) {
      lastViewerRenderableCameras = viewerRenderableCameras;
    }
  });

  const viewerCameraTransformsForRender = $derived.by<Record<string, { position: Vec3; quaternion?: PoseQuaternion }> | null>(() => {
    if (baseFrame !== 'field') return viewerCameraTransforms;
    const live = viewerCameraTransforms ?? null;
    if (live && Object.keys(live).length > 0) return live;
    return lastViewerCameraTransforms;
  });

  const viewerRenderableCamerasForRender = $derived.by<RigCameraInfo[]>(() => {
    if (baseFrame !== 'field') return viewerRenderableCameras;
    return viewerRenderableCameras.length > 0 ? viewerRenderableCameras : lastViewerRenderableCameras;
  });

  const viewerCameraGhostActive = $derived.by<boolean>(() => {
    if (baseFrame !== 'field') return false;
    const liveTransforms = viewerCameraTransforms ?? null;
    const hasLiveTransforms = Boolean(liveTransforms && Object.keys(liveTransforms).length > 0);
    const hasLiveCameras = viewerRenderableCameras.length > 0;
    const ghostTransforms = lastViewerCameraTransforms ?? null;
    const hasGhostTransforms = Boolean(ghostTransforms && Object.keys(ghostTransforms).length > 0);
    const hasGhostCameras = lastViewerRenderableCameras.length > 0;
    if (!hasGhostTransforms || !hasGhostCameras) return false;
    return !hasLiveTransforms || !hasLiveCameras;
  });

  const viewerShowCameras = $derived.by(() => {
    return viewerRenderableCamerasForRender.length > 0;
  });

  const viewerRobotTransform = $derived.by<{ position: Vec3; quaternion?: PoseQuaternion } | null>(() =>
    buildViewerRobotTransform({
      baseFrame,
      showRobotContext,
      coordinateSpace,
      viewProfileOverlays,
      solverRobotTransform
    })
  );
  const viewerRobotTransformForGhost = $derived.by<{ position: Vec3; quaternion?: PoseQuaternion } | null>(() => {
    if (viewerRobotTransform) return viewerRobotTransform;
    if (baseFrame === 'field') return solverRobotTransform;
    return null;
  });

  const viewerShowRobot = $derived.by(() =>
    coordinateSpace === 'camera_in_field'
      ? false
      : buildViewerShowRobot({
          baseFrame,
          showRobotContext,
          coordinateSpace
        })
  );

  const cameraPovOptionsBase = $derived.by<Array<Omit<CameraPovOption, 'available' | 'ghost'>>>(() => {
    const out: Array<Omit<CameraPovOption, 'available' | 'ghost'>> = [];
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
  });

  const cameraPovByOptionId = $derived.by<Record<string, CameraPovState>>(() => {
    const out: Record<string, CameraPovState> = {};
    const profileById = new Map($profiles.map((profile) => [profile.id, profile]));
    const sourceById = new Map(sources.map((source) => [source.id, source]));

    const toPoseTransform = (pose: { translation: { x: number; y: number; z: number }; rotation: { quaternion: PoseQuaternion } } | null | undefined): PoseTransform | null => {
      if (!pose) return null;
      return {
        position: [pose.translation.x, pose.translation.y, pose.translation.z],
        quaternion: pose.rotation.quaternion
      };
    };

    for (const option of cameraPovOptionsBase) {
      const profile = profileById.get(option.profileId) ?? null;
      const source = sourceById.get(option.sourceId) ?? null;
      if (!profile || !source) continue;

      const outputs = outputsForProfile(profile);
      if (!outputs) continue;

      const sourceKeySet = new Set(sourceKeys(source));
      const rigCamera = (() => {
        let best: { camera: RigCameraInfo; score: number } | null = null;
        for (const camera of rigLayoutState.layout.cameras) {
          const score = rigCameraKeys(camera).reduce(
            (count, key) => count + (sourceKeySet.has(key) ? 1 : 0),
            0
          );
          if (score <= 0) continue;
          if (!best || score > best.score) {
            best = { camera, score };
          }
        }
        return best?.camera ?? rigCameraForSource(source);
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

      const rigRobotFromCamera = (() => {
        const pose = rigCamera?.pose ?? null;
        return rigPoseToViewerTransform(pose);
      })();

      const fieldFromCamera =
        (fieldFromRobot && rigRobotFromCamera ? composeTransforms(fieldFromRobot, rigRobotFromCamera) : null) ??
        cameraInField;

      let transform: PoseTransform | null = null;
      if (coordinateSpace === 'tag_in_camera') {
        transform = identityTransform();
      } else if (coordinateSpace === 'tag_in_robot') {
        // In robot-space viewing, keep POV locked to rig-local camera pose.
        // Do not re-derive from field solves, which can drift or jump when tags are sparse.
        transform = rigRobotFromCamera;
      } else {
        transform = fieldFromCamera;
      }
      if (!transform) continue;

      const cameraKey =
        (source.cameraUid || source.streamId || source.id || rigCamera?.uid || '').trim() || null;
      const allKeys = new Set<string>([...Array.from(sourceKeySet.values()), ...rigCameraKeys(rigCamera)]);
      const intrinsicsEntry = Array.from(allKeys.values())
        .map((key) => cameraIntrinsicsByKey[key] ?? null)
        .find(
          (entry): entry is { undistorted: CameraPovIntrinsics | null; raw: CameraPovIntrinsics | null } =>
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
  });

  const cameraPovOptions = $derived.by<CameraPovOption[]>(() =>
    cameraPovOptionsBase.map((option) => {
      const hasLivePose = Boolean(cameraPovByOptionId[option.id]);
      const hasGhostPose = !hasLivePose && Boolean(lastCameraPovByOptionId[option.id]);
      return {
        ...option,
        available: hasLivePose || hasGhostPose,
        ghost: hasGhostPose
      };
    })
  );

  const selectedCameraPov = $derived.by<CameraPovState | null>(() => {
    const selected = cameraPovSelectionId.trim();
    if (!selected) return null;
    if (selected === ROBOT_FOLLOW_POV_OPTION_ID) return null;
    return cameraPovByOptionId[selected] ?? lastCameraPovByOptionId[selected] ?? null;
  });
  const selectedLiveCameraPov = $derived.by<CameraPovState | null>(() => {
    const selected = cameraPovSelectionId.trim();
    if (!selected) return null;
    if (selected === ROBOT_FOLLOW_POV_OPTION_ID) return null;
    return cameraPovByOptionId[selected] ?? null;
  });

  const viewerCameraPovEnabled = $derived.by(() => Boolean(selectedCameraPov));
  const viewerCameraPovTransform = $derived.by<PoseTransform | null>(() => selectedCameraPov?.transform ?? null);
  const viewerCameraPovIntrinsics = $derived.by<CameraPovIntrinsics | null>(() =>
    selectedPovIntrinsics(selectedCameraPov, cameraPovFovMode)
  );
  const viewerCameraPovApplyFov = $derived.by<boolean>(() => cameraPovFovMode !== 'none');
  const viewerCameraPovForwardSign = $derived.by<1 | -1>(() => {
    const selected = selectedCameraPov;
    const intrinsics = selectedPovIntrinsics(selected, cameraPovFovMode);
    if (!selected) return 1;

    const profile = $profiles.find((entry) => entry.id === selected.profileId) ?? null;
    if (!profile) return 1;
    const outputs = outputsForProfile(profile);
    if (!outputs) return 1;

    const sourceById = new Map(sources.map((source) => [source.id, source]));
    const detectionsForSpace =
      coordinateSpace === 'camera_in_field' || coordinateSpace === 'robot_in_field'
        ? fieldDetectionsForOutputs(outputs, sourceById)
        : detectionPosesForSpace(outputs, coordinateSpace);
    const detections = detectionsForSpace.filter((detection) => detection.sourceId === selected.sourceId);
    if (detections.length === 0) return 1;

    const scorePositive = detections.reduce(
      (count, detection) =>
        count + (detectionInsidePovFov(detection, coordinateSpace, selected, intrinsics, 1) ? 1 : 0),
      0
    );
    const scoreNegative = detections.reduce(
      (count, detection) =>
        count + (detectionInsidePovFov(detection, coordinateSpace, selected, intrinsics, -1) ? 1 : 0),
      0
    );
    if (scorePositive !== scoreNegative) {
      return scorePositive > scoreNegative ? 1 : -1;
    }

    let positive = 0;
    let negative = 0;
    for (const detection of detections) {
      const position = cameraFramePositionForDetection(detection, coordinateSpace, selected);
      const z = position?.[2];
      if (z == null || !Number.isFinite(z) || Math.abs(z) < 1e-6) continue;
      if (z > 0) {
        positive += 1;
      } else {
        negative += 1;
      }
    }
    return negative > positive ? -1 : 1;
  });
  const minimapPoseDot = $derived.by<{ position: Vec3; color: string | null } | null>(() => {
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
        if (!transform) continue;
        return { position: toGround(transform.position), color };
      }

      const fallback = Object.values(transforms)[0] ?? null;
      if (!fallback) return null;
      return { position: toGround(fallback.position), color };
    }

    if (baseFrame === 'camera' || baseFrame === 'robot') {
      return { position: [0, 0.04, 0], color };
    }

    return null;
  });

  $effect(() => {
    const current = cameraPovByOptionId;
    const next: Record<string, CameraPovState> = { ...lastCameraPovByOptionId };
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

    const validOptions = new Set(cameraPovOptionsBase.map((option) => option.id));
    for (const key of Object.keys(next)) {
      if (validOptions.has(key)) continue;
      delete next[key];
      changed = true;
    }

    if (changed) {
      lastCameraPovByOptionId = next;
    }
  });

  $effect(() => {
    const selected = cameraPovSelectionId.trim();
    if (!selected) return;
    if (selected === ROBOT_FOLLOW_POV_OPTION_ID) return;
    if (cameraPovOptionsBase.some((option) => option.id === selected)) return;
    cameraPovSelectionId = '';
  });

  const viewerMarkers = $derived.by<LocalizationMarker[]>(() => liveMarkers);

  const detectedFieldTagDetections = $derived.by<ViewerDetectionPose[]>(() => {
    if (baseFrame !== 'field') return [];
    const activeProfileId = $activeProfile?.id ?? null;
    const activeSolverKey = activeSolverConfig?.id ?? activeSolverId;
    const selectedPov = selectedCameraPov;
    const povIntrinsics = selectedPovIntrinsics(selectedPov, cameraPovFovMode);
    const povForwardSign = viewerCameraPovForwardSign;
    const povProfileId = selectedPov?.profileId ?? null;
    const povSourceId = selectedPov?.sourceId ?? null;
    const sourceById = new Map(sources.map((source) => [source.id, source]));
    const allDetections = viewProfiles.flatMap<ViewerDetectionPose>((profile) => {
      const resp = solveResponsesByProfile[profile.id] ?? (profile.id === solveResponse?.profileId ? solveResponse : null);
      const solvers = resp?.solvers ?? [];
      const outputs = (() => {
        if (profile.id === activeProfileId && activeSolverKey) {
          return solvers.find((solver) => solver.id === activeSolverKey)?.outputs ?? solvers[0]?.outputs ?? null;
        }
        return solvers[0]?.outputs ?? null;
      })();
      const color = profileColorForId(profile.id, $profiles, profileIndexById, PROFILE_COLORS);
      return fieldDetectionsForOutputs(outputs, sourceById).map((det) => ({
        ...det,
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
  });

  const referenceMarkersForViewer = $derived.by<LocalizationMarker[]>(() => {
    if (baseFrame !== 'field') return [];
    const doc = activeFieldMapDoc;
    if (!doc) return [];
    const detectedColorByTagId = new Map<string, string>();
    for (const detection of detectedFieldTagDetections) {
      const tagKey = String(detection.tagId);
      if (detectedColorByTagId.has(tagKey)) continue;
      detectedColorByTagId.set(tagKey, detection.color ?? '#38bdf8');
    }
    return doc.markers.map((marker) => {
      const tagKey = String(marker.id);
      const highlightColor = detectedColorByTagId.get(tagKey) ?? null;
      return {
        id: `map:${doc.id}:${marker.id}`,
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
  });

  const viewerTagLineMarkers = $derived.by<LocalizationMarker[]>(() => {
    if (baseFrame !== 'field') return liveMarkers;
    if (!showTagLines) return [];
    const doc = activeFieldMapDoc;
    if (!doc) return [];
    const markerByTagId = new Map<string, (typeof doc.markers)[number]>();
    for (const marker of doc.markers) {
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
  });

  function fieldSpacePoseForOutputs(
    outputs: LocalizationSolverOutputs | null,
    space: LocalizationPoseSpace
  ): PoseTransform | null {
    if (!outputs) return null;
    if (space === 'robot_in_field') {
      const p = outputs.robotInField?.pose ?? null;
      return p
        ? ({ position: [p.translation.x, p.translation.y, p.translation.z], quaternion: p.rotation.quaternion } as PoseTransform)
        : null;
    }
    if (space === 'camera_in_field') {
      const entry = outputs.cameraInField?.[0] ?? null;
      return entry
        ? ({ position: [entry.pose.translation.x, entry.pose.translation.y, entry.pose.translation.z], quaternion: entry.pose.rotation.quaternion } as PoseTransform)
        : null;
    }
    return null;
  }

  function poseTransformEquals(a: PoseTransform | null | undefined, b: PoseTransform | null | undefined): boolean {
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
  }

  function localTagPoseCacheKey(profileId: string, space: LocalizationPoseSpace, detection: LocalizationDetectionPose): string {
    const cameraUid = (detection.cameraUid ?? '').trim();
    return `${profileId}:${space}:${detection.sourceId}:${cameraUid}:${detection.tagId}`;
  }

  function localTagPoseValues(detection: LocalizationDetectionPose): string {
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
    return `id:${detection.tagId} (${formatNumber(tx, 3)}, ${formatNumber(ty, 3)}, ${formatNumber(tz, 3)}) (${formatNumber(roll, 1)}, ${formatNumber(pitch, 1)}, ${formatNumber(yaw, 1)}) d:${formatNumber(distanceM, 3)}m`;
  }

  $effect(() => {
    const isFieldSpace = coordinateSpace === 'camera_in_field' || coordinateSpace === 'robot_in_field';
    const nowMs = Date.now();
    const expireBeforeMs = nowMs - LOCAL_TAG_POSE_LINGER_MS;
    const next: Record<string, LocalTagPoseOverlayEntry> = {};
    let changed = false;

    for (const [key, entry] of Object.entries(lastLocalTagPoseByKey)) {
      if (entry.timestampMs < expireBeforeMs) {
        changed = true;
        continue;
      }
      next[key] = entry;
    }

    if (!isFieldSpace) {
      for (const profile of viewProfilesWithSources) {
        const outputs = outputsForProfile(profile);
        const detections = detectionPosesForSpace(outputs, coordinateSpace);
        const color = profileColorForId(profile.id, $profiles, profileIndexById, PROFILE_COLORS);
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
            nowMs - prev.timestampMs >= LOCAL_TAG_POSE_REFRESH_MS;
          if (!needsRefresh) continue;
          next[key] = { key, profileId: profile.id, space: coordinateSpace, color, label, values, timestampMs: nowMs };
          changed = true;
        }
      }
    }

    if (!changed && Object.keys(next).length !== Object.keys(lastLocalTagPoseByKey).length) {
      changed = true;
    }
    if (changed) {
      lastLocalTagPoseByKey = next;
    }
  });

  $effect(() => {
    const isFieldSpace = coordinateSpace === 'camera_in_field' || coordinateSpace === 'robot_in_field';
    const nowMs = Date.now();
    const expireBeforeMs = nowMs - FIELD_POSE_LINGER_MS;
    const next: Record<string, FieldSpacePoseOverlayEntry> = {};
    let changed = false;

    for (const [key, entry] of Object.entries(lastFieldSpacePoseByProfileSpace)) {
      if (entry.timestampMs < expireBeforeMs) {
        changed = true;
        continue;
      }
      next[key] = entry;
    }

    if (isFieldSpace) {
      for (const profile of viewProfilesWithSources) {
        const key = `${profile.id}:${coordinateSpace}`;
        const livePose = fieldSpacePoseForOutputs(outputsForProfile(profile), coordinateSpace);
        if (!livePose) continue;
        const prev = next[key] ?? null;
        const needsRefresh =
          !prev ||
          !poseTransformEquals(prev.pose, livePose) ||
          nowMs - prev.timestampMs >= FIELD_POSE_REFRESH_MS;
        if (!needsRefresh) continue;
        next[key] = { pose: livePose, timestampMs: nowMs };
        changed = true;
      }
    }

    if (!changed && Object.keys(next).length !== Object.keys(lastFieldSpacePoseByProfileSpace).length) {
      changed = true;
    }

    if (changed) {
      lastFieldSpacePoseByProfileSpace = next;
    }
  });

  const targetSpaceOverlay = $derived.by(() => {
    if (showOutputsOverlay) return null;

    const isFieldSpace = coordinateSpace === 'camera_in_field' || coordinateSpace === 'robot_in_field';

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

    // Field solves are maintained in viewer space (+X left, +Y up, +Z forward).
    // For field-space numeric readouts, present WPILib-style axes:
    //   X forward, Y left, Z up  =>  [vz, vx, vy]
    const viewerPositionToWpilib = (position: Vec3): Vec3 => [position[2], position[0], position[1]];

    const fmtValues = (pose: PoseTransform | null, originFromCenter: PoseTransform): string => {
      if (!pose) {
        return `x:${num(null, 8, 3)} y:${num(null, 8, 3)} z:${num(null, 8, 3)}  r:${num(null, 7, 1)} p:${num(null, 7, 1)} y:${num(null, 7, 1)}`;
      }
      const resolved = composeTransforms(originFromCenter, pose);
      const pos = viewerPositionToWpilib(resolved.position);
      const euler = quaternionToEulerDegreesXYZ(resolved.quaternion);
      return `x:${num(pos[0], 8, 3)} y:${num(pos[1], 8, 3)} z:${num(pos[2], 8, 3)}  r:${num(euler.roll, 7, 1)} p:${num(euler.pitch, 7, 1)} y:${num(euler.yaw, 7, 1)}`;
    };

    const rows = viewProfilesWithSources.map((profile) => {
      const outputs = outputsForProfile(profile);
      const key = `${profile.id}:${coordinateSpace}`;
      const livePose = fieldSpacePoseForOutputs(outputs, coordinateSpace);
      const cached = lastFieldSpacePoseByProfileSpace[key] ?? null;
      const pose = livePose ?? cached?.pose ?? null;

      const color = profileColorForId(profile.id, $profiles, profileIndexById, PROFILE_COLORS);
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
    return { header, rows: rows.map(({ originLabel: _originLabel, ...row }) => row) };
  });


  

  $effect(() => {
    // Viewer marker visibility is controlled by the "Views" overlay toggles (profile.viewEnabled).
    // If a profile is unchecked there, its detections should not appear in the 3D viewer.
    const isFieldSpace = coordinateSpace === 'camera_in_field' || coordinateSpace === 'robot_in_field';
    if (isFieldSpace) {
      rawMarkers = [];
      return;
    }
    const activeProfileId = $activeProfile?.id ?? null;
    const activeSolverKey = activeSolverConfig?.id ?? activeSolverId;
    const selectedPov = selectedCameraPov;
    const povIntrinsics = selectedPovIntrinsics(selectedPov, cameraPovFovMode);
    const povForwardSign = viewerCameraPovForwardSign;
    const povProfileId = selectedPov?.profileId ?? null;
    const povSourceId = selectedPov?.sourceId ?? null;
    const allDetections = viewProfiles.flatMap<ViewerDetectionPose>((profile) => {
      const resp = solveResponsesByProfile[profile.id] ?? (profile.id === solveResponse?.profileId ? solveResponse : null);
      const solvers = resp?.solvers ?? [];
      const outputs = (() => {
        if (profile.id === activeProfileId && activeSolverKey) {
          return solvers.find((solver) => solver.id === activeSolverKey)?.outputs ?? solvers[0]?.outputs ?? null;
        }
        return solvers[0]?.outputs ?? null;
      })();
      const color = profileColorForId(profile.id, $profiles, profileIndexById, PROFILE_COLORS);
      return detectionPosesForSpace(outputs, coordinateSpace).map((det) => ({
        ...det,
        profileId: profile.id,
        color
      }));
    });
    const detections = (() => {
      if (!povSourceId || !povProfileId) return allDetections;
      const scoped = allDetections.filter(
        (detection) => detection.sourceId === povSourceId && detection.profileId === povProfileId
      );
      const inFov = scoped.filter((detection) =>
        detectionInsidePovFov(detection, coordinateSpace, selectedPov, povIntrinsics, povForwardSign)
      );
      // Never blank non-field spaces because of intrinsics/FOV mismatch; if culling removes everything,
      // show the selected camera's detections.
      return inFov.length > 0 ? inFov : scoped;
    })();
    rawMarkers = markersFromDetections({
      detections,
      sources,
      selectedSources: viewOverlaySources,
      colors: SOURCE_COLORS
    });
  });

  $effect(() => {
    const profile = $activeProfile;
    if (!profile) return;
    void loadPipelineStatus(profile.id);
    if (profile.pipelineTemplateId) {
      void loadPipelineOutputs(profile.id);
    } else {
      pipelineOutputs = [];
    }
  });


  onMount(() => {
    void rigLayoutStore.refresh();
    connectLiveUpdates();
    void (async () => {
      await loadLocalizationConfig();
      await loadSources();
      await loadStreamsSnapshot();
      await loadPipelineTemplates();
      const profileId = $localizationConfig?.activeProfileId ?? $localizationConfig?.profiles?.[0]?.id ?? null;
      await loadPipelineStatus(profileId);
      const profile = $localizationConfig?.profiles?.find((entry) => entry.id === profileId) ?? $localizationConfig?.profiles?.[0] ?? null;
      if (profile?.pipelineTemplateId) {
        await loadPipelineOutputs(profile.id);
      }
    })();
    void loadLocalizationViewers();
    localizationStorage.loadFromStorage();
    selectedCustomFieldId = $customFields[0]?.id ?? null;
    selectedCustomFieldOriginId = $customFields[0]?.origins[0]?.id ?? null;
    void loadFieldMapList();
    if (browser) {
      visibilityHandler = () => {
        if (document.hidden) {
          pollVisibilityPaused = true;
          feedPoller.stop();
        } else if (pollVisibilityPaused) {
          pollVisibilityPaused = false;
          if (hasAnyFeedSources) {
            feedPoller.schedule(0);
          }
        }
      };
      document.addEventListener('visibilitychange', visibilityHandler);
    }
  });

  $effect(() => {
    if (!showCustomFieldsOverlay) return;
    mapAssignId = selectedFieldMapId ?? '';
  });

  onDestroy(() => {
    disconnectLiveUpdates();
    rigLayoutUnsubscribe();
    feedPoller.stop();
    for (const cleanup of streamMetricsCleanup.values()) {
      cleanup();
    }
    streamMetricsCleanup.clear();
    if (visibilityHandler) {
      document.removeEventListener('visibilitychange', visibilityHandler);
      visibilityHandler = null;
    }
  });

  $effect(() => {
    const id = selectedFieldMapId;
    if (!id) return;
    void ensureFieldMapLoaded(id);
  });

  $effect(() => {
    const fieldSpaceActive =
      coordinateSpace === 'camera_in_field' || coordinateSpace === 'robot_in_field';
    if (!fieldSpaceActive) {
      if (viewMode !== 'isolated') {
        viewMode = 'isolated';
      }
      return;
    }
    if (!selectedFieldMapId) {
      viewMode = 'isolated';
      return;
    }
    const matchingField = $customFields.find((field) => field.mapId === selectedFieldMapId) ?? null;
    // Field-space solves should default to frc-field so profile fieldOrigin (blue/red/center/custom)
    // controls the solve-space transform. Auto-switching into custom-field forces center-origin maps.
    if (viewMode !== 'frc-field') {
      viewMode = 'frc-field';
    }
    if (matchingField) {
      if (selectedCustomFieldId !== matchingField.id) {
        selectedCustomFieldId = matchingField.id;
      }
      if (!selectedCustomFieldOriginId || !matchingField.origins.some((origin) => origin.id === selectedCustomFieldOriginId)) {
        selectedCustomFieldOriginId = matchingField.origins[0]?.id ?? null;
      }
    }
  });
</script>

<div class="flex h-full min-h-0 flex-1 flex-col gap-4 overflow-hidden">
  <section class="flex min-h-0 flex-1 gap-4 overflow-hidden lg:gap-6">
    <aside class="w-full shrink-0 space-y-3 overflow-visible rounded border border-surface-800/60 bg-surface-950/40 p-3 text-xs text-surface-400 lg:max-w-[16rem] xl:max-w-[16.75rem] 2xl:max-w-[17.5rem]">
    <button
      class="btn btn-xs preset-filled-primary-500 w-full uppercase tracking-[0.22em]"
      type="button"
      onclick={addProfile}
    >
      New Profile
    </button>

    <SidebarSearchSection
      label="Search profiles"
      placeholder="Name or id"
      description="Name, id"
      bind:value={profileSearch}
      ariaLabel="Search profiles"
      size="compact"
    />

    <div>
      <p class="text-micro uppercase tracking-[0.22em] text-surface-500">Profiles</p>
      <div class="mt-2 space-y-1.5">
        {#if $profiles.length === 0}
          <p class="rounded border border-dashed border-surface-700/70 bg-surface-950/40 p-2.5 text-micro-tight text-surface-500">
            No profiles yet.
          </p>
        {:else if filteredProfiles.length === 0}
          <p class="rounded border border-dashed border-surface-700/70 bg-surface-950/40 p-2.5 text-micro-tight text-surface-500">
            No profiles match this search/filter.
          </p>
        {:else}
          {#each filteredProfiles as profile (profile.id)}
            {@const isSelected = $activeProfileId === profile.id}
            {@const enabled = profile.viewEnabled === true}
            {@const color = profileColorForId(profile.id, $profiles, profileIndexById, PROFILE_COLORS)}
            {@const supported = (profileSupportedSpacesById?.[profile.id] ?? []).includes(coordinateSpace)}
            <div
              class={`w-full rounded border px-2.5 py-1.5 text-left transition ${
                isSelected
                  ? 'border-primary-400/70 bg-primary-500/10 text-white shadow-lg shadow-primary-500/20'
                  : 'border-surface-700/40 text-surface-300 hover:border-surface-600/80'
              } ${supported ? '' : 'opacity-60'}`}
              role="button"
              tabindex="0"
              aria-pressed={isSelected ? 'true' : 'false'}
              onclick={() => void setActiveProfile(profile.id)}
              onkeydown={(event) => {
                if (event.key === 'Enter' || event.key === ' ') {
                  event.preventDefault();
                  void setActiveProfile(profile.id);
                }
              }}
            >
              <div class="flex items-start gap-3">
                <div
                  class={`relative mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-full border transition ${
                    isSelected ? 'border-white/40 hover:border-primary-200' : 'border-white/20 hover:border-primary-200/70'
                  }`}
                  title={`Set color for ${profile.name}`}
                >
                  <span class="h-3 w-3 rounded-full border border-white/40" style={`background-color: ${color};`}></span>
                  <input
                    type="color"
                    value={color}
                    class="absolute inset-0 cursor-pointer opacity-0"
                    aria-label={`Set color for ${profile.name}`}
                    onchange={(event) => setProfileColor(profile.id, (event.currentTarget as HTMLInputElement).value)}
                    onclick={(event) => event.stopPropagation()}
                  />
                </div>
                <div class="min-w-0 flex-1">
                  <div class="flex flex-wrap items-center gap-2">
                    <p class={`truncate text-xs font-semibold ${isSelected ? 'text-white' : 'text-surface-100'}`}>
                      {profile.name}
                    </p>
                    <button
                      type="button"
                      class={`shrink-0 rounded border px-1.5 py-[1px] text-micro-tight uppercase tracking-[0.16em] transition ${
                        enabled
                          ? 'border-emerald-500/60 bg-emerald-500/10 text-emerald-100 hover:border-emerald-400/80 hover:bg-emerald-500/20'
                          : 'border-surface-600/60 bg-surface-800/40 text-surface-300 hover:border-surface-500/80 hover:bg-surface-700/50'
                      }`}
                      aria-label={`Toggle ${profile.name} visibility in 3D view`}
                      title={enabled ? `Hide ${profile.name} in 3D view` : `Show ${profile.name} in 3D view`}
                      onclick={(event) => {
                        event.stopPropagation();
                        setProfileViewEnabled(profile.id, !enabled);
                      }}
                    >
                      {enabled ? 'Visible' : 'Hidden'}
                    </button>
                    {#if !supported}
                      <span class="shrink-0 rounded border border-amber-500/60 bg-amber-500/10 px-1.5 py-[1px] text-micro-tight uppercase tracking-[0.16em] text-amber-100">
                        Unsupported
                      </span>
                    {/if}
                  </div>
                </div>
                <div class="flex items-center gap-1">
                  <button
                    class={`flex h-7 w-7 shrink-0 items-center justify-center rounded-full border text-rose-300 transition hover:text-rose-100 disabled:cursor-not-allowed disabled:opacity-40 ${
                      isSelected
                        ? 'border-rose-300/40 hover:border-rose-200'
                        : 'border-rose-500/40 hover:border-rose-400'
                    }`}
                    type="button"
                    aria-label={`Delete profile ${profile.name}`}
                    title={$profiles.length <= 1 ? 'At least one profile is required' : `Delete profile ${profile.name}`}
                    disabled={$profiles.length <= 1}
                    onclick={(event) => {
                      event.stopPropagation();
                      openDeleteProfileModal(profile);
                    }}
                  >
                    🗑
                  </button>
                  <button
                    class={`flex h-7 w-7 shrink-0 items-center justify-center rounded-full border text-surface-400 transition hover:text-primary-100 ${
                      isSelected
                        ? 'border-white/30 hover:border-primary-300'
                        : 'border-surface-600/60 hover:border-primary-400'
                    }`}
                    type="button"
                    aria-label={`Open settings for ${profile.name}`}
                    title={`Open settings for ${profile.name}`}
                    onclick={(event) => {
                      event.stopPropagation();
                      void setActiveProfile(profile.id);
                      showOutputsOverlay = true;
                      showMetricsOverlay = false;
                    }}
                  >
                    ⚙
                  </button>
                </div>
              </div>
            </div>
          {/each}
        {/if}
      </div>
    </div>
    </aside>

    <div class="min-w-0 flex flex-1 flex-col">
      <LocalizationWorkspace
      viewersComponent={LocalizationViewersComponent}
	      markers={viewerMarkers}
	      tagLineMarkers={viewerTagLineMarkers}
	      referenceMarkers={referenceMarkersForViewer}
	      mode={viewMode}
	      bumperNumber={bumperId}
	      bumperColor={viewerAccentColor}
	      robotOverlays={viewProfileOverlays}
	      robot={rigLayoutState.layout.robot}
	      cameras={viewerRenderableCamerasForRender}
	      cameraTransforms={viewerCameraTransformsForRender}
	      robotTransform={viewerRobotTransformForGhost}
      sceneTransform={viewerSceneTransform}
      customField={activeCustomField}
      showRobot={viewerShowRobot && hasAnyViewsEnabled}
      bind:showOriginAxes={showOriginAxes}
      bind:showTagLines={showTagLines}
      bind:showFieldImage={showFieldImage}
      bind:showMinimapTrail={showMinimapTrail}
	      showCameras={viewerShowCameras}
	      cameraGhostActive={viewerCameraGhostActive}
	      cameraHighlightColor={viewerAccentColor}
	      minimapPoseDot={minimapPoseDot}
	      cameraPovEnabled={viewerCameraPovEnabled}
	      cameraPovTransform={viewerCameraPovTransform}
	      cameraPovIntrinsics={viewerCameraPovIntrinsics}
	      cameraPovApplyFov={viewerCameraPovApplyFov}
	      cameraPovForwardSign={viewerCameraPovForwardSign}
	      feedStatus={feedStatus}
	      selectedSourceCount={selectedSourceIds.length}
	      liveMarkerCount={liveMarkers.length}
	      lastPollMs={lastPollMs}
      bind:pollHz={pollHz}
	      feedMessage={feedMessage}
	      targetSpaceOverlay={targetSpaceOverlay}
	      baseFrame={baseFrame}
		      activeProfileId={$activeProfileId}
			      fieldSpaceAllowed={fieldSpaceAllowed}
		      bind:coordinateSpace={coordinateSpace}
		      cameraPovOptions={cameraPovOptions}
		      bind:cameraPovSelectionId={cameraPovSelectionId}
		      bind:cameraPovFovMode={cameraPovFovMode}
	      availableCoordinateSpaces={availableCoordinateSpaces}
	      poseSpaceLabel={poseSpaceLabel}
	      fieldOriginMode={fieldOriginMode}
	      fieldOriginCustom={profileFieldOriginCustom}
	      onSetFieldOriginMode={setProfileFieldOriginMode}
	      onSetFieldOriginCustomNumeric={setProfileFieldOriginCustomNumeric}
	      bind:showOutputsOverlay={showOutputsOverlay}
	      bind:showMetricsOverlay={showMetricsOverlay}
	      localizationConfigLoading={$localizationConfigLoading}
	      bind:profileNameInput={profileNameInput}
	      onCommitProfileName={commitProfileName}
	      activeSolverId={activeSolverId}
      solvers={$activeProfile?.solvers ?? []}
      onSetActiveSolverId={setActiveSolverIdForUi}
      onAddSolver={addSolver}
      onRemoveActiveSolver={removeActiveSolver}
      bind:solverNameInput={solverNameInput}
      onCommitSolverName={commitSolverName}
      activeSolverMode={activeSolverConfig?.mode ?? null}
      onSetSolverMode={setActiveSolverMode}
      activeSolverSourceIds={activeSolverConfig?.sourceIds ?? []}
      onSetActiveSolverUseAllSources={setActiveSolverUseAllSources}
      onToggleActiveSolverSource={toggleActiveSolverSource}
      solvePoseSpaces={solvePoseSpaces}
      derivedPoseSpaces={derivedPoseSpaces}
	      selectedFieldMapId={selectedFieldMapId}
      calibrationReady={calibrationReady}
      uncalibratedSourcesCount={uncalibratedSources.length}
      bind:tagSizeInput={tagSizeInput}
      tagSizeError={tagSizeError}
      onCommitTagSize={commitTagSize}
      snapZToGround={$activeProfile?.snapZToGround ?? false}
      snapRollToGround={$activeProfile?.snapRollToGround ?? false}
      snapPitchToGround={$activeProfile?.snapPitchToGround ?? false}
      onSetSnapZToGround={setSnapZToGround}
      onSetSnapRollToGround={setSnapRollToGround}
      onSetSnapPitchToGround={setSnapPitchToGround}
      profileTemporalStabilization={profileTemporalStabilization}
      onSetProfileTemporalEnabled={setProfileTemporalEnabled}
      onSetProfileTemporalNumeric={setProfileTemporalNumeric}
      activeSolverTemporalOverride={activeSolverTemporalOverride}
      activeSolverTemporalEffective={activeSolverTemporalEffective}
      onSetSolverTemporalOverrideEnabled={setSolverTemporalOverrideEnabled}
      onSetSolverTemporalEnabled={setSolverTemporalEnabled}
      onSetSolverTemporalNumeric={setSolverTemporalNumeric}
      activeSolverRuntimeTuning={activeSolverRuntimeTuning}
      onSetSolverRuntimeTuningNumeric={setSolverRuntimeTuningNumeric}
      fieldMaps={fieldMaps}
      fieldMapsLoading={fieldMapsLoading}
      fieldMapsError={fieldMapsError}
      mapUploadBusy={mapUploadBusy}
      mapUploadError={mapUploadError}
      bind:fieldMapSelection={fieldMapSelection}
      onSetFieldMapSelection={setFieldMapSelection}
      onUploadMapFile={handleMapUploadFile}
      compatibleSourcesCount={compatibleSources.length}
      sourcesLoading={sourcesLoading}
      sourcesError={sourcesError}
      groupedSources={groupedSources}
      openSourceGroups={openSourceGroups}
      onToggleSourceGroup={toggleSourceGroup}
      calibratedCameraIds={calibratedCameraIds}
      isSourceCalibrated={isSourceCalibrated}
	      selectedSourceIds={selectedSourceIds}
	      onToggleSource={toggleSource}
	      sourceWeightsById={sourceWeightsById}
	      onSetSourceWeight={setSourceWeight}
      sourceUsedByProfilesById={sourceUsedByProfilesById}
	      pipelineStatusError={pipelineStatusError}
	      pipelineOutputsError={pipelineOutputsError}
      localizationConfigError={$localizationConfigError}
      sourceStatusRows={sourceStatusRows}
      bind:showCameraPoseOverlay={showCameraPoseOverlay}
      bind:showCustomFieldsOverlay={showCustomFieldsOverlay}
      bind:showImuRotationOverlay={showImuRotationOverlay}
      imuRotationData={imuRotationOverlayData}
      imuRotationStatusMessage={imuRotationStatusMessage}
      primaryCameraKey={primaryCameraKey}
      bind:cameraPoseXInput={cameraPoseXInput}
      bind:cameraPoseYInput={cameraPoseYInput}
      bind:cameraPoseZInput={cameraPoseZInput}
      bind:cameraPosePitchDeg={cameraPosePitchDeg}
      bind:cameraPoseYawDeg={cameraPoseYawDeg}
      bind:cameraPoseRollDeg={cameraPoseRollDeg}
      cameraPoseEditorError={cameraPoseEditorError}
      onResetPrimaryCameraPoseInputs={resetPrimaryCameraPoseInputs}
      onApplyPrimaryCameraPose={applyPrimaryCameraPose}
      bind:newCustomFieldName={newCustomFieldName}
      bind:newCustomFieldWidth={newCustomFieldWidth}
      bind:newCustomFieldDepth={newCustomFieldDepth}
      newCustomFieldError={newCustomFieldError}
      onCreateCustomField={createCustomField}
      bind:newOriginName={newOriginName}
      bind:newOriginX={newOriginX}
      bind:newOriginZ={newOriginZ}
      bind:newOriginYaw={newOriginYaw}
      newOriginError={newOriginError}
      onAddOrigin={addOriginToSelectedField}
      selectedCustomField={selectedCustomField}
      activeFieldMapBitsStatus={activeFieldMapBitsStatus}
      onRefreshMaps={loadFieldMapList}
      mapUploadFile={mapUploadFile}
      bind:mapAssignId={mapAssignId}
      onAssignMap={assignMapToSelectedField}
      fieldMapDocErrors={fieldMapDocErrors}
      hasActiveProfile={Boolean($activeProfile)}
      onSetMapUploadFile={(file) => (mapUploadFile = file)}
      onUploadSelectedMapFile={uploadSelectedMapFile}
      />
    </div>
  </section>
</div>

{#if pendingProfileDelete}
  <div class="fixed inset-0 z-40 bg-black/60 backdrop-blur-sm"></div>
  <div class="fixed left-1/2 top-1/2 z-50 w-full max-w-lg -translate-x-1/2 -translate-y-1/2 rounded border border-surface-700 bg-surface-950/95 p-6 shadow-2xl max-h-[85vh] max-h-[85svh] max-h-[85dvh] overflow-y-auto">
    <div class="flex items-center justify-between gap-3">
      <div>
        <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Delete profile</p>
        <h2 class="text-lg font-semibold text-white">{pendingProfileDelete.name}</h2>
      </div>
      <button class="btn btn-3xs preset-outline uppercase tracking-[0.3em]" type="button" onclick={closeDeleteProfileModal} disabled={profileDeleteBusy}>
        Close
      </button>
    </div>
    <div class="mt-3 space-y-3 text-sm text-surface-300">
      <p>This action permanently deletes the profile, including its source selection and solver configuration.</p>
      <p class="text-xs uppercase tracking-[0.25em] text-surface-500">
        Profile #{pendingProfileDelete.id.slice(0, 8)}
      </p>
      {#if profileDeleteError}
        <p class="text-xs text-error-300">{profileDeleteError}</p>
      {/if}
    </div>
    <div class="mt-6 flex items-center justify-end gap-3">
      <button
        class="btn btn-2xs uppercase tracking-[0.3em] bg-surface-800/80 text-white hover:bg-surface-700/80"
        type="button"
        onclick={closeDeleteProfileModal}
        disabled={profileDeleteBusy}
      >
        Cancel
      </button>
      <button
        class="btn btn-2xs uppercase tracking-[0.3em] bg-error-600 text-white hover:bg-error-500"
        type="button"
        onclick={() => void confirmDeleteProfile()}
        disabled={profileDeleteBusy}
      >
        {profileDeleteBusy ? 'Deleting…' : 'Delete'}
      </button>
    </div>
  </div>
{/if}
