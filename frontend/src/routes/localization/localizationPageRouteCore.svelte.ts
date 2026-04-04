import { browser } from '$app/environment';
import { fromStore } from 'svelte/store';
import { toaster } from '$lib/toaster';
import { apiFetchResponse } from '$lib/api/core/http';
import { apiUrl } from '$lib/api/client';
import { loadOwnedStreams } from '$lib/api/streamResources';
import { LocalizationService, type LocalizationCapabilitiesResponse, type StreamInfo, type StreamMetrics } from '$lib/api/client';
import type { LocalizationMarker, LocalizationViewMode } from '$lib/features/localization/viewers/localizationViewerTypes';
import type { RigCameraInfo } from '$lib/types/rig';
import { DEFAULT_ROBOT_DIMENSIONS } from '$lib/3d/rigDefaults';
import { rigLayoutStore } from '$lib/stores/rigLayout';
import { formatMeters, parseLengthToMeters } from '$lib/utils/units';
import { composeTransforms, eulerDegreesToQuaternionXYZ, invertTransform, normalizeQuaternion, quaternionToEulerDegreesXYZ, yawDegreesToQuaternion, type PoseQuaternion, type Vec3 } from '$lib/features/localization/poseMath';
import { fetchPipelineOutputSample, isLocalizationCompatibleSource, isLocalizationDetectionSource, isLocalizationImuSource, type LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
import { DEVICE_IMU_EXTERNAL_STREAM_ID } from '$lib/features/localization/externalSourceIds';
import { fetchLocalizationProfilesExport, importLocalizationProfiles as importLocalizationProfilesApi, type LocalizationConfig, type LocalizationCustomFieldOrigin, type LocalizationFieldOriginConfig, type LocalizationFieldOriginMode, type LocalizationPoseSpace, type LocalizationProfile, type LocalizationProfilesExportEnvelope, type LocalizationSolveResponse, type LocalizationSolverConfig, type LocalizationSolverMode, type LocalizationSourceConfig, type LocalizationSourceSampleStatus } from '$lib/features/localization/localizationConfig';
import { createLocalizationProfileStore } from '$lib/features/localization/profileStore';
import { fetchFieldMap, listFieldMaps, uploadLimelightFmap, type FieldMapDocument, type FieldMapSummary } from '$lib/features/localization/fieldMaps';
import { createLocalizationStorageStore } from '$lib/features/localization/storage';
import { createFeedPoller, type PollRateLimits } from '$lib/features/localization/feedPoller';
import { cameraKeyForSource, DERIVED_POSE_SPACES, isSourceCalibrated, poseSpaceLabel, PROFILE_COLORS, SOLVE_POSE_SPACES, SOURCE_COLORS } from '$lib/features/localization/utils';
import { markersFromDetections, toNumber } from '$lib/features/localization/markerUtils';
import { applyDeviceSeparation } from '$lib/features/localization/page/localizationPageUtils';
import { removeStreamMetrics } from '$lib/features/localization/page/localizationMetricsUtils';
import { createLocalizationPoseHelpers } from './localizationPoseHelpers';
import { createLocalizationProfileActions } from './localizationProfileActions';
import { createLocalizationTuningActions, normalizeFieldOriginConfig, normalizeSolverRuntimeTuning, normalizeTemporalSettings } from './localizationTuningActions';
import { createLocalizationSourceSelection } from './localizationSourceSelection';
import { createLocalizationFieldMapActions } from './localizationFieldMapActions';
import { createLocalizationPageState } from '$lib/features/localization/page/LocalizationPageState';
import { createLocalizationPageActions } from '$lib/features/localization/page/localizationPageActions';
import { createLocalizationFeedRuntime } from './localizationFeedRuntime';
import type { CameraPovFovMode, CameraPovIntrinsics, CameraPovState } from './localizationCameraPovUtils';
import type { FieldSpacePoseOverlayEntry, LocalTagPoseOverlayEntry } from './localizationViewerOverlayState';
import { asRecord, BUMPER_ID, buildSourceConfigFromSource, DEFAULT_LOCALIZATION_CALIBRATION, type FeedStatus, type ImuRotationSample, type LocalizationCoordinateSpace, type RigLayoutViewState, UUID_LIKE_RE } from './localizationPageRouteSupport';

export function createLocalizationPageRouteCore() {
  const localizationProfiles = createLocalizationProfileStore();
  const localizationConfig = fromStore(localizationProfiles.config);
  const localizationConfigLoading = fromStore(localizationProfiles.loading);
  const activeProfileId = fromStore(localizationProfiles.activeProfileId);
  const profiles = fromStore(localizationProfiles.profiles);
  const activeProfile = fromStore(localizationProfiles.activeProfile);
  const localizationStorage = createLocalizationStorageStore();
  const cameraExtrinsics = fromStore(localizationStorage.cameraExtrinsics);
  const customFields = fromStore(localizationStorage.customFields);

  const state = $state({
    bumperId: BUMPER_ID,
    separateCameras: true,
    showRobotContext: true,
    ROBOT_FOLLOW_POV_OPTION_ID: '__robot_follow__',
    sources: [] as LocalizationPipelineSource[],
    sourcesLoading: false,
    sourcesError: null as string | null,
    selectedSourceIds: [] as string[],
    primarySourceId: null as string | null,
    solveResponse: null as LocalizationSolveResponse | null,
    solveResponsesByProfile: {} as Record<string, LocalizationSolveResponse>,
    localizationCapabilities: null as LocalizationCapabilitiesResponse | null,
    feedStatus: 'idle' as FeedStatus,
    feedMessage: null as string | null,
    pollHz: 30,
    lastPollMs: null as number | null,
    rawMarkers: [] as LocalizationMarker[],
    liveMarkers: [] as LocalizationMarker[],
    pollResults: [] as LocalizationSourceSampleStatus[],
    viewMode: 'isolated' as LocalizationViewMode,
    fieldOriginMode: 'blue' as LocalizationFieldOriginMode,
    coordinateSpace: 'tag_in_robot' as LocalizationCoordinateSpace,
    showOriginAxes: true,
    showTagLines: false,
    showFieldImage: true,
    showMinimapTrail: true,
    showCameraPoseOverlay: false,
    showCustomFieldsOverlay: false,
    showImuRotationOverlay: false,
    showOutputsOverlay: false,
    showMetricsOverlay: false,
    profileSearch: '',
    pendingProfileDelete: null as { id: string; name: string } | null,
    profileDeleteBusy: false,
    profileDeleteError: null as string | null,
    profileTransferBusy: false,
    profileImportInputEl: null as HTMLInputElement | null,
    cameraPoseEditorError: null as string | null,
    cameraPoseXInput: '',
    cameraPoseYInput: '',
    cameraPoseZInput: '',
    cameraPoseRollDeg: '0',
    cameraPosePitchDeg: '0',
    cameraPoseYawDeg: '0',
    cameraPoseInputsKey: null as string | null,
    selectedCustomFieldId: null as string | null,
    selectedCustomFieldOriginId: null as string | null,
    newCustomFieldName: 'Custom field',
    newCustomFieldWidth: '',
    newCustomFieldDepth: '',
    newCustomFieldError: null as string | null,
    newOriginName: 'Origin',
    newOriginX: '0m',
    newOriginZ: '0m',
    newOriginYaw: '0',
    newOriginError: null as string | null,
    fieldMaps: [] as FieldMapSummary[],
    fieldMapsLoading: false,
    fieldMapsError: null as string | null,
    fieldMapDocs: {} as Record<string, FieldMapDocument>,
    fieldMapDocErrors: {} as Record<string, string>,
    mapUploadFile: null as File | null,
    mapUploadError: null as string | null,
    mapUploadBusy: false,
    mapAssignId: '',
    profileNameInput: '',
    profileNameTargetId: null as string | null,
    solverNameInput: '',
    solverNameTargetKey: null as string | null,
    tagSizeInput: '',
    tagSizeTargetId: null as string | null,
    tagSizeError: null as string | null,
    excludedTagIdsInput: '',
    excludedTagIdsTargetId: null as string | null,
    excludedTagIdsError: null as string | null,
    fieldMapSelection: '',
    openSourceGroups: [] as string[],
    streamInfos: [] as StreamInfo[],
    localizationBootLoading: true,
    localizationBootError: null as string | null,
    cancelLocalizationBootstrap: null as (() => void) | null,
    cancelLocalizationViewersWarmup: null as (() => void) | null,
    localizationDisposed: false,
    activeSolverId: '',
    cameraPovSelectionId: '',
    cameraPovFovMode: 'undistorted' as CameraPovFovMode,
    lastCameraPovByOptionId: {} as Record<string, CameraPovState>,
    lastFieldSpacePoseByProfileSpace: {} as Record<string, FieldSpacePoseOverlayEntry>,
    lastLocalTagPoseByKey: {} as Record<string, LocalTagPoseOverlayEntry>,
    lastViewerCameraTransforms: null as Record<string, { position: Vec3; quaternion?: PoseQuaternion }> | null,
    lastViewerRenderableCameras: [] as RigCameraInfo[],
    imuRotationSample: null as ImuRotationSample | null,
    imuRotationError: null as string | null,
    streamMetricsById: {} as Record<string, StreamMetrics>,
    streamMetricsUpdatedAtById: {} as Record<string, number>,
    streamMetricsErrorById: {} as Record<string, string>,
    pollVisibilityPaused: false,
    visibilityHandler: null as (() => void) | null,
    stopLiveUpdates: null as (() => void) | null,
    liveUpdatesRefreshHandle: null as number | null,
    liveUpdatesRefreshSourcesPending: false,
    lastLiveSourcesRefreshAtMs: 0,
    LocalizationViewersComponent: null as
      | (typeof import('$lib/components/LocalizationViewers.svelte'))['default']
      | null,
    rigLayoutState: {
      layout: { robot: { ...DEFAULT_ROBOT_DIMENSIONS }, cameras: [] },
      loading: false,
      error: null,
      initialized: false
    } as RigLayoutViewState
  });

  const streamMetricsCleanup = new Map<string, () => void>();

  const rigLayoutUnsubscribe = rigLayoutStore.subscribe((next) => {
    state.rigLayoutState = next;
  });

  const {
    setProfileColor,
    setProfileEnabled,
    setProfileViewEnabled,
    persistProfileUpdate,
    setActiveProfile,
    addProfile,
    removeActiveProfile,
    commitProfileName,
    commitTagSize,
    commitExcludedTagIds
  } = createLocalizationProfileActions({
    profiles: () => profiles.current,
    activeProfile: () => activeProfile.current ?? null,
    profileNameInput: () => state.profileNameInput,
    tagSizeInput: () => state.tagSizeInput,
    setTagSizeError: (message) => {
      state.tagSizeError = message;
    },
    excludedTagIdsInput: () => state.excludedTagIdsInput,
    setExcludedTagIdsError: (message) => {
      state.excludedTagIdsError = message;
    },
    parseLengthToMeters,
    localizationProfiles
  });

  const removeProfileById = async (profileId: string): Promise<void> => {
    if (profiles.current.length <= 1) return;
    const currentActiveId = activeProfile.current?.id ?? activeProfileId.current ?? null;
    if (currentActiveId !== profileId) {
      await setActiveProfile(profileId);
    }
    await removeActiveProfile();
  };

  const openDeleteProfileModal = (profile: LocalizationProfile): void => {
    if (profiles.current.length <= 1) return;
    state.pendingProfileDelete = {
      id: profile.id,
      name: (profile.name ?? '').trim() || profile.id
    };
    state.profileDeleteError = null;
  };

  const closeDeleteProfileModal = (): void => {
    if (state.profileDeleteBusy) return;
    state.pendingProfileDelete = null;
    state.profileDeleteError = null;
  };

  const confirmDeleteProfile = async (): Promise<void> => {
    const target = state.pendingProfileDelete;
    if (!target || state.profileDeleteBusy) return;
    state.profileDeleteBusy = true;
    state.profileDeleteError = null;
    try {
      await removeProfileById(target.id);
      state.pendingProfileDelete = null;
    } catch (error) {
      const description = error instanceof Error ? error.message : 'Profile deletion failed';
      state.profileDeleteError = description;
      toaster.error({ title: 'Unable to remove profile', description });
    } finally {
      state.profileDeleteBusy = false;
    }
  };

  const exportLocalizationProfiles = async (): Promise<void> => {
    state.profileTransferBusy = true;
    try {
      const payload = await fetchLocalizationProfilesExport();
      const blob = new Blob([JSON.stringify(payload, null, 2)], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const stamp = new Date().toISOString().replace(/[:.]/g, '-');
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = `localization-profiles-${stamp}.json`;
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
      URL.revokeObjectURL(url);
    } catch (error) {
      const description = error instanceof Error ? error.message : 'Profile export failed';
      toaster.error({ title: 'Unable to export profiles', description });
    } finally {
      state.profileTransferBusy = false;
    }
  };

  const openImportProfilesDialog = (): void => {
    state.profileImportInputEl?.click();
  };

  const loadStreamsSnapshot = async (): Promise<void> => {
    try {
      state.streamInfos = await loadOwnedStreams();
    } catch {
      state.streamInfos = [];
    }
  };

  const loadLocalizationCapabilities = async (): Promise<void> => {
    try {
      state.localizationCapabilities = await LocalizationService.localizationCapabilitiesHandler();
    } catch {
      state.localizationCapabilities = null;
    }
  };

  const saveDefaultCalibrationForStreamIfMissing = async (streamId: string): Promise<void> => {
    const stream = state.streamInfos.find((entry) => String(entry.id ?? '').trim() === streamId) ?? null;
    if (!stream) return;
    const manifest = asRecord(stream.manifest) ?? {};
    const calibration = asRecord(manifest.calibration);
    if (calibration) return;
    const response = await apiFetchResponse(apiUrl(`/streams/${encodeURIComponent(streamId)}/calibration/save`), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
      body: JSON.stringify(DEFAULT_LOCALIZATION_CALIBRATION)
    });
    if (!response.ok) {
      const body = await response.text().catch(() => '');
      throw new Error(body || `Failed to save default calibration (${response.status})`);
    }
  };

  const maybeSeedDefaultLocalizationProfile = async (): Promise<boolean> => {
    const config = localizationConfig.current;
    if (!config || config.profiles.length !== 1) return false;

    const profile = config.profiles[0] ?? null;
    if (!profile) return false;
    const profileId = String(profile.id ?? '').trim().toLowerCase();
    const profileName = String(profile.name ?? '').trim().toLowerCase();
    const isDefaultProfile = profileId === 'default' || profileName === 'default';
    if (!isDefaultProfile) return false;

    const hasSources = (profile.sources ?? []).length > 0;
    const hasFieldMap = Boolean(String(profile.fieldMapId ?? '').trim());
    const hasSnapSettings =
      Boolean(profile.snapZToGround) ||
      Boolean(profile.snapRollToGround) ||
      Boolean(profile.snapPitchToGround);
    if (hasSources || hasFieldMap || hasSnapSettings) {
      return false;
    }

    const streamDetectionSource =
      state.sources.find(
        (source) =>
          isLocalizationDetectionSource(source) &&
          !source.streamId.startsWith('profile:') &&
          !source.streamId.startsWith('peer:') &&
          !source.streamId.startsWith('external:')
      ) ?? null;
    const fallbackDetectionSource = state.sources.find((source) => isLocalizationDetectionSource(source)) ?? null;
    const detectionSource = streamDetectionSource ?? fallbackDetectionSource;
    const imuSource =
      state.sources.find((source) => source.streamId.startsWith(DEVICE_IMU_EXTERNAL_STREAM_ID) && isLocalizationImuSource(source)) ??
      state.sources.find((source) => isLocalizationImuSource(source)) ??
      null;
    const fieldMapId = state.fieldMaps[0]?.id ?? null;

    if (!detectionSource && !imuSource && !fieldMapId) {
      return false;
    }

    const existingById = new Map((profile.sources ?? []).map((entry) => [entry.id, entry]));
    const seededSources: LocalizationSourceConfig[] = [];
    const pushSeedSource = (source: LocalizationPipelineSource | null) => {
      if (!source) return;
      const current = existingById.get(source.id) ?? null;
      seededSources.push(buildSourceConfigFromSource(source, current));
      existingById.delete(source.id);
    };
    pushSeedSource(detectionSource);
    pushSeedSource(imuSource);

    for (const source of existingById.values()) {
      seededSources.push({ ...source, enabled: true });
    }

    const nextProfile: LocalizationProfile = {
      ...profile,
      fieldMapId,
      snapZToGround: true,
      snapRollToGround: true,
      snapPitchToGround: true,
      sources: seededSources
    };
    const nextConfig: LocalizationConfig = {
      ...config,
      activeProfileId: profile.id,
      profiles: [nextProfile]
    };

    await localizationProfiles.persist(nextConfig);

    const streamsToSeed = Array.from(
      new Set(
        seededSources
          .map((source) => String(source.streamId ?? '').trim())
          .filter((streamId) => UUID_LIKE_RE.test(streamId))
      )
    );
    for (const streamId of streamsToSeed) {
      try {
        await saveDefaultCalibrationForStreamIfMissing(streamId);
      } catch (error) {
        const description = error instanceof Error ? error.message : `Failed to apply default calibration for stream ${streamId}`;
        toaster.error({ title: 'Default calibration not applied', description });
      }
    }

    toaster.success({
      title: 'Localization seeded',
      description: 'Default profile now uses field map + ArUco + IMU with ground snapping enabled.'
    });
    return true;
  };

  const importLocalizationProfiles = async (file: File): Promise<void> => {
    state.profileTransferBusy = true;
    try {
      const text = await file.text();
      let parsed: LocalizationProfilesExportEnvelope | LocalizationConfig;
      try {
        parsed = JSON.parse(text) as LocalizationProfilesExportEnvelope | LocalizationConfig;
      } catch {
        throw new Error('Selected file is not valid JSON.');
      }
      const imported = await importLocalizationProfilesApi(parsed);
      await localizationProfiles.load();
      await loadSources();
      await loadStreamsSnapshot();
      await loadFieldMapList();
      toaster.success({
        title: 'Profiles imported',
        description: `Loaded ${imported.profiles.length} localization profile(s).`
      });
    } catch (error) {
      const description = error instanceof Error ? error.message : 'Profile import failed';
      toaster.error({ title: 'Unable to import profiles', description });
    } finally {
      state.profileTransferBusy = false;
      if (state.profileImportInputEl) {
        state.profileImportInputEl.value = '';
      }
    }
  };

  const {
    setProfileFieldOriginMode,
    setProfileFieldOriginCustomNumeric,
    setSnapZToGround,
    setSnapRollToGround,
    setSnapPitchToGround,
    setProfileTemporalEnabled,
    setProfileTemporalNumeric,
    setSolverTemporalOverrideEnabled,
    setSolverTemporalEnabled,
    setSolverTemporalNumeric,
    setSolverRuntimeTuningNumeric
  } = createLocalizationTuningActions({
    getActiveProfile: () => activeProfile.current ?? null,
    getActiveSolver: () => core.activeSolverConfig ?? null,
    persistProfileUpdate
  });

  const {
    createCustomField,
    loadFieldMapList,
    ensureFieldMapLoaded,
    assignMapToSelectedField,
    handleMapUploadFile,
    uploadSelectedMapFile,
    addOriginToSelectedField
  } = createLocalizationFieldMapActions({
    getCustomFields: () => customFields.current,
    setCustomFields: (next) => localizationStorage.setCustomFields(next),
    getSelectedCustomFieldId: () => state.selectedCustomFieldId,
    setSelectedCustomFieldId: (next) => {
      state.selectedCustomFieldId = next;
    },
    setSelectedCustomFieldOriginId: (next) => {
      state.selectedCustomFieldOriginId = next;
    },
    getSelectedCustomField: () => core.selectedCustomField ?? null,
    setNewCustomFieldError: (message) => {
      state.newCustomFieldError = message;
    },
    getNewCustomFieldName: () => state.newCustomFieldName,
    getNewCustomFieldWidth: () => state.newCustomFieldWidth,
    getNewCustomFieldDepth: () => state.newCustomFieldDepth,
    parseLengthToMeters,
    setViewMode: (mode) => {
      state.viewMode = mode;
    },
    getActiveProfile: () => activeProfile.current ?? null,
    persistProfileUpdate,
    updateProfileFieldMapId: (profile, mapId) => ({ ...profile, fieldMapId: mapId }),
    fetchFieldMap,
    listFieldMaps,
    uploadLimelightFmap,
    getFieldMaps: () => state.fieldMaps,
    setFieldMaps: (next) => {
      state.fieldMaps = next;
    },
    getFieldMapDocs: () => state.fieldMapDocs,
    setFieldMapDocs: (next) => {
      state.fieldMapDocs = next;
    },
    getFieldMapDocErrors: () => state.fieldMapDocErrors,
    setFieldMapDocErrors: (next) => {
      state.fieldMapDocErrors = next;
    },
    setFieldMapsLoading: (loading) => {
      state.fieldMapsLoading = loading;
    },
    setFieldMapsError: (message) => {
      state.fieldMapsError = message;
    },
    getMapUploadFile: () => state.mapUploadFile,
    setMapUploadFile: (file) => {
      state.mapUploadFile = file;
    },
    setMapUploadBusy: (busy) => {
      state.mapUploadBusy = busy;
    },
    setMapUploadError: (message) => {
      state.mapUploadError = message;
    },
    setMapUploadSuccess: (summary) => {
      state.fieldMaps = [...state.fieldMaps, summary];
    },
    getMaxMapUploadBytes: () => core.maxMapUploadBytes,
    toaster,
    getNewOriginName: () => state.newOriginName,
    getNewOriginX: () => state.newOriginX,
    getNewOriginZ: () => state.newOriginZ,
    getNewOriginYaw: () => state.newOriginYaw,
    setNewOriginError: (message) => {
      state.newOriginError = message;
    },
    toNumber
  });

  let applySourceSelectionImpl: ((nextIds: string[]) => void) | null = null;
  const applySourceSelection = (nextIds: string[]): void => {
    applySourceSelectionImpl?.(nextIds);
  };

  const {
    loadLocalizationConfig,
    loadSources,
    loadLocalizationViewers
  } = createLocalizationPageState({
    localizationProfiles,
    setFeedStatus: (next) => {
      state.feedStatus = next;
    },
    setFeedMessage: (next) => {
      state.feedMessage = next;
    },
    setPipelineStatus: () => {},
    setPipelineStatusLoading: () => {},
    setPipelineStatusError: () => {},
    setPipelineOutputs: () => {},
    setPipelineOutputsLoading: () => {},
    setPipelineOutputsError: () => {},
    setSources: (next) => {
      state.sources = next;
    },
    setSourcesLoading: (next) => {
      state.sourcesLoading = next;
    },
    setSourcesError: (next) => {
      state.sourcesError = next;
    },
    setSourceCompatibility: () => {},
    getSelectedSourceIds: () => state.selectedSourceIds,
    applySourceSelection,
    toaster,
    isBrowser: browser,
    getLocalizationViewersComponent: () => state.LocalizationViewersComponent,
    setLocalizationViewersComponent: (next) => {
      state.LocalizationViewersComponent =
        next as (typeof import('$lib/components/LocalizationViewers.svelte'))['default'];
    }
  });

  const {
    cameraExtrinsicsTransform,
    rigCameraForSource,
    applyPrimaryCameraPose,
    resetPrimaryCameraPoseInputs
  } = createLocalizationPoseHelpers({
    getCameraExtrinsics: () => cameraExtrinsics.current,
    setCameraExtrinsics: (next) => localizationStorage.setCameraExtrinsics(next),
    getPrimaryCameraKey: () => core.primaryCameraKey,
    getRigCameras: () => state.rigLayoutState.layout.cameras,
    getOriginFromFieldCenterForEditor: () => core.originFromFieldCenterForEditor,
    getCameraPoseInputs: () => ({
      x: state.cameraPoseXInput,
      y: state.cameraPoseYInput,
      z: state.cameraPoseZInput,
      roll: state.cameraPoseRollDeg,
      pitch: state.cameraPosePitchDeg,
      yaw: state.cameraPoseYawDeg
    }),
    setCameraPoseInputs: (next) => {
      state.cameraPoseXInput = next.x;
      state.cameraPoseYInput = next.y;
      state.cameraPoseZInput = next.z;
      state.cameraPoseRollDeg = next.roll;
      state.cameraPosePitchDeg = next.pitch;
      state.cameraPoseYawDeg = next.yaw;
    },
    setCameraPoseEditorError: (message) => {
      state.cameraPoseEditorError = message;
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

  const core = {
    state,
    browser,
    toaster,
    localizationProfiles,
    localizationConfig,
    localizationConfigLoading,
    activeProfileId,
    profiles,
    activeProfile,
    localizationStorage,
    cameraExtrinsics,
    customFields,
    streamMetricsCleanup,
    rigLayoutUnsubscribe,
    createFeedPoller,
    createLocalizationFeedRuntime,
    normalizeFieldOriginConfig,
    normalizeSolverRuntimeTuning,
    normalizeTemporalSettings,
    createLocalizationSourceSelection,
    createLocalizationPageActions,
    fetchPipelineOutputSample,
    cameraKeyForSource,
    isLocalizationCompatibleSource,
    isLocalizationImuSource,
    isSourceCalibrated,
    poseSpaceLabel,
    SOLVE_POSE_SPACES,
    DERIVED_POSE_SPACES,
    SOURCE_COLORS,
    PROFILE_COLORS,
    removeStreamMetrics,
    applyDeviceSeparation,
    markersFromDetections,
    toNumber,
    rigLayoutStore,
    parseLengthToMeters,
    formatMeters,
    loadStreamsSnapshot,
    loadLocalizationCapabilities,
    loadLocalizationConfig,
    loadSources,
    loadLocalizationViewers,
    maybeSeedDefaultLocalizationProfile,
    setProfileColor,
    setProfileEnabled,
    setProfileViewEnabled,
    persistProfileUpdate,
    setActiveProfile,
    addProfile,
    removeActiveProfile,
    commitProfileName,
    commitTagSize,
    commitExcludedTagIds,
    openDeleteProfileModal,
    closeDeleteProfileModal,
    confirmDeleteProfile,
    exportLocalizationProfiles,
    openImportProfilesDialog,
    importLocalizationProfiles,
    setProfileFieldOriginMode,
    setProfileFieldOriginCustomNumeric,
    setSnapZToGround,
    setSnapRollToGround,
    setSnapPitchToGround,
    setProfileTemporalEnabled,
    setProfileTemporalNumeric,
    setSolverTemporalOverrideEnabled,
    setSolverTemporalEnabled,
    setSolverTemporalNumeric,
    setSolverRuntimeTuningNumeric,
    createCustomField,
    loadFieldMapList,
    ensureFieldMapLoaded,
    assignMapToSelectedField,
    handleMapUploadFile,
    uploadSelectedMapFile,
    addOriginToSelectedField,
    cameraExtrinsicsTransform,
    rigCameraForSource,
    applyPrimaryCameraPose,
    resetPrimaryCameraPoseInputs,
    applySourceSelection,
    setApplySourceSelectionImpl: (next: ((nextIds: string[]) => void) | null) => {
      applySourceSelectionImpl = next;
    },
    get activeSolverConfig(): LocalizationSolverConfig | null {
      return null;
    },
    get primaryCameraKey(): string | null {
      return null;
    },
    get originFromFieldCenterForEditor() {
      return null;
    },
    get selectedCustomField() {
      return null;
    },
    get maxMapUploadBytes(): number | null {
      return null;
    }
  };

  const attachMethod = <K extends string, V>(key: K, value: V): void => {
    Object.defineProperty(state, key, {
      configurable: true,
      enumerable: true,
      writable: true,
      value
    });
  };

  attachMethod('openDeleteProfileModal', openDeleteProfileModal);
  attachMethod('closeDeleteProfileModal', closeDeleteProfileModal);
  attachMethod('confirmDeleteProfile', confirmDeleteProfile);
  attachMethod('exportLocalizationProfiles', exportLocalizationProfiles);
  attachMethod('openImportProfilesDialog', openImportProfilesDialog);
  attachMethod('commitProfileName', commitProfileName);
  attachMethod('commitTagSize', commitTagSize);
  attachMethod('commitExcludedTagIds', commitExcludedTagIds);
  attachMethod('setProfileEnabled', setProfileEnabled);
  attachMethod('setProfileViewEnabled', setProfileViewEnabled);
  attachMethod('setProfileColor', setProfileColor);
  attachMethod('setActiveProfile', setActiveProfile);
  attachMethod('addProfile', addProfile);
  attachMethod('setProfileFieldOriginMode', setProfileFieldOriginMode);
  attachMethod('setProfileFieldOriginCustomNumeric', setProfileFieldOriginCustomNumeric);
  attachMethod('setSnapZToGround', setSnapZToGround);
  attachMethod('setSnapRollToGround', setSnapRollToGround);
  attachMethod('setSnapPitchToGround', setSnapPitchToGround);
  attachMethod('setProfileTemporalEnabled', setProfileTemporalEnabled);
  attachMethod('setProfileTemporalNumeric', setProfileTemporalNumeric);
  attachMethod('setSolverTemporalOverrideEnabled', setSolverTemporalOverrideEnabled);
  attachMethod('setSolverTemporalEnabled', setSolverTemporalEnabled);
  attachMethod('setSolverTemporalNumeric', setSolverTemporalNumeric);
  attachMethod('setSolverRuntimeTuningNumeric', setSolverRuntimeTuningNumeric);
  attachMethod('createCustomField', createCustomField);
  attachMethod('loadFieldMapList', loadFieldMapList);
  attachMethod('ensureFieldMapLoaded', ensureFieldMapLoaded);
  attachMethod('assignMapToSelectedField', assignMapToSelectedField);
  attachMethod('handleMapUploadFile', handleMapUploadFile);
  attachMethod('uploadSelectedMapFile', uploadSelectedMapFile);
  attachMethod('addOriginToSelectedField', addOriginToSelectedField);
  attachMethod('applyPrimaryCameraPose', applyPrimaryCameraPose);
  attachMethod('resetPrimaryCameraPoseInputs', resetPrimaryCameraPoseInputs);

  return core;
}

export type LocalizationPageRouteCore = ReturnType<typeof createLocalizationPageRouteCore>;
