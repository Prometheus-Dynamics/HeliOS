<script lang="ts">
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
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';
  import type {
    RuntimeTuningFieldKey,
    RuntimeTuningGroup,
    SourceGroup,
    SourceStatusRow
  } from './localizationConfigEditorTypes';
  import LocalizationConfigSourcesTab from './LocalizationConfigSourcesTab.svelte';
  import LocalizationConfigSolverTab from './LocalizationConfigSolverTab.svelte';
  import LocalizationConfigFieldTab from './LocalizationConfigFieldTab.svelte';
  import LocalizationConfigAdvancedTab from './LocalizationConfigAdvancedTab.svelte';
  type LocalizationConfigPanelProps = {
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

  let {
    open = false,
    onClose,
    activeProfileId = $bindable(''),
    localizationConfigLoading = false,
    profileNameInput = $bindable(''),
    onCommitProfileName,
    activeSolverId = $bindable(''),
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
    poseSpaceLabel = (space) => space,
    selectedFieldMapId = null,
    calibrationReady = false,
    uncalibratedSourcesCount = 0,
    tagSizeInput = $bindable(''),
    tagSizeError = null,
    onCommitTagSize,
    excludedTagIdsInput = $bindable(''),
    excludedTagIdsError = null,
    onCommitExcludedTagIds,
    fieldOriginMode = 'blue',
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
    sourceStatusRows = []
  }: LocalizationConfigPanelProps = $props();

  const hasActiveProfile = $derived(Boolean(activeProfileId));
  const solverCount = $derived(solvers.length);
  const canRemoveSolver = $derived(solverCount > 1);
  const solverUsesAllSources = $derived(activeSolverSourceIds.length === 0);
  const solverHasTemporalOverride = $derived(activeSolverTemporalOverride !== null);
  const solverModeDisplayLabel = (mode: LocalizationSolverMode): string => {
    switch (mode) {
      case 'group_solve':
        return 'Group solve';
      case 'robust_group_solve':
        return 'Robust group solve (outlier-pruned)';
      case 'per_camera_merge':
        return 'Per-camera merge';
      case 'triangulate':
        return 'Triangulate (merge cameras)';
      default:
        return mode;
    }
  };
  const supportedSolverModeSet = $derived.by(() => new SvelteSet(supportedSolverModes));
  const visibleSolverModes = $derived.by<LocalizationSolverMode[]>(() => {
    const seen = new SvelteSet<string>();
    const ordered: LocalizationSolverMode[] = [];
    for (const mode of supportedSolverModes) {
      const normalized = String(mode ?? '').trim();
      if (!normalized || seen.has(normalized)) continue;
      seen.add(normalized);
      ordered.push(mode);
    }
    if (activeSolverMode && !seen.has(activeSolverMode)) {
      ordered.push(activeSolverMode);
    }
    return ordered;
  });
  type SetupTab = 'sources' | 'solver' | 'field' | 'advanced';
  type SetupTabSpec = {
    id: SetupTab;
    label: string;
    hint: string;
  };
  const setupTabs: SetupTabSpec[] = [
    { id: 'sources', label: 'Sources', hint: 'Choose and filter pipeline outputs used for localization.' },
    { id: 'solver', label: 'Solver', hint: 'Select solver strategy and map selected sources into active solves.' },
    { id: 'field', label: 'Field', hint: 'Set tag size, origin rules, and field map assignment for this profile.' },
    { id: 'advanced', label: 'Advanced', hint: 'Tune runtime constants and temporal stabilization defaults.' }
  ];
  let setupTab = $state<SetupTab>('sources');
  const activeSetupTab = $derived.by(() => setupTabs.find((tab) => tab.id === setupTab) ?? setupTabs[0]);
  let sourceSearch = $state('');
  let sourceFilter = $state<'all' | 'selected'>('all');

  const selectedSourceSet = $derived.by(() => new SvelteSet(selectedSourceIds));
  const sourceStatusById = $derived.by(() => {
    const entries: Array<[string, SourceStatusRow]> = sourceStatusRows.map((entry) => [entry.source.id, entry]);
    return new SvelteMap(entries);
  });
  const normalizedSourceSearch = $derived.by(() => sourceSearch.trim().toLowerCase());
  const showSelectedOnly = $derived.by(() => sourceFilter === 'selected');
  $effect(() => {
    if (!open) {
      setupTab = 'sources';
      activeSourceGroupTab = 'all';
      activeSolverGroupTab = 'all';
    }
  });

  const filteredSourceGroups = $derived.by<SourceGroup[]>(() => {
    const search = normalizedSourceSearch;
    const filtered = groupedSources
      .map((group) => {
        const pipelines = group.pipelines
          .map((pipeline) => {
            const sources = pipeline.sources.filter((source) => {
              if (showSelectedOnly && !selectedSourceSet.has(source.id)) {
                return false;
              }
              if (!search) {
                return true;
              }
              const tokens = [
                source.outputKey,
                source.streamLabel,
                source.streamId,
                source.pipelineLabel,
                source.cameraUid,
                source.cameraPath,
                group.label,
                pipeline.label,
                dataTypeLabel(source)
              ]
                .filter(Boolean)
                .join(' ')
                .toLowerCase();
              return tokens.includes(search);
            });
            if (sources.length === 0) return null;
            return { ...pipeline, sources };
          })
          .filter((pipeline): pipeline is SourceGroup['pipelines'][number] => Boolean(pipeline));
        if (pipelines.length === 0) return null;
        return { ...group, pipelines };
      })
      .filter((group): group is SourceGroup => Boolean(group));
    return filtered.sort((left, right) => {
      const leftSelectable = left.pipelines.reduce(
        (count, pipeline) => count + pipeline.sources.filter((source) => !selectedSourceSet.has(source.id)).length,
        0
      );
      const rightSelectable = right.pipelines.reduce(
        (count, pipeline) => count + pipeline.sources.filter((source) => !selectedSourceSet.has(source.id)).length,
        0
      );
      const leftHasChoices = leftSelectable > 0;
      const rightHasChoices = rightSelectable > 0;
      if (leftHasChoices !== rightHasChoices) {
        return leftHasChoices ? -1 : 1;
      }
      return left.label.localeCompare(right.label);
    });
  });
  const compatibleSelectedCount = $derived.by(() =>
    groupedSources.reduce(
      (count, group) =>
        count +
        group.pipelines.reduce(
          (pipelineCount, pipeline) =>
            pipelineCount + pipeline.sources.filter((source) => selectedSourceSet.has(source.id)).length,
          0
        ),
      0
    )
  );
  let activeSourceGroupTab = $state<string>('all');
  const visibleSourceGroups = $derived.by<SourceGroup[]>(() =>
    activeSourceGroupTab === 'all'
      ? filteredSourceGroups
      : filteredSourceGroups.filter((group) => group.key === activeSourceGroupTab)
  );
  const selectedSourceGroups = $derived.by<SourceGroup[]>(() =>
    groupedSources
      .map((group) => {
        const pipelines = group.pipelines
          .map((pipeline) => {
            const sources = pipeline.sources.filter((source) => selectedSourceSet.has(source.id));
            if (sources.length === 0) return null;
            return { ...pipeline, sources };
          })
          .filter((pipeline): pipeline is SourceGroup['pipelines'][number] => Boolean(pipeline));
        if (pipelines.length === 0) return null;
        return { ...group, pipelines };
      })
      .filter((group): group is SourceGroup => Boolean(group))
  );
  const solverSourceSet = $derived.by<Set<string>>(() =>
    solverUsesAllSources ? new SvelteSet(selectedSourceIds) : new SvelteSet(activeSolverSourceIds)
  );
  const solverModeLabel = $derived.by(() => {
    if (!activeSolverMode) return 'Unset';
    return solverModeDisplayLabel(activeSolverMode);
  });
  const solverSourceCount = $derived.by(() => {
    const compatibleIds = new SvelteSet(
      groupedSources.flatMap((group) => group.pipelines.flatMap((pipeline) => pipeline.sources.map((source) => source.id)))
    );
    let count = 0;
    for (const sourceId of solverSourceSet) {
      if (compatibleIds.has(sourceId)) count += 1;
    }
    return count;
  });
  const selectedSourceCount = $derived.by(() =>
    selectedSourceGroups.reduce(
      (count, group) =>
        count + group.pipelines.reduce((pipelineCount, pipeline) => pipelineCount + pipeline.sources.length, 0),
      0
    )
  );
  const solverGroupSummaries = $derived.by<
    Array<{
      key: string;
      label: string;
      kind: string;
      enabled: number;
      total: number;
      uncalibrated: number;
    }>
  >(() =>
    selectedSourceGroups.map((group) => {
      let enabled = 0;
      let total = 0;
      let uncalibrated = 0;
      for (const pipeline of group.pipelines) {
        for (const source of pipeline.sources) {
          total += 1;
          if (solverSourceSet.has(source.id)) enabled += 1;
          if (!isSourceCalibrated(source, calibratedCameraIds)) uncalibrated += 1;
        }
      }
      return {
        key: group.key,
        label: group.label,
        kind: group.kind ?? (group.key.startsWith('peer:') ? 'peer' : 'stream'),
        enabled,
        total,
        uncalibrated
      };
    })
  );
  let activeSolverGroupTab = $state<string>('all');
  const visibleSolverGroups = $derived.by<SourceGroup[]>(() =>
    activeSolverGroupTab === 'all'
      ? selectedSourceGroups
      : selectedSourceGroups.filter((group) => group.key === activeSolverGroupTab)
  );
  $effect(() => {
    if (activeSourceGroupTab === 'all') return;
    if (!filteredSourceGroups.some((group) => group.key === activeSourceGroupTab)) {
      activeSourceGroupTab = 'all';
    }
  });
  $effect(() => {
    if (activeSolverGroupTab === 'all') return;
    if (!selectedSourceGroups.some((group) => group.key === activeSolverGroupTab)) {
      activeSolverGroupTab = 'all';
    }
  });

  function dataTypeLabel(source: LocalizationPipelineSource): string {
    const fallbackType = (() => {
      const key = String(source.outputKey ?? '').toLowerCase();
      if (key.includes('imu')) return 'imu pose';
      if (key.includes('aruco') || key.includes('detect') || key.includes('tag')) return 'detections';
      if (key.includes('tag_in_') || key.includes('camera_in_') || key.includes('robot_in_') || key.startsWith('solver:')) return 'pose';
      if (key.includes('pose')) return 'pose';
      if (key.includes('frame') || key.includes('image')) return 'image';
      return 'untyped';
    })();
    const dataType = source.dataType;
    if (!dataType) return fallbackType;
    if (typeof dataType === 'string') {
      const label = dataType.trim();
      if (!label || label.toLowerCase() === 'unknown') return fallbackType;
      return label;
    }
    const label = String(dataType.label ?? dataType.kind ?? '').trim();
    if (!label) return fallbackType;
    const format = String(dataType.format ?? '').trim();
    return format ? `${label} (${format})` : label;
  }

  function isDetectionSource(source: LocalizationPipelineSource): boolean {
    const key = String(source.outputKey ?? '').toLowerCase();
    return key.includes('aruco') || key.includes('detect') || key.includes('tag');
  }

  function sourceSharedProfiles(sourceId: string): string[] {
    return sourceUsedByProfilesById[sourceId] ?? [];
  }

  function sourceCardTone(source: LocalizationPipelineSource, row: SourceStatusRow | null): string {
    const selected = selectedSourceSet.has(source.id);
    if (selected && row?.error) return 'border-rose-500/50 bg-rose-500/10';
    if (selected) return 'border-primary-500/40 bg-primary-500/10';
    if (row?.error) return 'border-rose-500/35 bg-rose-500/5';
    return 'border-surface-800/70 bg-surface-950/60';
  }

  function readInputValue(event: Event): string | null {
    const input = event.currentTarget;
    return input instanceof HTMLInputElement ? input.value : null;
  }

  function readInputChecked(event: Event): boolean | null {
    const input = event.currentTarget;
    return input instanceof HTMLInputElement ? input.checked : null;
  }

  function readSelectValue(event: Event): string | null {
    const select = event.currentTarget;
    return select instanceof HTMLSelectElement ? select.value : null;
  }

  function handleProfileNameKeydown(event: KeyboardEvent) {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    onCommitProfileName?.();
    const input = event.currentTarget;
    if (input instanceof HTMLInputElement) {
      input.blur();
    }
  }

  function handleToggleSource(sourceId: string, event: Event) {
    const checked = readInputChecked(event);
    if (checked == null) return;
    onToggleSource?.(sourceId, checked);
  }

  function handleSetActiveSolverId(event: Event) {
    const value = readSelectValue(event);
    if (value == null) return;
    onSetActiveSolverId?.(value);
  }

  function handleSetSolverMode(event: Event) {
    const value = readSelectValue(event);
    if (value == null || !visibleSolverModes.includes(value as LocalizationSolverMode)) return;
    onSetSolverMode?.(value as LocalizationSolverMode);
  }

  function handleSetSolverTemporalOverrideEnabled(event: Event) {
    const checked = readInputChecked(event);
    if (checked == null) return;
    onSetSolverTemporalOverrideEnabled?.(checked);
  }

  function handleSetSolverTemporalEnabled(event: Event) {
    const checked = readInputChecked(event);
    if (checked == null) return;
    onSetSolverTemporalEnabled?.(checked);
  }

  function handleSetSolverTemporalNumeric(field: string, event: Event) {
    const value = readInputValue(event);
    if (value == null) return;
    onSetSolverTemporalNumeric?.(field, value);
  }

  function handleSetActiveSolverUseAllSources(event: Event) {
    const checked = readInputChecked(event);
    if (checked == null) return;
    onSetActiveSolverUseAllSources?.(checked);
  }

  function handleToggleActiveSolverSource(sourceId: string, event: Event) {
    const checked = readInputChecked(event);
    if (checked == null) return;
    onToggleActiveSolverSource?.(sourceId, checked);
  }

  function handleSetSourceWeight(sourceId: string, event: Event) {
    const value = readInputValue(event);
    if (value == null) return;
    onSetSourceWeight?.(sourceId, value);
  }

  function handleSetFieldOriginMode(event: Event) {
    const value = readSelectValue(event);
    if (value !== 'blue' && value !== 'red' && value !== 'center' && value !== 'custom') return;
    onSetFieldOriginMode?.(value);
  }

  function handleSetFieldOriginCustomNumeric(field: 'x' | 'z' | 'yawDeg', event: Event) {
    const value = readInputValue(event);
    if (value == null) return;
    onSetFieldOriginCustomNumeric?.(field, value);
  }

  function handleSetSnapZToGround(event: Event) {
    const checked = readInputChecked(event);
    if (checked == null) return;
    onSetSnapZToGround?.(checked);
  }

  function handleSetSnapRollToGround(event: Event) {
    const checked = readInputChecked(event);
    if (checked == null) return;
    onSetSnapRollToGround?.(checked);
  }

  function handleSetSnapPitchToGround(event: Event) {
    const checked = readInputChecked(event);
    if (checked == null) return;
    onSetSnapPitchToGround?.(checked);
  }

  function handleSetFieldMapSelection(event: Event) {
    const value = readSelectValue(event);
    if (value == null) return;
    onSetFieldMapSelection?.(value);
  }

  function handleUploadMapFile(event: Event) {
    const input = event.currentTarget;
    if (!(input instanceof HTMLInputElement)) return;
    const file = input.files?.[0] ?? null;
    if (!file) return;
    onUploadMapFile?.(file);
  }

  function handleSetProfileTemporalEnabled(event: Event) {
    const checked = readInputChecked(event);
    if (checked == null) return;
    onSetProfileTemporalEnabled?.(checked);
  }

  function handleSetProfileTemporalNumeric(field: string, event: Event) {
    const value = readInputValue(event);
    if (value == null) return;
    onSetProfileTemporalNumeric?.(field, value);
  }

  function handleSetSolverRuntimeTuningNumeric(field: RuntimeTuningFieldKey, event: Event) {
    const value = readInputValue(event);
    if (value == null) return;
    onSetSolverRuntimeTuningNumeric?.(field, value);
  }

  const runtimeTuningGroups: RuntimeTuningGroup[] = [
    {
      id: 'observation',
      label: 'Observation thresholds',
      description: 'Solve acceptance thresholds and single-vs-multi-tag confidence gates.',
      fields: [
        { key: 'minObservationWeight', label: 'Min observation weight', step: '0.01' },
        { key: 'minSingleTagSolveWeight', label: 'Min single-tag solve weight', step: '0.01' },
        { key: 'minMultiTagTotalWeight', label: 'Min multi-tag total weight', step: '0.01' },
        { key: 'minMultiTagEffectiveCount', label: 'Min multi-tag effective count', step: '0.01' },
        { key: 'weakSingleTagMargin', label: 'Weak single-tag margin', step: '0.01' }
      ]
    },
    {
      id: 'height-penalties',
      label: 'Per-camera merge penalties',
      description: 'Height-delta thresholds and per-tier penalties for merge weighting.',
      fields: [
        { key: 'coplanarHeightDeltaM', label: 'Co-planar map height delta (m)', step: '0.01' },
        { key: 'severeObservedHeightDeltaM', label: 'Severe observed height delta (m)', step: '0.01' },
        { key: 'moderateObservedHeightDeltaM', label: 'Moderate observed height delta (m)', step: '0.01' },
        { key: 'mildObservedHeightDeltaM', label: 'Mild observed height delta (m)', step: '0.01' },
        { key: 'severePenalty', label: 'Severe penalty', step: '0.01' },
        { key: 'moderatePenalty', label: 'Moderate penalty', step: '0.01' },
        { key: 'mildPenalty', label: 'Mild penalty', step: '0.01' }
      ]
    },
    {
      id: 'dt-scale',
      label: 'Temporal dt scaling',
      description: 'Clamp the frame-time scaling applied to temporal gains.',
      fields: [
        { key: 'dtScaleMin', label: 'Dt scale min', step: '0.01' },
        { key: 'dtScaleMax', label: 'Dt scale max', step: '0.01' }
      ]
    },
    {
      id: 'switch-single',
      label: 'Tag switch handling',
      description: 'Caps, reject windows, and gain dampening when switching single-tag identities.',
      fields: [
        { key: 'switchedSingleTagMaxTranslationJumpM', label: 'Switch max translation jump (m)', step: '0.001' },
        { key: 'switchedSingleTagMaxRotationJumpDeg', label: 'Switch max rotation jump (deg)', step: '0.01' },
        { key: 'switchedSingleTagRejectWindowScale', label: 'Switch reject window scale', step: '0.01' },
        { key: 'switchedSingleTagRejectWindowMinMs', label: 'Switch reject window min (ms)', step: '1' },
        { key: 'switchedSingleTagGainDamp', label: 'Switch gain damp', step: '0.01' },
        { key: 'switchedSingleTagMinTranslationGain', label: 'Switch min translation gain', step: '0.01' },
        { key: 'switchedSingleTagMinRotationGain', label: 'Switch min rotation gain', step: '0.01' }
      ]
    },
    {
      id: 'drop-single',
      label: 'Multi-to-single drop handling',
      description: 'Caps, reject windows, and gain dampening when dropping to single-tag tracking.',
      fields: [
        { key: 'droppedMultiToSingleMaxTranslationJumpM', label: 'Drop max translation jump (m)', step: '0.001' },
        { key: 'droppedMultiToSingleMaxRotationJumpDeg', label: 'Drop max rotation jump (deg)', step: '0.01' },
        { key: 'droppedMultiToSingleRejectWindowScale', label: 'Drop reject window scale', step: '0.01' },
        { key: 'droppedMultiToSingleRejectWindowMinMs', label: 'Drop reject window min (ms)', step: '1' },
        { key: 'droppedMultiToSingleGainDamp', label: 'Drop gain damp', step: '0.01' },
        { key: 'droppedMultiToSingleMinTranslationGain', label: 'Drop min translation gain', step: '0.01' },
        { key: 'droppedMultiToSingleMinRotationGain', label: 'Drop min rotation gain', step: '0.01' }
      ]
    }
  ];

  export type $$Props = LocalizationConfigPanelProps;
</script>

{#if open}
  <div
    class="fixed inset-0 z-60 flex items-center justify-center px-4 py-6"
    role="dialog"
    aria-modal="true"
    aria-labelledby="localization-setup-title"
  >
    <button
      class="absolute inset-0 bg-black/70"
      type="button"
      aria-label="Close localization setup"
      onclick={onClose}
    ></button>
    <aside class="relative z-10 flex h-[min(90dvh,58rem)] w-full max-w-6xl flex-col overflow-hidden rounded-lg border border-surface-700 bg-surface-950 text-surface-100 shadow-2xl">
      <header class="flex items-start justify-between gap-4 border-b border-surface-800 px-6 py-4">
        <div>
          <p class="text-xs uppercase tracking-[0.35em] text-surface-500">Localization</p>
          <h2 id="localization-setup-title" class="text-xl font-semibold text-surface-50">Setup</h2>
          <p class="mt-1 text-sm text-surface-400">{activeSetupTab.hint}</p>
        </div>
        <button
          class="btn btn-2xs preset-tonal uppercase tracking-[0.3em]"
          type="button"
          onclick={onClose}
          aria-label="Close localization setup"
        >
          Close
        </button>
      </header>

      <div class="flex min-h-0 flex-1 flex-col px-6 py-4">
        <div class="flex shrink-0 flex-wrap items-center gap-2 rounded border border-surface-800 bg-surface-900/40 p-1.5">
          {#each setupTabs as tab (tab.id)}
            <button
              class={`btn btn-2xs uppercase tracking-[0.24em] ${setupTab === tab.id ? 'preset-filled-primary-500' : 'preset-tonal'}`}
              type="button"
              onclick={() => (setupTab = tab.id)}
            >
              {tab.label}
            </button>
          {/each}
        </div>
        <div class="mt-3 rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2.5">
          <label class="text-micro-tight uppercase tracking-[0.24em] text-surface-500">
            Profile name
            <input
              class="mt-1.5 w-full rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs text-surface-100 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
              placeholder="Profile name"
              bind:value={profileNameInput}
              onchange={onCommitProfileName}
              onkeydown={handleProfileNameKeydown}
              disabled={localizationConfigLoading || !hasActiveProfile}
            />
          </label>
        </div>

        <div class="mt-3 min-h-0 flex-1">
          {#if setupTab === 'sources'}
            <LocalizationConfigSourcesTab
              compatibleSelectedCount={compatibleSelectedCount}
              compatibleSourcesCount={compatibleSourcesCount}
              bind:sourceSearch={sourceSearch}
              bind:sourceFilter={sourceFilter}
              {filteredSourceGroups}
              bind:activeSourceGroupTab={activeSourceGroupTab}
              {visibleSourceGroups}
              openSourceGroups={openSourceGroups}
              {sourcesLoading}
              {sourcesError}
              {localizationConfigLoading}
              calibratedCameraIds={calibratedCameraIds}
              selectedSourceSet={selectedSourceSet}
              sourceStatusById={sourceStatusById}
              {isSourceCalibrated}
              {dataTypeLabel}
              {isDetectionSource}
              {sourceSharedProfiles}
              {sourceCardTone}
              onToggleSourceGroup={onToggleSourceGroup}
              onToggleSource={onToggleSource}
            />
          {/if}

          {#if setupTab !== 'sources'}
          <div class="h-full min-h-0 space-y-4 overflow-y-auto pr-1">
          {#if setupTab === 'solver'}
            <LocalizationConfigSolverTab
              {solvePoseSpaces}
              {derivedPoseSpaces}
              {poseSpaceLabel}
              {solvers}
              bind:activeSolverId={activeSolverId}
              {localizationConfigLoading}
              {canRemoveSolver}
              bind:solverNameInput={solverNameInput}
              {activeSolverMode}
              {visibleSolverModes}
              supportedSolverModeSet={supportedSolverModeSet}
              {solverModeDisplayLabel}
              onSetActiveSolverId={onSetActiveSolverId}
              onAddSolver={onAddSolver}
              onRemoveActiveSolver={onRemoveActiveSolver}
              onCommitSolverName={onCommitSolverName}
              onSetSolverMode={onSetSolverMode}
              solverHasTemporalOverride={solverHasTemporalOverride}
              activeSolverTemporalOverride={activeSolverTemporalOverride}
              activeSolverTemporalEffective={activeSolverTemporalEffective}
              onSetSolverTemporalOverrideEnabled={onSetSolverTemporalOverrideEnabled}
              onSetSolverTemporalEnabled={onSetSolverTemporalEnabled}
              onSetSolverTemporalNumeric={onSetSolverTemporalNumeric}
              solverUsesAllSources={solverUsesAllSources}
              onSetActiveSolverUseAllSources={onSetActiveSolverUseAllSources}
              solverModeLabel={solverModeLabel}
              solverGroupSummaries={solverGroupSummaries}
              solverSourceCount={solverSourceCount}
              selectedSourceCount={selectedSourceCount}
              selectedSourceGroups={selectedSourceGroups}
              bind:activeSolverGroupTab={activeSolverGroupTab}
              visibleSolverGroups={visibleSolverGroups}
              solverSourceSet={solverSourceSet}
              sourceWeightsById={sourceWeightsById}
              calibratedCameraIds={calibratedCameraIds}
              {isSourceCalibrated}
              {sourceSharedProfiles}
              onToggleActiveSolverSource={onToggleActiveSolverSource}
              onSetSourceWeight={onSetSourceWeight}
              selectedFieldMapId={selectedFieldMapId}
              calibrationReady={calibrationReady}
              uncalibratedSourcesCount={uncalibratedSourcesCount}
            />
          {/if}

          {#if setupTab === 'field'}
            <LocalizationConfigFieldTab
              {fieldMaps}
              {fieldMapsLoading}
              {fieldMapsError}
              {mapUploadBusy}
              {mapUploadError}
              bind:fieldMapSelection={fieldMapSelection}
              hasActiveProfile={hasActiveProfile}
              {localizationConfigLoading}
              bind:tagSizeInput={tagSizeInput}
              {tagSizeError}
              bind:excludedTagIdsInput={excludedTagIdsInput}
              {excludedTagIdsError}
              {fieldOriginMode}
              {fieldOriginCustom}
              {snapZToGround}
              {snapRollToGround}
              {snapPitchToGround}
              onCommitTagSize={onCommitTagSize}
              onCommitExcludedTagIds={onCommitExcludedTagIds}
              onSetFieldOriginMode={onSetFieldOriginMode}
              onSetFieldOriginCustomNumeric={onSetFieldOriginCustomNumeric}
              onSetSnapZToGround={onSetSnapZToGround}
              onSetSnapRollToGround={onSetSnapRollToGround}
              onSetSnapPitchToGround={onSetSnapPitchToGround}
              onSetFieldMapSelection={onSetFieldMapSelection}
              onUploadMapFile={onUploadMapFile}
            />
          {/if}

          {#if setupTab === 'advanced'}
            <LocalizationConfigAdvancedTab
              hasActiveProfile={hasActiveProfile}
              {localizationConfigLoading}
              profileTemporalStabilization={profileTemporalStabilization}
              activeSolverRuntimeTuning={activeSolverRuntimeTuning}
              runtimeTuningGroups={runtimeTuningGroups}
              onSetProfileTemporalEnabled={onSetProfileTemporalEnabled}
              onSetProfileTemporalNumeric={onSetProfileTemporalNumeric}
              onSetSolverRuntimeTuningNumeric={onSetSolverRuntimeTuningNumeric}
            />
          {/if}

        </div>
          {/if}
      </div>
    </div>
    </aside>
  </div>
{/if}
