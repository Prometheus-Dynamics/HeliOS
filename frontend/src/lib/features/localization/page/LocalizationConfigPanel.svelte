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

  type RuntimeTuningFieldSpec = {
    key: RuntimeTuningFieldKey;
    label: string;
    step: string;
  };

  type RuntimeTuningGroup = {
    id: string;
    label: string;
    description: string;
    fields: RuntimeTuningFieldSpec[];
  };

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
              onkeydown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  onCommitProfileName?.();
                  (event.currentTarget as HTMLInputElement).blur();
                }
              }}
              disabled={localizationConfigLoading || !hasActiveProfile}
            />
          </label>
        </div>

        <div class="mt-3 min-h-0 flex-1">
          {#if setupTab === 'sources'}
          <div class="flex h-full min-h-0 flex-col pr-1">
            <section class="flex min-h-0 flex-1 flex-col rounded border border-surface-800/70 bg-surface-900/40 p-3">
              <div class="flex flex-wrap items-center justify-between gap-3">
                <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Input sources</p>
                <span class="text-micro-tight text-surface-500">{compatibleSelectedCount}/{compatibleSourcesCount} selected</span>
              </div>
              <p class="mt-1 text-micro text-surface-500">
                Showing only localization-supported outputs.
              </p>

              <div class="mt-3 flex flex-wrap items-center gap-2">
                <input
                  class="min-w-[13rem] flex-1 rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs text-surface-100 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
                  placeholder="Search stream, output, pipeline, data type"
                  bind:value={sourceSearch}
                />
                <button
                  class={`btn btn-2xs uppercase tracking-[0.24em] ${sourceFilter === 'all' ? 'preset-filled-primary-500' : 'preset-tonal'}`}
                  type="button"
                  onclick={() => (sourceFilter = 'all')}
                >
                  All
                </button>
                <button
                  class={`btn btn-2xs uppercase tracking-[0.24em] ${sourceFilter === 'selected' ? 'preset-filled-primary-500' : 'preset-tonal'}`}
                  type="button"
                  onclick={() => (sourceFilter = 'selected')}
                >
                  Selected
                </button>
              </div>

              {#if filteredSourceGroups.length > 0}
                <div class="mt-3 flex flex-wrap items-center gap-1.5 rounded border border-surface-800/70 bg-surface-950/50 p-1.5">
                  <button
                    class={`rounded border px-2 py-1 text-micro-tight uppercase tracking-[0.22em] ${
                      activeSourceGroupTab === 'all'
                        ? 'border-primary-500/40 bg-primary-500/15 text-primary-100'
                        : 'border-surface-700/70 bg-surface-900/70 text-surface-300 hover:border-surface-500 hover:text-surface-100'
                    }`}
                    type="button"
                    onclick={() => (activeSourceGroupTab = 'all')}
                  >
                    All groups
                  </button>
                  {#each filteredSourceGroups as group (group.key)}
                    {@const tabHasCalibrated = group.pipelines.some((pipeline) =>
                      pipeline.sources.some((source) => isSourceCalibrated(source, calibratedCameraIds))
                    )}
                    <button
                      class={`rounded border px-2 py-1 text-micro-tight uppercase tracking-[0.22em] ${
                        activeSourceGroupTab === group.key
                          ? 'border-primary-500/40 bg-primary-500/15 text-primary-100'
                          : 'border-surface-700/70 bg-surface-900/70 text-surface-300 hover:border-surface-500 hover:text-surface-100'
                      }`}
                      type="button"
                      onclick={() => (activeSourceGroupTab = group.key)}
                    >
                      {group.label}
                      {#if !tabHasCalibrated}
                        <span class="ml-1 inline-flex h-4 w-4 items-center justify-center rounded-full border border-amber-500/40 bg-amber-500/10 text-[0.62rem] text-amber-200">!</span>
                      {/if}
                    </button>
                  {/each}
                </div>
              {/if}

              <div class="mt-3 min-h-0 flex-1 space-y-2 overflow-y-auto pr-1">
                {#if filteredSourceGroups.length === 0}
                  {#if sourcesLoading}
                    <div class="rounded border border-surface-800/70 bg-surface-950/60 px-3 py-2 text-micro text-surface-500">
                      Loading sources…
                    </div>
                  {:else if sourcesError}
                    <div class="rounded border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-micro text-rose-200">
                      {sourcesError}
                    </div>
                  {:else}
                    <div class="rounded border border-surface-800/70 bg-surface-950/60 px-3 py-2 text-micro text-surface-500">
                      No sources match this filter.
                    </div>
                  {/if}
                {:else}
                  {#if sourcesLoading}
                    <div class="rounded border border-surface-800/70 bg-surface-950/60 px-3 py-2 text-micro text-surface-500">
                      Refreshing sources…
                    </div>
                  {/if}
                  {#if sourcesError}
                    <div class="rounded border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-micro text-rose-200">
                      {sourcesError}
                    </div>
                  {/if}
                  {#each visibleSourceGroups as group (group.key)}
                    {@const groupSelectedCount = group.pipelines.reduce(
                      (count, pipeline) => count + pipeline.sources.filter((source) => selectedSourceSet.has(source.id)).length,
                      0
                    )}
                    {@const groupCalibrated = group.pipelines.some((pipeline) =>
                      pipeline.sources.some((source) => isSourceCalibrated(source, calibratedCameraIds))
                    )}
                    <div
                      class={`rounded border border-surface-800/70 ${
                        groupCalibrated ? 'bg-surface-950/60' : 'bg-surface-950/40 opacity-75'
                      }`}
                    >
                      <button
                        class={`flex w-full items-start justify-between gap-3 px-3 py-2 text-left ${
                          groupCalibrated ? 'text-surface-200' : 'text-surface-500'
                        }`}
                        type="button"
                        onclick={() => onToggleSourceGroup?.(group.key)}
                      >
                        <div class="min-w-0">
                          <div class="text-micro uppercase tracking-[0.3em] text-surface-500">
                            {group.kind ?? (group.key.startsWith('peer:') ? 'peer' : 'stream')}
                          </div>
                          <div class="mt-1 truncate text-xs text-surface-100">{group.label}</div>
                          <div class="mt-1 flex flex-wrap items-center gap-1.5">
                            <span class="rounded border border-surface-700/60 bg-surface-900/70 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-surface-400">
                              {group.pipelines.length} pipelines
                            </span>
                            <span class="rounded border border-primary-500/35 bg-primary-500/10 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-primary-100">
                              {groupSelectedCount} selected
                            </span>
                            {#if !groupCalibrated}
                              <span class="inline-flex items-center gap-1 rounded border border-amber-500/40 bg-amber-500/10 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-amber-200">
                                <span class="inline-flex h-3.5 w-3.5 items-center justify-center rounded-full border border-amber-500/40 bg-amber-500/20 text-[0.62rem]">!</span>
                                Uncalibrated
                              </span>
                            {/if}
                          </div>
                        </div>
                        <span class="pt-1 text-[0.7rem] text-surface-400">
                          {openSourceGroups.includes(group.key) ? '−' : '+'}
                        </span>
                      </button>

                      {#if openSourceGroups.includes(group.key)}
                        <div class="border-t border-surface-800/70 px-3 py-3">
                          <div class="space-y-2">
                            {#each group.pipelines as pipeline (pipeline.key)}
                              <div class="rounded border border-surface-800/70 bg-surface-950/60 px-3 py-3">
                                <div>
                                  <div>
                                    <div class="text-micro uppercase tracking-[0.3em] text-surface-500">Pipeline</div>
                                    <div class="mt-1 text-xs text-surface-100">{pipeline.label}</div>
                                  </div>
                                </div>

                                <div class="mt-2 space-y-1.5">
                                  {#each pipeline.sources as source (source.id)}
                                    {@const sourceCalibrated = isSourceCalibrated(source, calibratedCameraIds)}
                                    {@const sourceSelected = selectedSourceSet.has(source.id)}
                                    {@const row = sourceStatusById.get(source.id) ?? null}
                                    {@const sharedProfiles = sourceSharedProfiles(source.id)}
                                    <label
                                      class={`grid gap-1.5 rounded border px-2 py-2 text-micro ${
                                        sourceCardTone(source, row)
                                      }`}
                                    >
                                      <div class="flex items-start justify-between gap-2">
                                        <div class="min-w-0">
                                          <div class="truncate text-surface-100">{source.outputKey}</div>
                                          <div class="truncate text-micro-tight text-surface-500">
                                            {source.streamLabel || source.streamId} · {dataTypeLabel(source)}
                                          </div>
                                          {#if !sourceCalibrated || sharedProfiles.length > 0}
                                            <div class="mt-1 flex flex-wrap items-center gap-1">
                                              {#if !sourceCalibrated}
                                                <span class="inline-flex items-center gap-1 rounded border border-amber-500/40 bg-amber-500/10 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-amber-200">
                                                  <span class="inline-flex h-3.5 w-3.5 items-center justify-center rounded-full border border-amber-500/40 bg-amber-500/20 text-[0.62rem]">!</span>
                                                  Uncalibrated
                                                </span>
                                              {/if}
                                              {#each sharedProfiles as profileName (profileName)}
                                                <span class="inline-flex items-center rounded border border-surface-700/70 bg-surface-900/70 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-surface-300">
                                                  {profileName}
                                                </span>
                                              {/each}
                                            </div>
                                          {/if}
                                        </div>
                                        <input
                                          type="checkbox"
                                          checked={sourceSelected}
                                          disabled={localizationConfigLoading}
                                          onchange={(event) =>
                                            onToggleSource?.(source.id, (event.currentTarget as HTMLInputElement).checked)}
                                        />
                                      </div>
                                      <div class="flex flex-wrap items-center gap-1.5 text-micro-tight uppercase tracking-[0.22em] text-surface-500">
                                        {#if row}
                                          {#if isDetectionSource(source)}
                                            <span class="rounded border border-surface-700/70 bg-surface-900/60 px-1.5 py-0.5 text-surface-200">
                                              {row.detections} det
                                            </span>
                                          {/if}
                                          <span class="rounded border border-surface-700/70 bg-surface-900/60 px-1.5 py-0.5 text-surface-200">
                                            poll {row.pollMs.toFixed(1)}ms
                                          </span>
                                          {#if row.graphMs != null}
                                            <span class="rounded border border-surface-700/70 bg-surface-900/60 px-1.5 py-0.5 text-surface-200">
                                              graph {row.graphMs.toFixed(2)}ms
                                            </span>
                                          {/if}
                                          {#if row.tagSize != null}
                                            <span class="rounded border border-surface-700/70 bg-surface-900/60 px-1.5 py-0.5">
                                              tag {row.tagSize.toFixed(4)}m
                                            </span>
                                          {/if}
                                        {:else}
                                          <span class="rounded border border-surface-700/70 bg-surface-900/60 px-1.5 py-0.5">
                                            no sample yet
                                          </span>
                                        {/if}
                                      </div>
                                      {#if row?.error || row?.metricsError}
                                        <p class="text-micro text-rose-200">{row?.error ?? row?.metricsError}</p>
                                      {/if}
                                    </label>
                                  {/each}
                                </div>
                              </div>
                            {/each}
                          </div>
                        </div>
                      {/if}
                    </div>
                  {/each}
                {/if}
              </div>
            </section>
          </div>
          {/if}

          {#if setupTab !== 'sources'}
          <div class="h-full min-h-0 space-y-4 overflow-y-auto pr-1">
          {#if setupTab === 'solver'}
          <section class="rounded border border-surface-800/70 bg-surface-900/40 p-3">
            <div class="flex items-center justify-between">
              <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Solver + sources</p>
              <span class="text-micro-tight text-surface-500">
                {solvePoseSpaces.length} solving · {derivedPoseSpaces.length} derived
              </span>
            </div>

            <div class="mt-3 grid gap-3">
              <div class="rounded border border-surface-800/70 bg-surface-950/60 px-3 py-3">
                <div class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">Solver</div>
                {#if solvers.length > 0}
                  <div class="mt-2 space-y-2">
                    <select
                      class="w-full rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs uppercase tracking-[0.3em] text-surface-200 focus:border-primary-400 focus:outline-none"
                      bind:value={activeSolverId}
                      onchange={(event) => onSetActiveSolverId?.((event.currentTarget as HTMLSelectElement).value)}
                      disabled={localizationConfigLoading}
                    >
                      {#each solvers as solver (solver.id)}
                        <option value={solver.id}>{solver.name}</option>
                      {/each}
                    </select>
                    <div class="flex flex-wrap items-center gap-2">
                      <button
                        class="rounded-md border border-surface-700/70 bg-surface-900/70 px-3 py-2 text-micro uppercase tracking-[0.3em] text-surface-200 transition hover:border-surface-500 hover:text-white disabled:cursor-not-allowed disabled:border-surface-800/60 disabled:text-surface-600"
                        type="button"
                        onclick={onAddSolver}
                        disabled={localizationConfigLoading}
                      >
                        New
                      </button>
                      <button
                        class="rounded-md border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-micro uppercase tracking-[0.3em] text-rose-100 transition hover:border-rose-400 hover:text-white disabled:cursor-not-allowed disabled:border-surface-800/60 disabled:text-surface-600 disabled:bg-surface-900/60"
                        type="button"
                        onclick={onRemoveActiveSolver}
                        disabled={localizationConfigLoading || !canRemoveSolver}
                      >
                        Remove
                      </button>
                    </div>
                    <input
                      class="w-full rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs text-surface-100 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
                      placeholder="Solver name"
                      bind:value={solverNameInput}
                      onchange={onCommitSolverName}
                      disabled={localizationConfigLoading}
                    />
                    {#if activeSolverMode}
                      <select
                        class="w-full rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs uppercase tracking-[0.3em] text-surface-200 focus:border-primary-400 focus:outline-none"
                        value={activeSolverMode}
                        onchange={(event) => onSetSolverMode?.((event.currentTarget as HTMLSelectElement).value as LocalizationSolverMode)}
                        disabled={localizationConfigLoading}
                      >
                        {#each visibleSolverModes as mode (mode)}
                          <option value={mode}>
                            {solverModeDisplayLabel(mode)}
                            {#if !supportedSolverModeSet.has(mode)} (unsupported by backend){/if}
                          </option>
                        {/each}
                      </select>
                    {/if}
                    <div class="rounded border border-surface-800/70 bg-surface-950/50 px-3 py-3">
                      <div class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">Temporal override</div>
                      <label class="mt-2 flex items-center justify-between gap-3 text-micro text-surface-300">
                        <span class="uppercase tracking-[0.3em]">Use solver override</span>
                        <input
                          type="checkbox"
                          checked={solverHasTemporalOverride}
                          disabled={localizationConfigLoading}
                          onchange={(event) =>
                            onSetSolverTemporalOverrideEnabled?.((event.currentTarget as HTMLInputElement).checked)}
                        />
                      </label>
                      {#if solverHasTemporalOverride}
                        <label class="mt-2 flex items-center justify-between gap-3 text-micro text-surface-300">
                          <span class="uppercase tracking-[0.3em]">Enable smoothing</span>
                          <input
                            type="checkbox"
                            checked={activeSolverTemporalOverride?.enabled ?? false}
                            disabled={localizationConfigLoading}
                            onchange={(event) =>
                              onSetSolverTemporalEnabled?.((event.currentTarget as HTMLInputElement).checked)}
                          />
                        </label>
                        <div class="mt-2 grid gap-2 text-micro">
                          <label class="grid gap-1">
                            <span class="uppercase tracking-[0.3em] text-surface-500">Single-tag translation alpha</span>
                            <input
                              type="number"
                              step="0.01"
                              value={activeSolverTemporalOverride?.singleTagTranslationAlpha ?? 0}
                              disabled={localizationConfigLoading}
                              onchange={(event) => onSetSolverTemporalNumeric?.('singleTagTranslationAlpha', (event.currentTarget as HTMLInputElement).value)}
                              class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                            />
                          </label>
                          <label class="grid gap-1">
                            <span class="uppercase tracking-[0.3em] text-surface-500">Single-tag rotation alpha</span>
                            <input
                              type="number"
                              step="0.01"
                              value={activeSolverTemporalOverride?.singleTagRotationAlpha ?? 0}
                              disabled={localizationConfigLoading}
                              onchange={(event) => onSetSolverTemporalNumeric?.('singleTagRotationAlpha', (event.currentTarget as HTMLInputElement).value)}
                              class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                            />
                          </label>
                          <label class="grid gap-1">
                            <span class="uppercase tracking-[0.3em] text-surface-500">Multi-tag translation alpha</span>
                            <input
                              type="number"
                              step="0.01"
                              value={activeSolverTemporalOverride?.multiTagTranslationAlpha ?? 0}
                              disabled={localizationConfigLoading}
                              onchange={(event) => onSetSolverTemporalNumeric?.('multiTagTranslationAlpha', (event.currentTarget as HTMLInputElement).value)}
                              class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                            />
                          </label>
                          <label class="grid gap-1">
                            <span class="uppercase tracking-[0.3em] text-surface-500">Multi-tag rotation alpha</span>
                            <input
                              type="number"
                              step="0.01"
                              value={activeSolverTemporalOverride?.multiTagRotationAlpha ?? 0}
                              disabled={localizationConfigLoading}
                              onchange={(event) => onSetSolverTemporalNumeric?.('multiTagRotationAlpha', (event.currentTarget as HTMLInputElement).value)}
                              class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                            />
                          </label>
                          <label class="grid gap-1">
                            <span class="uppercase tracking-[0.3em] text-surface-500">Max jump translation (m)</span>
                            <input
                              type="number"
                              step="0.01"
                              value={activeSolverTemporalOverride?.maxTranslationJumpM ?? 0}
                              disabled={localizationConfigLoading}
                              onchange={(event) => onSetSolverTemporalNumeric?.('maxTranslationJumpM', (event.currentTarget as HTMLInputElement).value)}
                              class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                            />
                          </label>
                          <label class="grid gap-1">
                            <span class="uppercase tracking-[0.3em] text-surface-500">Max jump rotation (deg)</span>
                            <input
                              type="number"
                              step="0.1"
                              value={activeSolverTemporalOverride?.maxRotationJumpDeg ?? 0}
                              disabled={localizationConfigLoading}
                              onchange={(event) => onSetSolverTemporalNumeric?.('maxRotationJumpDeg', (event.currentTarget as HTMLInputElement).value)}
                              class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                            />
                          </label>
                          <label class="grid gap-1">
                            <span class="uppercase tracking-[0.3em] text-surface-500">Reanchor reject window (ms)</span>
                            <input
                              type="number"
                              step="10"
                              value={activeSolverTemporalOverride?.reanchorRejectWindowMs ?? 0}
                              disabled={localizationConfigLoading}
                              onchange={(event) => onSetSolverTemporalNumeric?.('reanchorRejectWindowMs', (event.currentTarget as HTMLInputElement).value)}
                              class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                            />
                          </label>
                        </div>
                      {:else}
                        <p class="mt-2 text-micro text-surface-500">
                          Using profile temporal settings (single-tag alpha {activeSolverTemporalEffective.singleTagTranslationAlpha.toFixed(2)}).
                        </p>
                      {/if}
                    </div>
                  </div>
                {:else}
                  <p class="mt-2 text-micro text-surface-500">No solver configured for this profile.</p>
                {/if}
              </div>

              <div class="rounded border border-surface-800/70 bg-surface-950/60 px-3 py-3">
                <div class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">Sources</div>
                <label class="mt-2 flex items-center justify-between gap-3 text-micro text-surface-300">
                  <span class="uppercase tracking-[0.3em]">Use all input sources</span>
                  <input
                    type="checkbox"
                    checked={solverUsesAllSources}
                    disabled={localizationConfigLoading}
                    onchange={(event) => onSetActiveSolverUseAllSources?.((event.currentTarget as HTMLInputElement).checked)}
                  />
                </label>
                <div class="mt-2 grid gap-2 sm:grid-cols-3">
                  <div class="rounded border border-surface-800/70 bg-surface-950/50 px-2.5 py-2">
                    <div class="text-micro-tight uppercase tracking-[0.28em] text-surface-500">Solver mode</div>
                    <div class="mt-1 text-micro text-surface-100">{solverModeLabel}</div>
                  </div>
                  <div class="rounded border border-surface-800/70 bg-surface-950/50 px-2.5 py-2">
                    <div class="text-micro-tight uppercase tracking-[0.28em] text-surface-500">Groups</div>
                    <div class="mt-1 text-micro text-surface-100">{solverGroupSummaries.length}</div>
                  </div>
                  <div class="rounded border border-surface-800/70 bg-surface-950/50 px-2.5 py-2">
                    <div class="text-micro-tight uppercase tracking-[0.28em] text-surface-500">Source coverage</div>
                    <div class="mt-1 text-micro text-surface-100">{solverSourceCount}/{selectedSourceCount} in solver</div>
                  </div>
                </div>
                {#if selectedSourceGroups.length > 0}
                  <div class="mt-2 flex items-center justify-between gap-2 rounded border border-surface-800/70 bg-surface-950/50 px-2 py-1.5">
                    <p class="text-micro-tight uppercase tracking-[0.26em] text-surface-500">Group focus</p>
                    <button
                      class={`rounded border px-2 py-1 text-micro-tight uppercase tracking-[0.22em] ${
                        activeSolverGroupTab === 'all'
                          ? 'border-primary-500/40 bg-primary-500/15 text-primary-100'
                          : 'border-surface-700/70 bg-surface-900/70 text-surface-300 hover:border-surface-500 hover:text-surface-100'
                      }`}
                      type="button"
                      onclick={() => (activeSolverGroupTab = 'all')}
                    >
                      All groups
                    </button>
                  </div>
                  <div class="mt-2 grid gap-2 sm:grid-cols-2">
                    {#each solverGroupSummaries as groupSummary (groupSummary.key)}
                      <button
                        class={`rounded border px-2.5 py-2 text-left transition ${
                          activeSolverGroupTab === groupSummary.key
                            ? 'border-primary-500/45 bg-primary-500/12 text-primary-100'
                            : 'border-surface-800/70 bg-surface-950/55 text-surface-200 hover:border-surface-600/80'
                        }`}
                        type="button"
                        onclick={() => (activeSolverGroupTab = groupSummary.key)}
                      >
                        <div class="text-micro-tight uppercase tracking-[0.26em] text-surface-500">{groupSummary.kind}</div>
                        <div class="mt-1 truncate text-micro text-surface-100">{groupSummary.label}</div>
                        <div class="mt-1 text-micro-tight uppercase tracking-[0.22em] text-surface-400">
                          {groupSummary.enabled}/{groupSummary.total} in solver
                        </div>
                        {#if groupSummary.uncalibrated > 0}
                          <div class="mt-1 text-micro-tight uppercase tracking-[0.22em] text-amber-200">
                            ! {groupSummary.uncalibrated} uncalibrated
                          </div>
                        {/if}
                      </button>
                    {/each}
                  </div>
                {/if}
                <div class={`mt-2 grid gap-2 ${solverUsesAllSources ? 'opacity-60' : ''}`}>
                  {#if selectedSourceGroups.length === 0}
                    <div class="text-micro text-surface-500">No sources selected yet.</div>
                  {:else}
                    {#each visibleSolverGroups as group (group.key)}
                      {@const groupSolverCount = group.pipelines.reduce(
                        (count, pipeline) => count + pipeline.sources.filter((source) => solverSourceSet.has(source.id)).length,
                        0
                      )}
                      {@const groupTotal = group.pipelines.reduce((count, pipeline) => count + pipeline.sources.length, 0)}
                      <div class="rounded border border-surface-800/70 bg-surface-950/55 px-2.5 py-2.5">
                        <div class="flex flex-wrap items-center justify-between gap-2">
                          <div>
                            <div class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">
                              {group.kind ?? (group.key.startsWith('peer:') ? 'peer' : 'stream')}
                            </div>
                            <div class="text-xs text-surface-100">{group.label}</div>
                          </div>
                          <span class="rounded border border-surface-700/70 bg-surface-900/70 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-surface-300">
                            {groupSolverCount}/{groupTotal} in solver
                          </span>
                        </div>
                        <div class="mt-2 space-y-2">
                          {#each group.pipelines as pipeline (pipeline.key)}
                            <div class="rounded border border-surface-800/70 bg-surface-950/60 px-2 py-2">
                              <div class="text-micro-tight uppercase tracking-[0.28em] text-surface-500">{pipeline.label}</div>
                              <div class="mt-2 space-y-1.5">
                                {#each pipeline.sources as source (source.id)}
                                  {@const checked = solverSourceSet.has(source.id)}
                                  {@const sourceCalibrated = isSourceCalibrated(source, calibratedCameraIds)}
                                  {@const sourceWeight = sourceWeightsById[source.id] ?? 1}
                                  {@const sharedProfiles = sourceSharedProfiles(source.id)}
                                  <div class="grid gap-1.5 rounded border border-surface-800/70 bg-surface-950/70 px-2 py-2">
                                    <div class="flex items-start justify-between gap-2">
                                      <div class="min-w-0">
                                        <div class="truncate text-micro text-surface-100">
                                          {source.streamLabel || source.streamId} · {source.outputKey}
                                        </div>
                                        {#if !sourceCalibrated || sharedProfiles.length > 0}
                                          <div class="mt-1 flex flex-wrap items-center gap-1">
                                            {#if !sourceCalibrated}
                                              <span class="inline-flex items-center gap-1 rounded border border-amber-500/40 bg-amber-500/10 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-amber-200">
                                                <span class="inline-flex h-3.5 w-3.5 items-center justify-center rounded-full border border-amber-500/40 bg-amber-500/20 text-[0.62rem]">!</span>
                                                Uncalibrated
                                              </span>
                                            {/if}
                                            {#each sharedProfiles as profileName (profileName)}
                                              <span class="inline-flex items-center rounded border border-surface-700/70 bg-surface-900/70 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-surface-300">
                                                {profileName}
                                              </span>
                                            {/each}
                                          </div>
                                        {/if}
                                      </div>
                                      <input
                                        type="checkbox"
                                        checked={checked}
                                        disabled={localizationConfigLoading || solverUsesAllSources}
                                        onchange={(event) =>
                                          onToggleActiveSolverSource?.(source.id, (event.currentTarget as HTMLInputElement).checked)}
                                      />
                                    </div>
                                    <label class="grid gap-1">
                                      <span class="text-micro-tight uppercase tracking-[0.26em] text-surface-500">
                                        Merge weight
                                      </span>
                                      <input
                                        type="number"
                                        step="0.01"
                                        value={sourceWeight}
                                        class="w-full rounded border border-surface-800 bg-surface-950/80 px-2 py-1 text-micro text-surface-100 focus:border-primary-400 focus:outline-none"
                                        disabled={localizationConfigLoading}
                                        onchange={(event) =>
                                          onSetSourceWeight?.(source.id, (event.currentTarget as HTMLInputElement).value)}
                                      />
                                    </label>
                                  </div>
                                {/each}
                              </div>
                            </div>
                          {/each}
                        </div>
                      </div>
                    {/each}
                  {/if}
                </div>
                <div class="mt-2 text-micro text-surface-500">
                  Merge control: source weights scale each input contribution; set weight to 0 to ignore a source.
                </div>
                <div class="mt-1 text-micro text-surface-500">
                  Solo solve: uncheck "Use all input sources", then keep one source enabled for this solver.
                </div>
              </div>
            </div>
            <div class="mt-3 rounded border border-surface-800/70 bg-surface-900/40 px-3 py-2 text-micro text-surface-400">
              <div class="text-micro-tight uppercase tracking-[0.3em] text-surface-500">Target spaces</div>
              <div class="mt-1 flex flex-wrap gap-2">
                {#each solvePoseSpaces as space (space)}
                  <span class="rounded border border-surface-800/70 bg-surface-900/70 px-2 py-1 uppercase tracking-[0.3em] text-surface-200">
                    {poseSpaceLabel(space)}
                  </span>
                {/each}
              </div>
              {#if derivedPoseSpaces.length > 0}
                <div class="mt-2 text-micro-tight uppercase tracking-[0.3em] text-surface-500">Derived outputs</div>
                <div class="mt-1 flex flex-wrap gap-2">
                  {#each derivedPoseSpaces as space (space)}
                    <span class="rounded border border-surface-800/70 bg-surface-900/70 px-2 py-1 uppercase tracking-[0.3em] text-surface-200">
                      {poseSpaceLabel(space)}
                    </span>
                  {/each}
                </div>
              {/if}
              {#if !selectedFieldMapId}
                <p class="mt-2 text-micro text-surface-500">Add a field map to auto-enable field target spaces.</p>
              {:else if !calibrationReady}
                <p class="mt-2 text-micro text-amber-300">
                  Field space outputs are enrolled, but calibration is missing for {uncalibratedSourcesCount} source(s).
                </p>
              {/if}
            </div>
          </section>
          {/if}

          {#if setupTab === 'field'}
          <section class="rounded border border-surface-800/70 bg-surface-900/40 p-3">
            <div class="flex items-center justify-between">
              <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Calibration + field</p>
              <span class="text-micro-tight text-surface-500">{fieldMaps.length} maps</span>
            </div>
            <div class="mt-3 grid gap-3 lg:grid-cols-2">
              <div class="grid gap-3">
                <div class="grid gap-2">
                  <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Tag size</p>
                  <input
                    class="w-full rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs text-surface-100 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
                    placeholder="0.03175m or 1.25in"
                    bind:value={tagSizeInput}
                    onchange={onCommitTagSize}
                    disabled={!hasActiveProfile || localizationConfigLoading}
                  />
                  {#if tagSizeError}
                    <p class="text-micro text-rose-200">{tagSizeError}</p>
                  {/if}
                  <p class="text-micro text-surface-500">Required for pose solving; field map sizes are ignored.</p>
                </div>

                <div class="grid gap-2">
                  <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Excluded tag IDs</p>
                  <input
                    class="w-full rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs text-surface-100 placeholder:text-surface-600 focus:border-primary-400 focus:outline-none"
                    placeholder="e.g. 1, 2 5"
                    bind:value={excludedTagIdsInput}
                    onchange={onCommitExcludedTagIds}
                    disabled={!hasActiveProfile || localizationConfigLoading}
                  />
                  {#if excludedTagIdsError}
                    <p class="text-micro text-rose-200">{excludedTagIdsError}</p>
                  {/if}
                  <p class="text-micro text-surface-500">Comma/space-separated IDs to ignore during localization solve.</p>
                </div>

                <div class="grid gap-2">
                  <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Field origin</p>
                  <select
                    class="w-full rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs uppercase tracking-[0.3em] text-surface-200 focus:border-primary-400 focus:outline-none"
                    value={fieldOriginMode}
                    disabled={!hasActiveProfile || localizationConfigLoading}
                    onchange={(event) =>
                      onSetFieldOriginMode?.((event.currentTarget as HTMLSelectElement).value as LocalizationFieldOriginMode)}
                  >
                    <option value="blue">wpiblue</option>
                    <option value="red">wpired</option>
                    <option value="center">center</option>
                    <option value="custom">custom (from center)</option>
                  </select>
                  {#if fieldOriginMode === 'custom'}
                    <div class="grid gap-2 rounded border border-surface-800/70 bg-surface-950/50 px-3 py-3">
                      <label class="grid gap-1">
                        <span class="uppercase tracking-[0.3em] text-surface-500">Custom X (m)</span>
                        <input
                          type="number"
                          step="0.01"
                          value={fieldOriginCustom?.x ?? 0}
                          disabled={!hasActiveProfile || localizationConfigLoading}
                          onchange={(event) => onSetFieldOriginCustomNumeric?.('x', (event.currentTarget as HTMLInputElement).value)}
                          class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                        />
                      </label>
                      <label class="grid gap-1">
                        <span class="uppercase tracking-[0.3em] text-surface-500">Custom Z (m)</span>
                        <input
                          type="number"
                          step="0.01"
                          value={fieldOriginCustom?.z ?? 0}
                          disabled={!hasActiveProfile || localizationConfigLoading}
                          onchange={(event) => onSetFieldOriginCustomNumeric?.('z', (event.currentTarget as HTMLInputElement).value)}
                          class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                        />
                      </label>
                      <label class="grid gap-1">
                        <span class="uppercase tracking-[0.3em] text-surface-500">Custom yaw (deg)</span>
                        <input
                          type="number"
                          step="0.1"
                          value={fieldOriginCustom?.yawDeg ?? 0}
                          disabled={!hasActiveProfile || localizationConfigLoading}
                          onchange={(event) =>
                            onSetFieldOriginCustomNumeric?.('yawDeg', (event.currentTarget as HTMLInputElement).value)}
                          class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                        />
                      </label>
                    </div>
                  {/if}
                  <p class="text-micro text-surface-500">
                    Defines the reported field pose frame for this profile, relative to field center.
                  </p>
                </div>

                <div class="grid gap-2">
                  <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Ground snap</p>
                  <label
                    class={`flex items-center justify-between gap-3 rounded border px-3 py-2 text-micro ${
                      snapZToGround
                        ? 'border-primary-500/40 bg-primary-500/10 text-primary-100'
                        : 'border-surface-800/70 bg-surface-950/60 text-surface-300'
                    } ${!hasActiveProfile || localizationConfigLoading ? 'opacity-60' : ''}`}
                  >
                    <span class="uppercase tracking-[0.3em]">Snap Z to ground</span>
                    <input
                      type="checkbox"
                      checked={snapZToGround}
                      disabled={!hasActiveProfile || localizationConfigLoading}
                      onchange={(event) => onSetSnapZToGround?.((event.currentTarget as HTMLInputElement).checked)}
                    />
                  </label>
                  <label
                    class={`flex items-center justify-between gap-3 rounded border px-3 py-2 text-micro ${
                      snapRollToGround
                        ? 'border-primary-500/40 bg-primary-500/10 text-primary-100'
                        : 'border-surface-800/70 bg-surface-950/60 text-surface-300'
                    } ${!hasActiveProfile || localizationConfigLoading ? 'opacity-60' : ''}`}
                  >
                    <span class="uppercase tracking-[0.3em]">Snap roll to level</span>
                    <input
                      type="checkbox"
                      checked={snapRollToGround}
                      disabled={!hasActiveProfile || localizationConfigLoading}
                      onchange={(event) => onSetSnapRollToGround?.((event.currentTarget as HTMLInputElement).checked)}
                    />
                  </label>
                  <label
                    class={`flex items-center justify-between gap-3 rounded border px-3 py-2 text-micro ${
                      snapPitchToGround
                        ? 'border-primary-500/40 bg-primary-500/10 text-primary-100'
                        : 'border-surface-800/70 bg-surface-950/60 text-surface-300'
                    } ${!hasActiveProfile || localizationConfigLoading ? 'opacity-60' : ''}`}
                  >
                    <span class="uppercase tracking-[0.3em]">Snap pitch to level</span>
                    <input
                      type="checkbox"
                      checked={snapPitchToGround}
                      disabled={!hasActiveProfile || localizationConfigLoading}
                      onchange={(event) => onSetSnapPitchToGround?.((event.currentTarget as HTMLInputElement).checked)}
                    />
                  </label>
                  <p class="text-micro text-surface-500">
                    Constrains field-space height and/or tilt for more stable solves when tags are sparse.
                  </p>
                </div>

              </div>

              <div class="grid gap-2 rounded border border-surface-800/70 bg-surface-950/60 px-3 py-3">
                <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Field map</p>
                <select
                  class="w-full rounded border border-surface-800 bg-surface-950/70 px-3 py-2 text-xs text-surface-100 focus:border-primary-400 focus:outline-none"
                  bind:value={fieldMapSelection}
                  onchange={(event) => onSetFieldMapSelection?.((event.currentTarget as HTMLSelectElement).value)}
                  disabled={fieldMapsLoading || localizationConfigLoading}
                >
                  <option value="">No field map</option>
                  {#each fieldMaps as map (map.id)}
                    <option value={map.id}>{map.name}</option>
                  {/each}
                </select>
                <label
                  class={`inline-flex items-center justify-center rounded-md border px-3 py-2 text-micro-tight uppercase tracking-[0.3em] transition ${
                    mapUploadBusy || localizationConfigLoading
                      ? 'cursor-not-allowed border-surface-800/60 text-surface-600'
                      : 'border-surface-700/70 bg-surface-900/70 text-surface-200 hover:border-surface-500 hover:text-white'
                  }`}
                >
                  {mapUploadBusy ? 'Uploading…' : 'Upload map'}
                  <input
                    type="file"
                    class="sr-only"
                    onchange={(event) => {
                      const input = event.currentTarget as HTMLInputElement;
                      const file = input.files?.[0] ?? null;
                      if (!file) return;
                      onUploadMapFile?.(file);
                    }}
                    disabled={mapUploadBusy || localizationConfigLoading}
                  />
                </label>
                {#if fieldMapsError}
                  <p class="text-micro text-rose-200">{fieldMapsError}</p>
                {/if}
                {#if mapUploadError}
                  <p class="text-micro text-rose-200">{mapUploadError}</p>
                {/if}
              </div>
            </div>
          </section>
          {/if}

          {#if setupTab === 'advanced'}
          <section class="rounded border border-surface-800/70 bg-surface-900/40 p-3">
            <div class="flex items-center justify-between">
              <p class="text-micro-tight uppercase tracking-[0.35em] text-surface-500">Advanced solver tuning</p>
            </div>
            <div class="mt-3 space-y-3">
              <details class="localization-accordion rounded border border-surface-800/70 bg-surface-950/60">
                <summary class="flex cursor-pointer items-start gap-3 px-3 py-2.5 text-left transition hover:bg-surface-900/40">
                  <span class="accordion-chevron mt-[0.18rem] text-[0.7rem] text-surface-400" aria-hidden="true">▶</span>
                  <div class="min-w-0 flex-1">
                    <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-400">Temporal profile defaults</p>
                    <p class="mt-1 text-micro text-surface-500">Base smoothing values used unless a solver override is enabled.</p>
                  </div>
                  <span class="rounded border border-surface-700/60 bg-surface-900/70 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-surface-300">
                    7 knobs
                  </span>
                </summary>
                <div class="border-t border-surface-800/70 px-3 py-3">
                  <label class={`flex items-center justify-between gap-3 rounded border px-3 py-2 text-micro ${
                    profileTemporalStabilization.enabled
                      ? 'border-primary-500/40 bg-primary-500/10 text-primary-100'
                      : 'border-surface-800/70 bg-surface-950/60 text-surface-300'
                  } ${!hasActiveProfile || localizationConfigLoading ? 'opacity-60' : ''}`}>
                    <span class="uppercase tracking-[0.3em]">Enable smoothing</span>
                    <input
                      type="checkbox"
                      checked={profileTemporalStabilization.enabled}
                      disabled={!hasActiveProfile || localizationConfigLoading}
                      onchange={(event) => onSetProfileTemporalEnabled?.((event.currentTarget as HTMLInputElement).checked)}
                    />
                  </label>
                  <div class="mt-2 grid gap-2">
                    <label class="grid gap-1">
                      <span class="uppercase tracking-[0.3em] text-surface-500">Single-tag translation alpha</span>
                      <input
                        type="number"
                        step="0.01"
                        value={profileTemporalStabilization.singleTagTranslationAlpha}
                        disabled={!hasActiveProfile || localizationConfigLoading}
                        onchange={(event) => onSetProfileTemporalNumeric?.('singleTagTranslationAlpha', (event.currentTarget as HTMLInputElement).value)}
                        class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                      />
                    </label>
                    <label class="grid gap-1">
                      <span class="uppercase tracking-[0.3em] text-surface-500">Single-tag rotation alpha</span>
                      <input
                        type="number"
                        step="0.01"
                        value={profileTemporalStabilization.singleTagRotationAlpha}
                        disabled={!hasActiveProfile || localizationConfigLoading}
                        onchange={(event) => onSetProfileTemporalNumeric?.('singleTagRotationAlpha', (event.currentTarget as HTMLInputElement).value)}
                        class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                      />
                    </label>
                    <label class="grid gap-1">
                      <span class="uppercase tracking-[0.3em] text-surface-500">Multi-tag translation alpha</span>
                      <input
                        type="number"
                        step="0.01"
                        value={profileTemporalStabilization.multiTagTranslationAlpha}
                        disabled={!hasActiveProfile || localizationConfigLoading}
                        onchange={(event) => onSetProfileTemporalNumeric?.('multiTagTranslationAlpha', (event.currentTarget as HTMLInputElement).value)}
                        class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                      />
                    </label>
                    <label class="grid gap-1">
                      <span class="uppercase tracking-[0.3em] text-surface-500">Multi-tag rotation alpha</span>
                      <input
                        type="number"
                        step="0.01"
                        value={profileTemporalStabilization.multiTagRotationAlpha}
                        disabled={!hasActiveProfile || localizationConfigLoading}
                        onchange={(event) => onSetProfileTemporalNumeric?.('multiTagRotationAlpha', (event.currentTarget as HTMLInputElement).value)}
                        class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                      />
                    </label>
                    <label class="grid gap-1">
                      <span class="uppercase tracking-[0.3em] text-surface-500">Max jump translation (m)</span>
                      <input
                        type="number"
                        step="0.01"
                        value={profileTemporalStabilization.maxTranslationJumpM}
                        disabled={!hasActiveProfile || localizationConfigLoading}
                        onchange={(event) => onSetProfileTemporalNumeric?.('maxTranslationJumpM', (event.currentTarget as HTMLInputElement).value)}
                        class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                      />
                    </label>
                    <label class="grid gap-1">
                      <span class="uppercase tracking-[0.3em] text-surface-500">Max jump rotation (deg)</span>
                      <input
                        type="number"
                        step="0.1"
                        value={profileTemporalStabilization.maxRotationJumpDeg}
                        disabled={!hasActiveProfile || localizationConfigLoading}
                        onchange={(event) => onSetProfileTemporalNumeric?.('maxRotationJumpDeg', (event.currentTarget as HTMLInputElement).value)}
                        class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                      />
                    </label>
                    <label class="grid gap-1">
                      <span class="uppercase tracking-[0.3em] text-surface-500">Reanchor reject window (ms)</span>
                      <input
                        type="number"
                        step="10"
                        value={profileTemporalStabilization.reanchorRejectWindowMs}
                        disabled={!hasActiveProfile || localizationConfigLoading}
                        onchange={(event) => onSetProfileTemporalNumeric?.('reanchorRejectWindowMs', (event.currentTarget as HTMLInputElement).value)}
                        class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                      />
                    </label>
                  </div>
                </div>
              </details>

              {#each runtimeTuningGroups as group (group.id)}
                <details class="localization-accordion rounded border border-surface-800/70 bg-surface-950/60">
                  <summary class="flex cursor-pointer items-start gap-3 px-3 py-2.5 text-left transition hover:bg-surface-900/40">
                    <span class="accordion-chevron mt-[0.18rem] text-[0.7rem] text-surface-400" aria-hidden="true">▶</span>
                    <div class="min-w-0 flex-1">
                      <p class="text-micro-tight uppercase tracking-[0.3em] text-surface-400">{group.label}</p>
                      <p class="mt-1 text-micro text-surface-500">{group.description}</p>
                    </div>
                    <span class="rounded border border-surface-700/60 bg-surface-900/70 px-1.5 py-0.5 text-micro-tight uppercase tracking-[0.22em] text-surface-300">
                      {group.fields.length} knobs
                    </span>
                  </summary>
                  <div class="grid gap-2 border-t border-surface-800/70 px-3 py-3">
                    {#each group.fields as field (field.key)}
                      <label class="grid gap-1">
                        <span class="uppercase tracking-[0.3em] text-surface-500">{field.label}</span>
                        <input
                          type="number"
                          step={field.step}
                          value={activeSolverRuntimeTuning[field.key]}
                          disabled={localizationConfigLoading}
                          onchange={(event) =>
                            onSetSolverRuntimeTuningNumeric?.(field.key, (event.currentTarget as HTMLInputElement).value)}
                          class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none"
                        />
                      </label>
                    {/each}
                  </div>
                </details>
              {/each}
            </div>
          </section>
          {/if}

        </div>
          {/if}
      </div>
    </div>
    </aside>
  </div>
{/if}

<style>
  .localization-accordion > summary {
    list-style: none;
  }

  .localization-accordion > summary::-webkit-details-marker {
    display: none;
  }

  .localization-accordion .accordion-chevron {
    transform: rotate(0deg);
    transition: transform 150ms ease;
  }

  .localization-accordion[open] .accordion-chevron {
    transform: rotate(90deg);
  }
</style>
