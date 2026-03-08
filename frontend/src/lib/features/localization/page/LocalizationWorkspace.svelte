<script lang="ts">
  import type { LocalizationMarker, LocalizationViewMode, LocalizationFieldDefinition } from '$lib/features/localization/viewers/localizationViewerTypes';
  import type {
    LocalizationCustomFieldOrigin,
    LocalizationFieldOriginMode,
    LocalizationPoseSpace,
    LocalizationSolverConfig,
    LocalizationSolverRuntimeTuningConfig,
    LocalizationSolverMode,
    LocalizationTemporalStabilizationConfig
  } from '$lib/features/localization/localizationConfig';
  import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
  import type { FieldMapSummary } from '$lib/features/localization/fieldMaps';
  import type { RigCameraInfo, RobotDimensions } from '$lib/types/rig';
  import type { CustomField } from '$lib/features/localization/types';
  import type { PoseQuaternion, Vec3 } from '$lib/features/localization/poseMath';
  import type { SensorOrientation } from '$lib/types/devices';
  import type LocalizationViewers from '$lib/components/LocalizationViewers.svelte';
  import ImuOrientationViewer from '$lib/components/ImuOrientationViewer.svelte';
  import LocalizationConfigPanel from '$lib/features/localization/page/LocalizationConfigPanel.svelte';
  import SolverPanel from '$lib/features/localization/page/SolverPanel.svelte';
  import CameraPoseOverlay from '$lib/features/localization/page/CameraPoseOverlay.svelte';
  import FieldMapManager from '$lib/features/localization/page/FieldMapManager.svelte';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';

  type ViewerTransform = { position: Vec3; quaternion?: PoseQuaternion } | null;
  type ViewProfileOverlay = {
    id: string;
    label?: string;
    color?: string;
    transform: ViewerTransform;
  };
  type CameraPovOption = {
    id: string;
    groupLabel: string;
    subgroupLabel: string;
    label: string;
    available: boolean;
    ghost: boolean;
  };
  type CameraPovFovMode = 'undistorted' | 'raw' | 'none';
  type CameraPovOptionSubgroup = { label: string; options: CameraPovOption[] };
  type CameraPovOptionGroup = { label: string; subgroups: CameraPovOptionSubgroup[] };
  const ROBOT_FOLLOW_POV_OPTION_ID = '__robot_follow__';
  type RuntimeTuningFieldKey = Extract<keyof LocalizationSolverRuntimeTuningConfig, string>;

  type SourceGroup = {
    key: string;
    kind?: 'stream' | 'profile' | 'peer' | 'peripheral';
    label: string;
    path?: string | null;
    pipelines: Array<{
      key: string;
      label: string;
      sources: LocalizationPipelineSource[];
    }>;
  };

  type SourceStatusRow = {
    source: LocalizationPipelineSource;
    pollMs: number;
    detections: number;
    tagSize: number | null;
    graphMs: number | null;
    metricsUpdatedAt: number | null;
    metricsError: string | null;
    error: string | null;
  };
  type ImuRotationOverlayData = {
    sourceId: string;
    sourceLabel: string;
    outputKey: string;
    roll: number;
    pitch: number;
    yaw: number;
    quaternion: PoseQuaternion | null;
    translation: { x: number; y: number; z: number } | null;
    sampleTimestampMs: number | null;
    ageMs: number;
  };

  type LocalizationWorkspaceProps = {
    viewersComponent?: typeof LocalizationViewers | null;
    markers?: LocalizationMarker[];
    tagLineMarkers?: LocalizationMarker[];
    referenceMarkers?: LocalizationMarker[];
    mode?: LocalizationViewMode;
    bumperNumber?: string;
    bumperColor?: string | null;
    robotOverlays?: ViewProfileOverlay[];
    robot?: RobotDimensions;
    cameras?: RigCameraInfo[];
    cameraTransforms?: Record<string, { position: Vec3; quaternion?: PoseQuaternion }> | null;
    robotTransform?: ViewerTransform;
    sceneTransform?: { position: Vec3; quaternion: PoseQuaternion } | null;
    customField?: CustomField | LocalizationFieldDefinition | null;
    showRobot?: boolean;
    showCameras?: boolean;
    cameraGhostActive?: boolean;
    cameraHighlightColor?: string | null;
    minimapPoseDot?: { position: Vec3; color?: string | null } | null;
    cameraPovEnabled?: boolean;
    cameraPovTransform?: { position: Vec3; quaternion?: PoseQuaternion } | null;
    cameraPovIntrinsics?: { fx: number; fy: number; cx: number; cy: number; width: number; height: number } | null;
    cameraPovApplyFov?: boolean;
    cameraPovForwardSign?: 1 | -1;
    feedStatus?: string;
    selectedSourceCount?: number;
    liveMarkerCount?: number;
    lastPollMs?: number | null;
    pollHz?: number;
    pollHzMin?: number;
    pollHzMax?: number;
    pollHzStep?: number;
    feedMessage?: string | null;
    targetSpaceOverlay?: {
      header: string;
      rows: Array<{
        key: string;
        color: string;
        label: string;
        values: string;
        stale?: boolean;
      }>;
    } | null;
    showOriginAxes?: boolean;
    showTagLines?: boolean;
    showFieldImage?: boolean;
    showMinimapTrail?: boolean;
    baseFrame?: 'camera' | 'robot' | 'field';
    activeProfileId?: string | null;
	    fieldSpaceAllowed?: boolean;
	    coordinateSpace?: LocalizationPoseSpace;
	    availableCoordinateSpaces?: LocalizationPoseSpace[];
	    cameraPovOptions?: CameraPovOption[];
	    cameraPovSelectionId?: string;
      cameraPovFovMode?: CameraPovFovMode;
    poseSpaceLabel?: (space: LocalizationPoseSpace) => string;
    fieldOriginMode?: LocalizationFieldOriginMode;
    showOutputsOverlay?: boolean;
    showMetricsOverlay?: boolean;
    localizationConfigLoading?: boolean;
    profileNameInput?: string;
    onCommitProfileName?: () => void;
    activeSolverId?: string;
    solvers?: LocalizationSolverConfig[];
    onSetActiveSolverId?: (solverId: string) => void;
    onAddSolver?: () => void;
    onRemoveActiveSolver?: () => void;
    solverNameInput?: string;
    onCommitSolverName?: () => void;
    activeSolverMode?: LocalizationSolverMode | null;
    supportedSolverModes?: LocalizationSolverMode[];
    onSetSolverMode?: (mode: LocalizationSolverMode) => void;
    activeSolverSourceIds?: string[];
    onSetActiveSolverUseAllSources?: (useAll: boolean) => void;
    onToggleActiveSolverSource?: (sourceId: string, enabled: boolean) => void;
    solvePoseSpaces?: LocalizationPoseSpace[];
    derivedPoseSpaces?: LocalizationPoseSpace[];
    selectedFieldMapId?: string | null;
    calibrationReady?: boolean;
    uncalibratedSourcesCount?: number;
    tagSizeInput?: string;
    tagSizeError?: string | null;
    onCommitTagSize?: () => void;
    excludedTagIdsInput?: string;
    excludedTagIdsError?: string | null;
    onCommitExcludedTagIds?: () => void;
    fieldOriginCustom?: LocalizationCustomFieldOrigin | null;
    onSetFieldOriginMode?: (mode: LocalizationFieldOriginMode) => void;
    onSetFieldOriginCustomNumeric?: (field: 'x' | 'z' | 'yawDeg', value: string) => void;
    snapZToGround?: boolean;
    snapRollToGround?: boolean;
    snapPitchToGround?: boolean;
    onSetSnapZToGround?: (enabled: boolean) => void;
    onSetSnapRollToGround?: (enabled: boolean) => void;
    onSetSnapPitchToGround?: (enabled: boolean) => void;
    profileTemporalStabilization?: LocalizationTemporalStabilizationConfig;
    onSetProfileTemporalEnabled?: (enabled: boolean) => void;
    onSetProfileTemporalNumeric?: (field: string, value: string) => void;
    activeSolverTemporalOverride?: LocalizationTemporalStabilizationConfig | null;
    activeSolverTemporalEffective?: LocalizationTemporalStabilizationConfig;
    onSetSolverTemporalOverrideEnabled?: (enabled: boolean) => void;
    onSetSolverTemporalEnabled?: (enabled: boolean) => void;
    onSetSolverTemporalNumeric?: (field: string, value: string) => void;
    activeSolverRuntimeTuning?: LocalizationSolverRuntimeTuningConfig;
    onSetSolverRuntimeTuningNumeric?: (field: RuntimeTuningFieldKey, value: string) => void;
    fieldMaps?: FieldMapSummary[];
    fieldMapsLoading?: boolean;
    fieldMapsError?: string | null;
    mapUploadBusy?: boolean;
    mapUploadError?: string | null;
    fieldMapSelection?: string;
    onSetFieldMapSelection?: (value: string) => void;
    onUploadMapFile?: (file: File) => void;
    compatibleSourcesCount?: number;
    sourcesLoading?: boolean;
    sourcesError?: string | null;
    groupedSources?: SourceGroup[];
    openSourceGroups?: string[];
    onToggleSourceGroup?: (key: string) => void;
    calibratedCameraIds?: Set<string>;
    isSourceCalibrated?: (source: LocalizationPipelineSource, calibrated: Set<string>) => boolean;
    selectedSourceIds?: string[];
    onToggleSource?: (sourceId: string, enabled: boolean) => void;
    sourceWeightsById?: Record<string, number>;
    onSetSourceWeight?: (sourceId: string, value: string) => void;
    sourceUsedByProfilesById?: Record<string, string[]>;
    sourceStatusRows?: SourceStatusRow[];
    showCameraPoseOverlay?: boolean;
    showCustomFieldsOverlay?: boolean;
    showImuRotationOverlay?: boolean;
    imuRotationData?: ImuRotationOverlayData | null;
    imuRotationStatusMessage?: string | null;
    primaryCameraKey?: string | null;
    cameraPoseXInput?: string;
    cameraPoseYInput?: string;
    cameraPoseZInput?: string;
    cameraPosePitchDeg?: string;
    cameraPoseYawDeg?: string;
    cameraPoseRollDeg?: string;
    cameraPoseEditorError?: string | null;
    onResetPrimaryCameraPoseInputs?: () => void;
    onApplyPrimaryCameraPose?: () => void;
    newCustomFieldName?: string;
    newCustomFieldWidth?: string;
    newCustomFieldDepth?: string;
    newCustomFieldError?: string | null;
    onCreateCustomField?: () => void;
    newOriginName?: string;
    newOriginX?: string;
    newOriginZ?: string;
    newOriginYaw?: string;
    newOriginError?: string | null;
    onAddOrigin?: () => void;
    selectedCustomField?: CustomField | null;
    activeFieldMapBitsStatus?: string | null;
    onRefreshMaps?: () => void;
    mapUploadFile?: File | null;
    mapAssignId?: string;
    onAssignMap?: (mapId: string | null) => void;
    fieldMapDocErrors?: Record<string, string>;
    hasActiveProfile?: boolean;
    onSetMapUploadFile?: (file: File | null) => void;
    onUploadSelectedMapFile?: () => void;
  };

  let {
    viewersComponent = null,
    markers = [],
    tagLineMarkers = [],
    referenceMarkers = [],
    mode = 'isolated',
    bumperNumber = '',
    bumperColor = null,
    robotOverlays = [],
    robot = { width: 0, length: 0, bumperHeight: 0, bumperThickness: 0, groundClearance: 0 },
    cameras = [],
    cameraTransforms = null,
    robotTransform = null,
    sceneTransform = null,
    customField = null,
    showRobot = true,
    showCameras = true,
    cameraGhostActive = false,
    cameraHighlightColor = null,
    minimapPoseDot = null,
    cameraPovEnabled = false,
    cameraPovTransform = null,
    cameraPovIntrinsics = null,
    cameraPovApplyFov = true,
    cameraPovForwardSign = 1,
    feedStatus = 'idle',
    selectedSourceCount = 0,
    liveMarkerCount = 0,
    lastPollMs = null,
    pollHz = $bindable(30),
    pollHzMin = 1,
    pollHzMax = 240,
    pollHzStep = 1,
    feedMessage = null,
    targetSpaceOverlay = null,
    showOriginAxes = $bindable(true),
    showTagLines = $bindable(false),
    showFieldImage = $bindable(true),
    showMinimapTrail = $bindable(true),
    baseFrame = 'camera',
    activeProfileId = $bindable<string | null>(null),
    fieldSpaceAllowed = false,
    coordinateSpace = $bindable('tag_in_camera' as LocalizationPoseSpace),
    cameraPovOptions = [],
    cameraPovSelectionId = $bindable(''),
    cameraPovFovMode = $bindable('undistorted' as CameraPovFovMode),
    availableCoordinateSpaces = [],
    poseSpaceLabel = (space) => space,
    fieldOriginMode = $bindable('blue' as LocalizationFieldOriginMode),
    showOutputsOverlay = $bindable(false),
    showMetricsOverlay = $bindable(false),
    localizationConfigLoading = false,
    profileNameInput = $bindable(''),
    onCommitProfileName,
    activeSolverId = '',
    solvers = [],
    onSetActiveSolverId,
    onAddSolver,
    onRemoveActiveSolver,
    solverNameInput = $bindable(''),
    onCommitSolverName,
    activeSolverMode = null,
    supportedSolverModes = [],
    onSetSolverMode,
    activeSolverSourceIds = [],
    onSetActiveSolverUseAllSources,
    onToggleActiveSolverSource,
    solvePoseSpaces = [],
    derivedPoseSpaces = [],
    selectedFieldMapId = null,
    calibrationReady = false,
    uncalibratedSourcesCount = 0,
    tagSizeInput = $bindable(''),
    tagSizeError = null,
    onCommitTagSize,
    excludedTagIdsInput = $bindable(''),
    excludedTagIdsError = null,
    onCommitExcludedTagIds,
    fieldOriginCustom = { x: 0, z: 0, yawDeg: 0 },
    onSetFieldOriginMode,
    onSetFieldOriginCustomNumeric,
    snapZToGround = false,
    snapRollToGround = false,
    snapPitchToGround = false,
    onSetSnapZToGround,
    onSetSnapRollToGround,
    onSetSnapPitchToGround,
    profileTemporalStabilization = {
      enabled: true,
      singleTagTranslationAlpha: 0.18,
      singleTagRotationAlpha: 0.16,
      multiTagTranslationAlpha: 0.45,
      multiTagRotationAlpha: 0.38,
      maxTranslationJumpM: 1.2,
      maxRotationJumpDeg: 70,
      reanchorRejectWindowMs: 450
    },
    onSetProfileTemporalEnabled,
    onSetProfileTemporalNumeric,
    activeSolverTemporalOverride = null,
    activeSolverTemporalEffective = {
      enabled: true,
      singleTagTranslationAlpha: 0.18,
      singleTagRotationAlpha: 0.16,
      multiTagTranslationAlpha: 0.45,
      multiTagRotationAlpha: 0.38,
      maxTranslationJumpM: 1.2,
      maxRotationJumpDeg: 70,
      reanchorRejectWindowMs: 450
    },
    onSetSolverTemporalOverrideEnabled,
    onSetSolverTemporalEnabled,
    onSetSolverTemporalNumeric,
    activeSolverRuntimeTuning = {
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
    },
    onSetSolverRuntimeTuningNumeric,
    fieldMaps = [],
    fieldMapsLoading = false,
    fieldMapsError = null,
    mapUploadBusy = false,
    mapUploadError = null,
    fieldMapSelection = $bindable(''),
    onSetFieldMapSelection,
    onUploadMapFile,
    compatibleSourcesCount = 0,
    sourcesLoading = false,
    sourcesError = null,
    groupedSources = [],
    openSourceGroups = [],
    onToggleSourceGroup,
    calibratedCameraIds = new SvelteSet<string>(),
    isSourceCalibrated = () => true,
    selectedSourceIds = [],
    onToggleSource,
    sourceWeightsById = {},
    onSetSourceWeight,
    sourceUsedByProfilesById = {},
    sourceStatusRows = [],
    showCameraPoseOverlay = $bindable(false),
    showCustomFieldsOverlay = $bindable(false),
    showImuRotationOverlay = $bindable(false),
    imuRotationData = null,
    imuRotationStatusMessage = null,
    primaryCameraKey = null,
    cameraPoseXInput = $bindable(''),
    cameraPoseYInput = $bindable(''),
    cameraPoseZInput = $bindable(''),
    cameraPosePitchDeg = $bindable('0'),
    cameraPoseYawDeg = $bindable('0'),
    cameraPoseRollDeg = $bindable('0'),
    cameraPoseEditorError = null,
    onResetPrimaryCameraPoseInputs,
    onApplyPrimaryCameraPose,
    newCustomFieldName = $bindable('Custom field'),
    newCustomFieldWidth = $bindable(''),
    newCustomFieldDepth = $bindable(''),
    newCustomFieldError = null,
    onCreateCustomField,
    newOriginName = $bindable('Origin'),
    newOriginX = $bindable('0m'),
    newOriginZ = $bindable('0m'),
    newOriginYaw = $bindable('0'),
    newOriginError = null,
    onAddOrigin,
    selectedCustomField = null,
    activeFieldMapBitsStatus = null,
    onRefreshMaps,
    mapUploadFile = null,
    mapAssignId = $bindable(''),
    onAssignMap,
    fieldMapDocErrors = {},
    hasActiveProfile = false,
    onSetMapUploadFile,
    onUploadSelectedMapFile
  }: LocalizationWorkspaceProps = $props();

  const ViewersComponent = $derived(viewersComponent);
  const groupedCameraPovOptions = $derived.by<CameraPovOptionGroup[]>(() => {
    const groups = new SvelteMap<string, Map<string, CameraPovOption[]>>();
    for (const option of cameraPovOptions) {
      const groupKey = (option.groupLabel ?? '').trim() || 'Profile';
      const subgroupKey = (option.subgroupLabel ?? '').trim() || 'Camera';
      const subgroups = groups.get(groupKey) ?? new SvelteMap<string, CameraPovOption[]>();
      const list = subgroups.get(subgroupKey) ?? [];
      list.push(option);
      subgroups.set(subgroupKey, list);
      groups.set(groupKey, subgroups);
    }
    return Array.from(groups.entries()).map(([label, subgroups]) => ({
      label,
      subgroups: Array.from(subgroups.entries()).map(([subLabel, options]) => ({ label: subLabel, options }))
    }));
  });
  const selectedCameraPovOption = $derived.by<CameraPovOption | null>(() => {
    const selected = cameraPovSelectionId.trim();
    if (!selected || selected === ROBOT_FOLLOW_POV_OPTION_ID) return null;
    return cameraPovOptions.find((option) => option.id === selected) ?? null;
  });
  const selectedCameraPovIsGhost = $derived.by<boolean>(() => Boolean(selectedCameraPovOption?.ghost));
  const robotFollowPovEnabled = $derived.by(
    () => cameraPovSelectionId.trim() === ROBOT_FOLLOW_POV_OPTION_ID
  );
  const imuOrientationForViewer = $derived.by<SensorOrientation>(() => {
    const sample = imuRotationData;
    return {
      roll: sample?.roll ?? 0,
      pitch: sample?.pitch ?? 0,
      yaw: sample?.yaw ?? 0,
      quaternion: sample?.quaternion
        ? {
            w: sample.quaternion.w,
            x: sample.quaternion.x,
            y: sample.quaternion.y,
            z: sample.quaternion.z
          }
        : null
    };
  });
  let posesCollapsed = $state(true);
  const IMU_WINDOW_MIN_WIDTH_PX = 280;
  const IMU_WINDOW_MIN_HEIGHT_PX = 240;
  const IMU_WINDOW_DEFAULT_WIDTH_PX = 368;
  const IMU_WINDOW_DEFAULT_HEIGHT_PX = 416;
  const IMU_WINDOW_MARGIN_PX = 16;
  let workspaceRootEl = $state<HTMLDivElement | null>(null);
  let imuWindowEl = $state<HTMLDivElement | null>(null);
  let imuWindowWidth = $state(IMU_WINDOW_DEFAULT_WIDTH_PX);
  let imuWindowHeight = $state(IMU_WINDOW_DEFAULT_HEIGHT_PX);
  let imuWindowX = $state<number | null>(null);
  let imuWindowY = $state<number | null>(null);
  let imuDragActive = false;
  let imuResizeActive = false;
  let imuDragOffsetX = 0;
  let imuDragOffsetY = 0;
  let imuResizePointerStartX = 0;
  let imuResizePointerStartY = 0;
  let imuResizeStartWidth = IMU_WINDOW_DEFAULT_WIDTH_PX;
  let imuResizeStartHeight = IMU_WINDOW_DEFAULT_HEIGHT_PX;
  let lastImuOverlayVisible = false;

  function clampImuWindowRect(
    nextX: number,
    nextY: number,
    nextWidth: number,
    nextHeight: number
  ): { x: number; y: number; width: number; height: number } {
    const fallbackWidth = typeof window !== 'undefined' ? window.innerWidth : 1280;
    const fallbackHeight = typeof window !== 'undefined' ? window.innerHeight : 720;
    const rootWidth = workspaceRootEl?.clientWidth ?? fallbackWidth;
    const rootHeight = workspaceRootEl?.clientHeight ?? fallbackHeight;

    const maxWidth = Math.max(IMU_WINDOW_MIN_WIDTH_PX, rootWidth - IMU_WINDOW_MARGIN_PX * 2);
    const maxHeight = Math.max(IMU_WINDOW_MIN_HEIGHT_PX, rootHeight - IMU_WINDOW_MARGIN_PX * 2);
    const width = Math.min(Math.max(nextWidth, IMU_WINDOW_MIN_WIDTH_PX), maxWidth);
    const height = Math.min(Math.max(nextHeight, IMU_WINDOW_MIN_HEIGHT_PX), maxHeight);
    const maxX = Math.max(IMU_WINDOW_MARGIN_PX, rootWidth - width - IMU_WINDOW_MARGIN_PX);
    const maxY = Math.max(IMU_WINDOW_MARGIN_PX, rootHeight - height - IMU_WINDOW_MARGIN_PX);
    const x = Math.min(Math.max(nextX, IMU_WINDOW_MARGIN_PX), maxX);
    const y = Math.min(Math.max(nextY, IMU_WINDOW_MARGIN_PX), maxY);
    return { x, y, width, height };
  }

  function applyImuWindowRect(next: { x: number; y: number; width: number; height: number }): void {
    imuWindowX = next.x;
    imuWindowY = next.y;
    imuWindowWidth = next.width;
    imuWindowHeight = next.height;
  }

  function placeImuWindowDefault(): void {
    const fallbackWidth = typeof window !== 'undefined' ? window.innerWidth : 1280;
    const fallbackHeight = typeof window !== 'undefined' ? window.innerHeight : 720;
    const rootWidth = workspaceRootEl?.clientWidth ?? fallbackWidth;
    const rootHeight = workspaceRootEl?.clientHeight ?? fallbackHeight;
    applyImuWindowRect(
      clampImuWindowRect(
        rootWidth - imuWindowWidth - IMU_WINDOW_MARGIN_PX,
        rootHeight - imuWindowHeight - IMU_WINDOW_MARGIN_PX,
        imuWindowWidth,
        imuWindowHeight
      )
    );
  }

  function ensureImuWindowVisible(): void {
    if (imuWindowX == null || imuWindowY == null) {
      placeImuWindowDefault();
      return;
    }
    applyImuWindowRect(clampImuWindowRect(imuWindowX, imuWindowY, imuWindowWidth, imuWindowHeight));
  }

  function stopImuWindowPointerTracking(): void {
    imuDragActive = false;
    imuResizeActive = false;
    if (typeof window === 'undefined') return;
    window.removeEventListener('pointermove', onImuWindowPointerMove);
    window.removeEventListener('pointerup', onImuWindowPointerEnd);
    window.removeEventListener('pointercancel', onImuWindowPointerEnd);
  }

  function startImuWindowPointerTracking(): void {
    if (typeof window === 'undefined') return;
    window.removeEventListener('pointermove', onImuWindowPointerMove);
    window.removeEventListener('pointerup', onImuWindowPointerEnd);
    window.removeEventListener('pointercancel', onImuWindowPointerEnd);
    window.addEventListener('pointermove', onImuWindowPointerMove);
    window.addEventListener('pointerup', onImuWindowPointerEnd);
    window.addEventListener('pointercancel', onImuWindowPointerEnd);
  }

  function onImuWindowPointerMove(event: PointerEvent): void {
    if (!showImuRotationOverlay) return;
    const rootRect = workspaceRootEl?.getBoundingClientRect();
    const rootLeft = rootRect?.left ?? 0;
    const rootTop = rootRect?.top ?? 0;
    if (imuDragActive) {
      applyImuWindowRect(
        clampImuWindowRect(
          event.clientX - rootLeft - imuDragOffsetX,
          event.clientY - rootTop - imuDragOffsetY,
          imuWindowWidth,
          imuWindowHeight
        )
      );
      return;
    }
    if (imuResizeActive) {
      const deltaX = event.clientX - imuResizePointerStartX;
      const deltaY = event.clientY - imuResizePointerStartY;
      applyImuWindowRect(
        clampImuWindowRect(
          imuWindowX ?? IMU_WINDOW_MARGIN_PX,
          imuWindowY ?? IMU_WINDOW_MARGIN_PX,
          imuResizeStartWidth + deltaX,
          imuResizeStartHeight + deltaY
        )
      );
    }
  }

  function onImuWindowPointerEnd(): void {
    stopImuWindowPointerTracking();
  }

  function beginImuWindowDrag(event: PointerEvent): void {
    if (event.button !== 0) return;
    ensureImuWindowVisible();
    const panelRect = imuWindowEl?.getBoundingClientRect();
    if (!panelRect) return;
    imuDragActive = true;
    imuResizeActive = false;
    imuDragOffsetX = event.clientX - panelRect.left;
    imuDragOffsetY = event.clientY - panelRect.top;
    startImuWindowPointerTracking();
    event.preventDefault();
  }

  function beginImuWindowResize(event: PointerEvent): void {
    if (event.button !== 0) return;
    ensureImuWindowVisible();
    imuResizeActive = true;
    imuDragActive = false;
    imuResizePointerStartX = event.clientX;
    imuResizePointerStartY = event.clientY;
    imuResizeStartWidth = imuWindowWidth;
    imuResizeStartHeight = imuWindowHeight;
    startImuWindowPointerTracking();
    event.preventDefault();
  }

  function onImuWindowViewportResize(): void {
    if (!showImuRotationOverlay) return;
    ensureImuWindowVisible();
  }

  function openImuRotationOverlay(): void {
    showImuRotationOverlay = true;
    queueMicrotask(() => {
      ensureImuWindowVisible();
    });
  }

  function imuWindowStyle(): string {
    if (imuWindowX == null || imuWindowY == null) {
      return `right:${IMU_WINDOW_MARGIN_PX}px;bottom:${IMU_WINDOW_MARGIN_PX}px;width:${imuWindowWidth}px;height:${imuWindowHeight}px;`;
    }
    return `left:${imuWindowX}px;top:${imuWindowY}px;width:${imuWindowWidth}px;height:${imuWindowHeight}px;`;
  }

  $effect(() => {
    const visible = showImuRotationOverlay;
    if (visible && !lastImuOverlayVisible) {
      queueMicrotask(() => {
        ensureImuWindowVisible();
      });
    }
    if (!visible && lastImuOverlayVisible) {
      stopImuWindowPointerTracking();
    }
    lastImuOverlayVisible = visible;
  });

  $effect(() => {
    return () => {
      stopImuWindowPointerTracking();
    };
  });

  function compactPoseValues(values: string): string {
    if (values.includes('id:') || values.includes('d:')) {
      return values.trim();
    }
    const tokens = values.match(/—|[-+]?\d+(?:\.\d+)?/g) ?? [];
    if (tokens.length < 6) return values.trim();
    const pos = tokens.slice(0, 3).join(', ');
    const rot = tokens.slice(3, 6).join(', ');
    return `(${pos}) (${rot})`;
  }

  function toggleMetrics() {
    showMetricsOverlay = !showMetricsOverlay;
    showOutputsOverlay = false;
  }

  function formatFixed(value: number, digits = 2): string {
    return Number.isFinite(value) ? value.toFixed(digits) : '—';
  }

  function formatSigned(value: number, digits = 2): string {
    if (!Number.isFinite(value)) return '—';
    return `${value >= 0 ? '+' : ''}${value.toFixed(digits)}`;
  }

  export type $$Props = LocalizationWorkspaceProps;
