<script lang="ts">
  import type {
    LocalizationPoseSpace,
    LocalizationSolverMode,
    LocalizationTemporalStabilizationConfig,
    LocalizationSolverConfig
  } from '$lib/features/localization/localizationConfig';
  import type { LocalizationPipelineSource } from '$lib/features/localization/pipelineSources';
  import type { SourceGroup } from './localizationConfigEditorTypes';

  type SolverGroupSummary = {
    key: string;
    label: string;
    kind: string;
    enabled: number;
    total: number;
    uncalibrated: number;
  };

  type Props = {
    solvePoseSpaces: LocalizationPoseSpace[];
    derivedPoseSpaces: LocalizationPoseSpace[];
    poseSpaceLabel?: (space: LocalizationPoseSpace) => string;
    solvers: LocalizationSolverConfig[];
    activeSolverId?: string;
    localizationConfigLoading?: boolean;
    canRemoveSolver?: boolean;
    solverNameInput?: string;
    activeSolverMode?: LocalizationSolverMode | null;
    visibleSolverModes: LocalizationSolverMode[];
    supportedSolverModeSet?: Set<string>;
    solverModeDisplayLabel?: (mode: LocalizationSolverMode) => string;
    onSetActiveSolverId?: (solverId: string) => void;
    onAddSolver?: () => void;
    onRemoveActiveSolver?: () => void;
    onCommitSolverName?: () => void;
    onSetSolverMode?: (mode: LocalizationSolverMode) => void;
    solverHasTemporalOverride?: boolean;
    activeSolverTemporalOverride?: LocalizationTemporalStabilizationConfig | null;
    activeSolverTemporalEffective: LocalizationTemporalStabilizationConfig;
    onSetSolverTemporalOverrideEnabled?: (enabled: boolean) => void;
    onSetSolverTemporalEnabled?: (enabled: boolean) => void;
    onSetSolverTemporalNumeric?: (field: string, value: string) => void;
    solverUsesAllSources?: boolean;
    onSetActiveSolverUseAllSources?: (enabled: boolean) => void;
    solverModeLabel: string;
    solverGroupSummaries: SolverGroupSummary[];
    solverSourceCount: number;
    selectedSourceCount: number;
    selectedSourceGroups: SourceGroup[];
    activeSolverGroupTab?: string;
    visibleSolverGroups: SourceGroup[];
    solverSourceSet: Set<string>;
    sourceWeightsById?: Record<string, number>;
    calibratedCameraIds?: Set<string>;
    isSourceCalibrated?: (source: LocalizationPipelineSource, calibrated: Set<string>) => boolean;
    sourceSharedProfiles?: (sourceId: string) => string[];
    onToggleActiveSolverSource?: (sourceId: string, enabled: boolean) => void;
    onSetSourceWeight?: (sourceId: string, value: string) => void;
    selectedFieldMapId?: string | null;
    calibrationReady?: boolean;
    uncalibratedSourcesCount?: number;
  };

  let {
    solvePoseSpaces,
    derivedPoseSpaces,
    poseSpaceLabel = (space) => space,
    solvers,
    activeSolverId = $bindable(''),
    localizationConfigLoading = false,
    canRemoveSolver = false,
    solverNameInput = $bindable(''),
    activeSolverMode = null,
    visibleSolverModes,
    supportedSolverModeSet = new Set<string>(),
    solverModeDisplayLabel = (mode) => mode,
    onSetActiveSolverId,
    onAddSolver,
    onRemoveActiveSolver,
    onCommitSolverName,
    onSetSolverMode,
    solverHasTemporalOverride = false,
    activeSolverTemporalOverride = null,
    activeSolverTemporalEffective,
    onSetSolverTemporalOverrideEnabled,
    onSetSolverTemporalEnabled,
    onSetSolverTemporalNumeric,
    solverUsesAllSources = true,
    onSetActiveSolverUseAllSources,
    solverModeLabel,
    solverGroupSummaries,
    solverSourceCount,
    selectedSourceCount,
    selectedSourceGroups,
    activeSolverGroupTab = $bindable('all'),
    visibleSolverGroups,
    solverSourceSet,
    sourceWeightsById = {},
    calibratedCameraIds = new Set<string>(),
    isSourceCalibrated = () => true,
    sourceSharedProfiles = () => [],
    onToggleActiveSolverSource,
    onSetSourceWeight,
    selectedFieldMapId = null,
    calibrationReady = false,
    uncalibratedSourcesCount = 0
  }: Props = $props();

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

  function handleSetActiveSolverId(event: Event) {
    const value = readSelectValue(event);
    if (value != null) onSetActiveSolverId?.(value);
  }

  function handleSetSolverMode(event: Event) {
    const value = readSelectValue(event);
    if (value && visibleSolverModes.includes(value as LocalizationSolverMode)) {
      onSetSolverMode?.(value as LocalizationSolverMode);
    }
  }

  function handleSetSolverTemporalOverrideEnabled(event: Event) {
    const checked = readInputChecked(event);
    if (checked != null) onSetSolverTemporalOverrideEnabled?.(checked);
  }

  function handleSetSolverTemporalEnabled(event: Event) {
    const checked = readInputChecked(event);
    if (checked != null) onSetSolverTemporalEnabled?.(checked);
  }

  function handleSetSolverTemporalNumeric(field: string, event: Event) {
    const value = readInputValue(event);
    if (value != null) onSetSolverTemporalNumeric?.(field, value);
  }

  function handleSetActiveSolverUseAllSources(event: Event) {
    const checked = readInputChecked(event);
    if (checked != null) onSetActiveSolverUseAllSources?.(checked);
  }

  function handleToggleActiveSolverSource(sourceId: string, event: Event) {
    const checked = readInputChecked(event);
    if (checked != null) onToggleActiveSolverSource?.(sourceId, checked);
  }

  function handleSetSourceWeight(sourceId: string, event: Event) {
    const value = readInputValue(event);
    if (value != null) onSetSourceWeight?.(sourceId, value);
  }
