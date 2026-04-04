import type {
  LocalizationCustomFieldOrigin,
  LocalizationFieldOriginMode,
  LocalizationPoseSpace,
  LocalizationSolverConfig,
  LocalizationSolverMode,
  LocalizationSolverRuntimeTuningConfig,
  LocalizationTemporalStabilizationConfig
} from '$lib/features/localization/localizationConfig';
import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
import type { FieldMapSummary } from '$lib/features/localization/fieldMaps';
import type {
  RuntimeTuningFieldKey,
  SourceGroup,
  SourceStatusRow
} from './localizationConfigEditorTypes';

export type LocalizationConfigPanelProps = {
  open?: boolean;
  onClose?: () => void;
  activeProfileId?: string;
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
  poseSpaceLabel?: (space: LocalizationPoseSpace) => string;
  selectedFieldMapId?: string | null;
  calibrationReady?: boolean;
  uncalibratedSourcesCount?: number;
  tagSizeInput?: string;
  tagSizeError?: string | null;
  onCommitTagSize?: () => void;
  excludedTagIdsInput?: string;
  excludedTagIdsError?: string | null;
  onCommitExcludedTagIds?: () => void;
  fieldOriginMode?: LocalizationFieldOriginMode;
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
};