</script>

<svelte:window onresize={onImuWindowViewportResize} />

<div class="relative flex-1 min-h-0 overflow-hidden" bind:this={workspaceRootEl}>
  {#if ViewersComponent}
    <ViewersComponent
      {markers}
      tagLineMarkers={tagLineMarkers}
      referenceMarkers={referenceMarkers}
      mode={mode}
      bumperNumber={bumperNumber}
      bumperColor={bumperColor}
      robotOverlays={robotOverlays}
      robot={robot}
      cameras={cameras}
      cameraTransforms={cameraTransforms}
      robotTransform={robotTransform}
      sceneTransform={sceneTransform}
      customField={customField}
      showRobot={showRobot}
      showCameras={showCameras}
      cameraGhostActive={cameraGhostActive}
      showOriginAxes={showOriginAxes}
      showTagLines={showTagLines}
      showFieldImage={showFieldImage}
      showMinimapTrail={showMinimapTrail}
      cameraHighlightColor={cameraHighlightColor}
      minimapPoseDot={minimapPoseDot}
      cameraPovEnabled={cameraPovEnabled}
      cameraPovTransform={cameraPovTransform}
      cameraPovIntrinsics={cameraPovIntrinsics}
      cameraPovApplyFov={cameraPovApplyFov}
      cameraPovForwardSign={cameraPovForwardSign}
      robotFollowPovEnabled={robotFollowPovEnabled}
      metricsActive={showMetricsOverlay}
      onMetricsToggle={toggleMetrics}
    >
      {#snippet footerStatus()}
        <div class="grid grid-cols-[12ch_8ch_8ch_10ch_7ch_minmax(0,1fr)] items-center gap-x-2 text-micro-tight tracking-[0.3em] text-surface-400">
          <span class="whitespace-nowrap font-semibold uppercase text-surface-50">{feedStatus}</span>
          <span class="whitespace-nowrap text-right font-mono tabular-nums">{selectedSourceCount} src</span>
          <span class="whitespace-nowrap text-right font-mono tabular-nums">{liveMarkerCount} tags</span>
          <span class="whitespace-nowrap text-right font-mono tabular-nums">{lastPollMs != null ? `${lastPollMs.toFixed(1)}ms` : '—'}</span>
          <span class="whitespace-nowrap text-right font-mono tabular-nums">{pollHz}Hz</span>
          <span class={`min-w-0 truncate ${feedMessage ? 'text-error-300' : 'text-surface-500'}`}>{feedMessage ?? ''}</span>
        </div>
      {/snippet}

      {#snippet minimapControls()}
        <div class="pointer-events-auto w-full space-y-2">
          <div class="w-full rounded border border-surface-800 bg-surface-950/70 p-3 text-xs text-surface-300 shadow-xl backdrop-blur">
            <div class="flex items-center justify-between gap-3">
              <div>
                <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Update rate</p>
                <span class="text-micro font-semibold text-surface-50">{pollHz} Hz</span>
              </div>
            </div>
            <input
              type="range"
              min={pollHzMin}
              max={pollHzMax}
              step={pollHzStep}
              class="range range-xs mt-2 w-full"
              bind:value={pollHz}
            />

            <div class="mt-3 border-t border-surface-800/70 pt-3">
              <p class="text-micro uppercase tracking-[0.35em] text-surface-500">Viewer</p>
              <div class="mt-2 flex flex-col gap-2">
                <label class="flex items-center justify-between gap-3">
                  <span class="text-surface-500">Show origin axes</span>
                  <input type="checkbox" bind:checked={showOriginAxes} />
                </label>
                <label class="flex items-center justify-between gap-3">
                  <span class="text-surface-500">Draw tag lines</span>
                  <input type="checkbox" bind:checked={showTagLines} />
                </label>
                <label class="flex items-center justify-between gap-3">
                  <span class="text-surface-500">Show field image</span>
                  <input type="checkbox" bind:checked={showFieldImage} />
                </label>
                <label class="flex items-center justify-between gap-3">
                  <span class="text-surface-500">Show minimap trail</span>
                  <input type="checkbox" bind:checked={showMinimapTrail} />
                </label>
              </div>
            </div>
          </div>
        </div>
      {/snippet}
    </ViewersComponent>
  {:else}
    <div class="flex h-full w-full items-center justify-center text-sm text-surface-500">Loading viewer...</div>
  {/if}

  <div class="absolute left-4 top-4 z-40 flex max-w-[92vw] flex-col gap-2">
    <div class="grid gap-3 rounded border border-surface-800 bg-surface-950/70 p-3 text-xs text-surface-200 shadow-xl backdrop-blur">
      <div class="grid gap-2">
        <div class="flex items-center justify-between">
          <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Target space</p>
          {#if !fieldSpaceAllowed && selectedFieldMapId}
            <span class="text-micro-tight text-amber-300">uncalibrated</span>
          {/if}
        </div>
        <select
          class="min-w-[12rem] rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs uppercase tracking-[0.3em] text-surface-200 focus:border-primary-400 focus:outline-none"
          bind:value={coordinateSpace}
        >
          {#each availableCoordinateSpaces as space (space)}
            <option value={space}>
              {poseSpaceLabel(space)}
            </option>
          {/each}
        </select>

      </div>

      <div class="flex items-center gap-3 border-t border-surface-800/70 pt-3">
        <p class="shrink-0 text-micro-tight uppercase tracking-[0.35em] text-surface-500">POV</p>
        <select
          class={`min-w-0 flex-1 rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs uppercase tracking-[0.3em] focus:border-primary-400 focus:outline-none ${
            selectedCameraPovIsGhost ? 'text-surface-400' : 'text-surface-200'
          }`}
          bind:value={cameraPovSelectionId}
        >
          <option value="">off</option>
          <option value={ROBOT_FOLLOW_POV_OPTION_ID}>robot</option>
          {#each groupedCameraPovOptions as group (group.label)}
            <optgroup label={group.label}>
              {#each group.subgroups as subgroup (subgroup.label)}
                <option value={`__header:${group.label}:${subgroup.label}`} disabled>{`- ${subgroup.label}`}</option>
                {#each subgroup.options as option (option.id)}
                  <option
                    value={option.id}
                    disabled={!option.available}
                    style={option.ghost ? 'opacity:0.62; text-decoration: line-through;' : undefined}
                  >
                    {option.ghost
                      ? `${option.label} (ghost)`
                      : option.available
                        ? option.label
                        : `${option.label} (no pose)`}
                  </option>
                {/each}
              {/each}
            </optgroup>
          {/each}
        </select>
      </div>

      <div class="flex items-center gap-3 border-t border-surface-800/70 pt-3">
        <p class="shrink-0 text-micro-tight uppercase tracking-[0.35em] text-surface-500">POV FOV</p>
        <select
          class="min-w-0 flex-1 rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs uppercase tracking-[0.3em] text-surface-200 focus:border-primary-400 focus:outline-none"
          bind:value={cameraPovFovMode}
        >
          <option value="undistorted">calibration undistorted</option>
          <option value="raw">raw distorted</option>
          <option value="none">none</option>
        </select>
      </div>

      {#if mode === 'frc-field' && baseFrame === 'field'}
        <div class="grid gap-2">
          <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Origin</p>
          <select
            class="min-w-[12rem] rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs uppercase tracking-[0.3em] text-surface-200 focus:border-primary-400 focus:outline-none"
            value={fieldOriginMode}
            onchange={(event) => onSetFieldOriginMode?.((event.currentTarget as HTMLSelectElement).value as LocalizationFieldOriginMode)}
          >
            <option value="blue">wpiblue</option>
            <option value="red">wpired</option>
            <option value="center">center</option>
            <option value="custom">custom</option>
          </select>
        </div>
      {/if}

    </div>

    {#if targetSpaceOverlay && !showOutputsOverlay}
      <div class="grid gap-2 rounded border border-surface-800 bg-surface-950/70 p-3 text-xs text-surface-200 shadow-xl backdrop-blur">
        <button
          type="button"
          class="flex items-center justify-between gap-3 rounded border border-surface-800/70 bg-surface-950/40 px-2 py-1.5 text-left"
          onclick={() => {
            posesCollapsed = !posesCollapsed;
          }}
        >
          <span class="text-micro-tight uppercase tracking-[0.3em] text-surface-400">
            poses ({targetSpaceOverlay.rows.length})
          </span>
          <span class="text-[0.7rem] text-surface-500">{posesCollapsed ? '+' : '−'}</span>
        </button>
        {#if !posesCollapsed}
          <p class="text-micro-tight text-surface-500">{targetSpaceOverlay.header}</p>
          <div class="max-h-28 space-y-1 overflow-y-auto pr-1">
            {#each targetSpaceOverlay.rows as row (row.key)}
              <div class={`grid grid-cols-[0.6rem_minmax(0,1fr)] items-center gap-2 text-[0.65rem] leading-4 ${row.stale ? 'opacity-45 text-surface-400' : 'opacity-100 text-surface-200'}`}>
                <div class="h-2 w-2 rounded-full" style={`background:${row.color}; opacity:${row.stale ? 0.5 : 1};`}></div>
                <div class={`truncate font-mono tabular-nums ${row.stale ? 'text-surface-400' : 'text-surface-100'}`}>
                  {compactPoseValues(row.values)}
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

    <!-- Anchor/tag-frame viewer controls removed for now (camera_in_tag/robot_in_tag are not exposed). -->
  </div>

  {#if !showImuRotationOverlay}
    <div class="pointer-events-auto absolute bottom-20 right-4 z-[69]">
      <button
        type="button"
        class="rounded border border-surface-700/70 bg-surface-950/88 px-3 py-2 text-micro-tight uppercase tracking-[0.24em] text-surface-200 shadow-lg transition hover:border-primary-400/80 hover:text-primary-100"
        onclick={openImuRotationOverlay}
      >
        IMU Viewer
      </button>
    </div>
  {/if}

  {#if showImuRotationOverlay}
    <div
      bind:this={imuWindowEl}
      class="pointer-events-auto absolute z-[70] max-w-[96vw] overflow-hidden rounded border border-surface-800 bg-surface-950/80 text-xs text-surface-200 shadow-xl backdrop-blur"
      style={imuWindowStyle()}
    >
      {#if imuRotationData}
        <div class="relative h-full w-full">
          <ImuOrientationViewer
            orientation={imuOrientationForViewer}
            showReferenceControls={false}
            showLegend={false}
            showWorldDecorations={false}
            showGroundPlane={true}
            cameraDistanceScale={0.22}
          />
          <div class="pointer-events-none absolute inset-0 flex flex-col justify-between p-3">
            <div class="flex items-start justify-between gap-2">
              <div
                class="pointer-events-auto max-w-[75%] cursor-move select-none rounded border border-surface-700/70 bg-surface-950/72 px-2 py-1.5"
                style="touch-action:none;"
                role="presentation"
                onpointerdown={beginImuWindowDrag}
              >
                <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-300">IMU Rotation</p>
                <p class="truncate text-[0.65rem] text-surface-400">{imuRotationData.sourceLabel} · {imuRotationData.outputKey}</p>
              </div>
              <button
                type="button"
                class="pointer-events-auto rounded border border-surface-700/70 bg-surface-950/72 px-2 py-0.5 text-micro-tight uppercase tracking-[0.2em] text-surface-300 transition hover:border-primary-400/80 hover:text-primary-100"
                onclick={() => (showImuRotationOverlay = false)}
              >
                Close
              </button>
            </div>

            <div class="space-y-2">
              <div class="rounded border border-surface-700/70 bg-surface-950/72 px-2 py-1.5 font-mono tabular-nums text-[0.67rem]">
                <p>
                  <span style="color: var(--axis-roll)">R {formatSigned(imuRotationData.roll, 2)}°</span>
                  <span class="mx-1 text-surface-600">|</span>
                  <span style="color: var(--axis-pitch)">P {formatSigned(imuRotationData.pitch, 2)}°</span>
                  <span class="mx-1 text-surface-600">|</span>
                  <span style="color: var(--axis-yaw)">Y {formatSigned(imuRotationData.yaw, 2)}°</span>
                </p>
                {#if imuRotationData.quaternion}
                  <p class="truncate text-[0.62rem] text-surface-400">
                    q ({formatFixed(imuRotationData.quaternion.x, 3)}, {formatFixed(imuRotationData.quaternion.y, 3)}, {formatFixed(imuRotationData.quaternion.z, 3)}, {formatFixed(imuRotationData.quaternion.w, 3)})
                  </p>
                {/if}
                {#if imuRotationData.translation}
                  <p class="truncate text-[0.62rem] text-surface-400">
                    t ({formatFixed(imuRotationData.translation.x, 3)}, {formatFixed(imuRotationData.translation.y, 3)}, {formatFixed(imuRotationData.translation.z, 3)})
                  </p>
                {/if}
              </div>
              <div class="flex items-center justify-between rounded border border-surface-700/70 bg-surface-950/72 px-2 py-1 font-mono tabular-nums text-[0.62rem] text-surface-400">
                <span>age {Math.max(0, imuRotationData.ageMs).toFixed(0)} ms</span>
                <span>sample {imuRotationData.sampleTimestampMs != null ? `${imuRotationData.sampleTimestampMs.toFixed(0)} ms` : '—'}</span>
              </div>
            </div>
          </div>
        </div>
      {:else}
        <div class="relative h-full w-full bg-surface-950/90">
          <div class="absolute left-3 top-3">
            <div
              class="cursor-move select-none rounded border border-surface-700/70 bg-surface-950/72 px-2 py-0.5 text-micro-tight uppercase tracking-[0.2em] text-surface-300"
              style="touch-action:none;"
              role="presentation"
              onpointerdown={beginImuWindowDrag}
            >
              IMU Rotation
            </div>
          </div>
          <div class="absolute right-3 top-3">
            <button
              type="button"
              class="rounded border border-surface-700/70 bg-surface-950/72 px-2 py-0.5 text-micro-tight uppercase tracking-[0.2em] text-surface-300 transition hover:border-primary-400/80 hover:text-primary-100"
              onclick={() => (showImuRotationOverlay = false)}
            >
              Close
            </button>
          </div>
          <div class="flex h-full items-center justify-center p-4 text-micro-tight text-surface-400">
            {imuRotationStatusMessage ?? 'Waiting for IMU sample...'}
          </div>
        </div>
      {/if}
      <div
        class="absolute bottom-0 right-0 h-5 w-5 cursor-se-resize pointer-events-auto"
        style="touch-action:none;"
        role="presentation"
        onpointerdown={beginImuWindowResize}
      >
        <div class="absolute bottom-1 right-1 h-2.5 w-2.5 border-b border-r border-surface-400/80"></div>
      </div>
    </div>
  {/if}

  {#if showOutputsOverlay}
    <LocalizationConfigPanel
      open={showOutputsOverlay}
      onClose={() => (showOutputsOverlay = false)}
      activeProfileId={activeProfileId}
      localizationConfigLoading={localizationConfigLoading}
      bind:profileNameInput={profileNameInput}
      onCommitProfileName={onCommitProfileName}
      {activeSolverId}
      {solvers}
      onSetActiveSolverId={onSetActiveSolverId}
      onAddSolver={onAddSolver}
      onRemoveActiveSolver={onRemoveActiveSolver}
      bind:solverNameInput={solverNameInput}
      onCommitSolverName={onCommitSolverName}
      activeSolverMode={activeSolverMode}
      supportedSolverModes={supportedSolverModes}
      onSetSolverMode={onSetSolverMode}
      activeSolverSourceIds={activeSolverSourceIds}
      onSetActiveSolverUseAllSources={onSetActiveSolverUseAllSources}
      onToggleActiveSolverSource={onToggleActiveSolverSource}
      solvePoseSpaces={solvePoseSpaces}
      derivedPoseSpaces={derivedPoseSpaces}
      poseSpaceLabel={poseSpaceLabel}
      selectedFieldMapId={selectedFieldMapId}
      calibrationReady={calibrationReady}
      uncalibratedSourcesCount={uncalibratedSourcesCount}
      bind:tagSizeInput={tagSizeInput}
      tagSizeError={tagSizeError}
      onCommitTagSize={onCommitTagSize}
      bind:excludedTagIdsInput={excludedTagIdsInput}
      excludedTagIdsError={excludedTagIdsError}
      onCommitExcludedTagIds={onCommitExcludedTagIds}
      fieldOriginMode={fieldOriginMode}
      fieldOriginCustom={fieldOriginCustom}
      onSetFieldOriginMode={onSetFieldOriginMode}
      onSetFieldOriginCustomNumeric={onSetFieldOriginCustomNumeric}
      snapZToGround={snapZToGround}
      snapRollToGround={snapRollToGround}
      snapPitchToGround={snapPitchToGround}
      onSetSnapZToGround={onSetSnapZToGround}
      onSetSnapRollToGround={onSetSnapRollToGround}
      onSetSnapPitchToGround={onSetSnapPitchToGround}
      profileTemporalStabilization={profileTemporalStabilization}
      onSetProfileTemporalEnabled={onSetProfileTemporalEnabled}
      onSetProfileTemporalNumeric={onSetProfileTemporalNumeric}
      activeSolverTemporalOverride={activeSolverTemporalOverride}
      activeSolverTemporalEffective={activeSolverTemporalEffective}
      onSetSolverTemporalOverrideEnabled={onSetSolverTemporalOverrideEnabled}
      onSetSolverTemporalEnabled={onSetSolverTemporalEnabled}
      onSetSolverTemporalNumeric={onSetSolverTemporalNumeric}
      activeSolverRuntimeTuning={activeSolverRuntimeTuning}
      onSetSolverRuntimeTuningNumeric={onSetSolverRuntimeTuningNumeric}
      fieldMaps={fieldMaps}
      fieldMapsLoading={fieldMapsLoading}
      fieldMapsError={fieldMapsError}
      mapUploadBusy={mapUploadBusy}
      mapUploadError={mapUploadError}
      bind:fieldMapSelection={fieldMapSelection}
      onSetFieldMapSelection={onSetFieldMapSelection}
      onUploadMapFile={onUploadMapFile}
      compatibleSourcesCount={compatibleSourcesCount}
      sourcesLoading={sourcesLoading}
      sourcesError={sourcesError}
      groupedSources={groupedSources}
      openSourceGroups={openSourceGroups}
      onToggleSourceGroup={onToggleSourceGroup}
      calibratedCameraIds={calibratedCameraIds}
      isSourceCalibrated={isSourceCalibrated}
      selectedSourceIds={selectedSourceIds}
      onToggleSource={onToggleSource}
      sourceWeightsById={sourceWeightsById}
      onSetSourceWeight={onSetSourceWeight}
      sourceUsedByProfilesById={sourceUsedByProfilesById}
      sourceStatusRows={sourceStatusRows}
    />
  {/if}

  <SolverPanel
    open={showMetricsOverlay}
    {feedStatus}
    {pollHz}
    {lastPollMs}
    sourceStatusRows={sourceStatusRows}
    onClose={() => (showMetricsOverlay = false)}
  />

  {#if showCameraPoseOverlay || showCustomFieldsOverlay}
    <div class="absolute inset-4 z-50 pointer-events-none">
      <div class="grid max-h-full grid-cols-1 content-start gap-3 lg:grid-cols-2">
        {#if showCameraPoseOverlay}
          <CameraPoseOverlay
            onClose={() => (showCameraPoseOverlay = false)}
            primaryCameraKey={primaryCameraKey}
            bind:cameraPoseXInput={cameraPoseXInput}
            bind:cameraPoseYInput={cameraPoseYInput}
            bind:cameraPoseZInput={cameraPoseZInput}
            bind:cameraPosePitchDeg={cameraPosePitchDeg}
            bind:cameraPoseYawDeg={cameraPoseYawDeg}
            bind:cameraPoseRollDeg={cameraPoseRollDeg}
            cameraPoseEditorError={cameraPoseEditorError}
            onReset={onResetPrimaryCameraPoseInputs}
            onApply={onApplyPrimaryCameraPose}
          />
        {/if}

        {#if showCustomFieldsOverlay}
          <FieldMapManager
            onClose={() => (showCustomFieldsOverlay = false)}
            bind:newCustomFieldName={newCustomFieldName}
            bind:newCustomFieldWidth={newCustomFieldWidth}
            bind:newCustomFieldDepth={newCustomFieldDepth}
            newCustomFieldError={newCustomFieldError}
            onCreateCustomField={onCreateCustomField}
            bind:newOriginName={newOriginName}
            bind:newOriginX={newOriginX}
            bind:newOriginZ={newOriginZ}
            bind:newOriginYaw={newOriginYaw}
            newOriginError={newOriginError}
            onAddOrigin={onAddOrigin}
            hasSelectedCustomField={Boolean(selectedCustomField)}
            activeFieldMapBitsStatus={activeFieldMapBitsStatus}
            onRefreshMaps={onRefreshMaps}
            fieldMapsLoading={fieldMapsLoading}
            mapUploadFile={mapUploadFile}
            mapUploadBusy={mapUploadBusy}
            mapUploadError={mapUploadError}
            fieldMapsError={fieldMapsError}
            fieldMaps={fieldMaps}
            bind:mapAssignId={mapAssignId}
            onAssignMap={onAssignMap}
            selectedFieldMapId={selectedFieldMapId}
            fieldMapDocErrors={fieldMapDocErrors}
            hasActiveProfile={hasActiveProfile}
            onSetMapUploadFile={onSetMapUploadFile}
            onUploadSelectedMapFile={onUploadSelectedMapFile}
          />
        {/if}
      </div>
    </div>
  {/if}
</div>