</script>

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
            onchange={handleSetActiveSolverId}
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
              onchange={handleSetSolverMode}
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
                onchange={handleSetSolverTemporalOverrideEnabled}
              />
            </label>
            {#if solverHasTemporalOverride}
              <label class="mt-2 flex items-center justify-between gap-3 text-micro text-surface-300">
                <span class="uppercase tracking-[0.3em]">Enable smoothing</span>
                <input
                  type="checkbox"
                  checked={activeSolverTemporalOverride?.enabled ?? false}
                  disabled={localizationConfigLoading}
                  onchange={handleSetSolverTemporalEnabled}
                />
              </label>
              <div class="mt-2 grid gap-2 text-micro">
                <label class="grid gap-1">
                  <span class="uppercase tracking-[0.3em] text-surface-500">Single-tag translation alpha</span>
                  <input type="number" step="0.01" value={activeSolverTemporalOverride?.singleTagTranslationAlpha ?? 0} disabled={localizationConfigLoading} onchange={(event) => handleSetSolverTemporalNumeric('singleTagTranslationAlpha', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
                </label>
                <label class="grid gap-1">
                  <span class="uppercase tracking-[0.3em] text-surface-500">Single-tag rotation alpha</span>
                  <input type="number" step="0.01" value={activeSolverTemporalOverride?.singleTagRotationAlpha ?? 0} disabled={localizationConfigLoading} onchange={(event) => handleSetSolverTemporalNumeric('singleTagRotationAlpha', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
                </label>
                <label class="grid gap-1">
                  <span class="uppercase tracking-[0.3em] text-surface-500">Multi-tag translation alpha</span>
                  <input type="number" step="0.01" value={activeSolverTemporalOverride?.multiTagTranslationAlpha ?? 0} disabled={localizationConfigLoading} onchange={(event) => handleSetSolverTemporalNumeric('multiTagTranslationAlpha', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
                </label>
                <label class="grid gap-1">
                  <span class="uppercase tracking-[0.3em] text-surface-500">Multi-tag rotation alpha</span>
                  <input type="number" step="0.01" value={activeSolverTemporalOverride?.multiTagRotationAlpha ?? 0} disabled={localizationConfigLoading} onchange={(event) => handleSetSolverTemporalNumeric('multiTagRotationAlpha', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
                </label>
                <label class="grid gap-1">
                  <span class="uppercase tracking-[0.3em] text-surface-500">Max jump translation (m)</span>
                  <input type="number" step="0.01" value={activeSolverTemporalOverride?.maxTranslationJumpM ?? 0} disabled={localizationConfigLoading} onchange={(event) => handleSetSolverTemporalNumeric('maxTranslationJumpM', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
                </label>
                <label class="grid gap-1">
                  <span class="uppercase tracking-[0.3em] text-surface-500">Max jump rotation (deg)</span>
                  <input type="number" step="0.1" value={activeSolverTemporalOverride?.maxRotationJumpDeg ?? 0} disabled={localizationConfigLoading} onchange={(event) => handleSetSolverTemporalNumeric('maxRotationJumpDeg', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
                </label>
                <label class="grid gap-1">
                  <span class="uppercase tracking-[0.3em] text-surface-500">Reanchor reject window (ms)</span>
                  <input type="number" step="10" value={activeSolverTemporalOverride?.reanchorRejectWindowMs ?? 0} disabled={localizationConfigLoading} onchange={(event) => handleSetSolverTemporalNumeric('reanchorRejectWindowMs', event)} class="w-full rounded border border-surface-800 bg-surface-950/70 px-2 py-1.5 text-surface-100 focus:border-primary-400 focus:outline-none" />
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
          onchange={handleSetActiveSolverUseAllSources}
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
                              onchange={(event) => handleToggleActiveSolverSource(source.id, event)}
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
                              onchange={(event) => handleSetSourceWeight(source.id, event)}
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
