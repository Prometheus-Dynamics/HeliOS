import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
import type {
  LocalizationCustomFieldOrigin,
  LocalizationFieldOriginMode,
  LocalizationPoseSpace,
  LocalizationSolverConfig,
  LocalizationSolverRuntimeTuningConfig,
  LocalizationSolverMode,
  LocalizationTemporalStabilizationConfig
} from '$lib/features/localization/localizationConfig';
import type {
  LocalizationFieldDefinition,
  LocalizationMarker,
  LocalizationViewMode
} from '$lib/features/localization/viewers/localizationViewerTypes';
import type ImuOrientationViewer from '$lib/components/ImuOrientationViewer.svelte';
import type LocalizationViewers from '$lib/components/LocalizationViewers.svelte';
import type { FieldMapSummary } from '$lib/features/localization/fieldMaps';
import type { PoseQuaternion, Vec3 } from '$lib/features/localization/poseMath';
import type { CustomField } from '$lib/features/localization/types';
import type { SensorOrientation } from '$lib/types/devices';
import type { RigCameraInfo, RobotDimensions } from '$lib/types/rig';

export type ViewerTransform = { position: Vec3; quaternion?: PoseQuaternion } | null;

export type ViewProfileOverlay = {
  id: string;
  label?: string;
  color?: string;
  transform: ViewerTransform;
};

export type CameraPovOption = {
  id: string;
  groupLabel: string;
  subgroupLabel: string;
  label: string;
  available: boolean;
  ghost: boolean;
};

export type CameraPovFovMode = 'undistorted' | 'raw' | 'none';
export type CameraPovOptionSubgroup = { label: string; options: CameraPovOption[] };
export type CameraPovOptionGroup = { label: string; subgroups: CameraPovOptionSubgroup[] };
export const ROBOT_FOLLOW_POV_OPTION_ID = '__robot_follow__';
export type RuntimeTuningFieldKey = Extract<keyof LocalizationSolverRuntimeTuningConfig, string>;

export type SourceGroup = {
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

export type SourceStatusRow = {
  source: LocalizationPipelineSource;
  pollMs: number;
  detections: number;
  tagSize: number | null;
  graphMs: number | null;
  metricsUpdatedAt: number | null;
  metricsError: string | null;
  error: string | null;
};

export type ProfileTimingRow = {
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

export type ImuRotationOverlayData = {
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

export type ImuOrientationViewerComponent = typeof ImuOrientationViewer;

export type LocalizationWorkspaceProps = {
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
  activeSolveMs?: number | null;
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
  profileTimingRows?: ProfileTimingRow[];
  showCameraPoseOverlay?: boolean;
  showCustomFieldsOverlay?: boolean;
  showImuRotationOverlay?: boolean;
  openImuRotationOverlay?: () => void;
  imuWindowEl?: HTMLElement | null;
  imuWindowStyle?: () => string;
  ImuOrientationViewerComponent?: ImuOrientationViewerComponent | null;
  imuOrientationForViewer?: SensorOrientation | null;
  imuRotationData?: ImuRotationOverlayData | null;
  imuRotationStatusMessage?: string | null;
  beginImuWindowDrag?: (event: PointerEvent) => void;
  beginImuWindowResize?: (event: PointerEvent) => void;
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
