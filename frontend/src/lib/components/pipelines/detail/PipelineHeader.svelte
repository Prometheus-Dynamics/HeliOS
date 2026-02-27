<script lang="ts">
  import PipelineOverview from './PipelineOverview.svelte';
  import PipelineActions from './PipelineActions.svelte';

  type StatusChipTone = 'default' | 'info' | 'success' | 'warning' | 'error';
  type AutosaveChipState = {
    value: string;
    tone: StatusChipTone;
    showRetry: boolean;
  };

  type Props = {
    profileNameDraft: string;
    profileAliasDraft: string;
    profileError: string | null;
    isDirty: boolean;
    savingState: 'idle' | 'saving' | 'error';
    autosaveChipState: AutosaveChipState;
    revisionChipValue: string;
    shortPipelineId: string;
    graphSearchQuery: string;
    normalizedGraphSearchQuery: string;
    metaChipBase: string;
    metaChipTones: Record<StatusChipTone, string>;
    actionChipBase: string;
    actionChipIconBase: string;
    hasPipelineWarnings: boolean;
    warningsPanelOpen: boolean;
    warningsCount: number;
    canOrganizeGraph: boolean;
    canShowEngineConfig: boolean;
    engineConfigOpen: boolean;
    syncOverlayEnabled: boolean;
    gpuOverlayEnabled: boolean;
    heatmapEnabled: boolean;
    metricsInspectorActive: boolean;
    onOpenProfiler: () => void;
    onSubmitProfile: () => void;
    onClearProfileError: () => void;
    onRetrySave: () => void;
    onClearSearch: () => void;
    onToggleWarnings: () => void;
    onOrganize: () => void;
    onAssign: () => void;
    onExport: () => void;
    onToggleSyncOverlay: () => void;
    onToggleGpuOverlay: () => void;
    onToggleHeatmap: () => void;
    onToggleEngineConfig: () => void;
  };

  let {
    profileNameDraft = $bindable(''),
    profileAliasDraft = $bindable(''),
    profileError,
    isDirty,
    savingState,
    autosaveChipState,
    revisionChipValue,
    shortPipelineId,
    graphSearchQuery = $bindable(''),
    normalizedGraphSearchQuery,
    metaChipBase,
    metaChipTones,
    actionChipBase,
    actionChipIconBase,
    hasPipelineWarnings,
    warningsPanelOpen,
    warningsCount,
    canOrganizeGraph,
    canShowEngineConfig,
    engineConfigOpen,
    syncOverlayEnabled,
    gpuOverlayEnabled,
    heatmapEnabled,
    metricsInspectorActive,
    onOpenProfiler,
    onSubmitProfile,
    onClearProfileError,
    onRetrySave,
    onClearSearch,
    onToggleWarnings,
    onOrganize,
    onAssign,
    onExport,
    onToggleSyncOverlay,
    onToggleGpuOverlay,
    onToggleHeatmap,
    onToggleEngineConfig
  }: Props = $props();
</script>

<header class="flex flex-col gap-3 rounded border border-surface-800/80 bg-surface-950/80 px-4 py-3 shadow-lg shadow-black/20">
  <div class="flex flex-wrap items-start gap-3">
    <div class="flex min-w-0 flex-1 flex-wrap gap-3">
      <label class="flex min-w-[200px] flex-1 flex-col gap-1 text-[0.56rem] uppercase tracking-[0.26em] text-surface-500">
        <span>Name</span>
        <input
          class="input h-9 text-base font-semibold text-white"
          type="text"
          bind:value={profileNameDraft}
          oninput={onClearProfileError}
          onkeydown={(event) => {
            if (event.key === 'Enter' && !event.shiftKey) {
              event.preventDefault();
              onSubmitProfile();
            }
          }}
          onblur={onSubmitProfile}
          placeholder="Pipeline name"
        />
      </label>
      <label class="flex min-w-[200px] flex-1 flex-col gap-1 text-[0.56rem] uppercase tracking-[0.26em] text-surface-500">
        <span>Alias</span>
        <input
          class="input h-9 text-base font-semibold text-white"
          type="text"
          bind:value={profileAliasDraft}
          oninput={onClearProfileError}
          onkeydown={(event) => {
            if (event.key === 'Enter' && !event.shiftKey) {
              event.preventDefault();
              onSubmitProfile();
            }
          }}
          onblur={onSubmitProfile}
          placeholder="Optional alias"
        />
      </label>
    </div>
    <div class="flex flex-wrap items-center gap-2 text-micro-tight uppercase tracking-[0.28em] text-surface-500 lg:ml-auto">
      {#if isDirty}
        <span class="rounded border border-amber-500/40 bg-amber-500/10 px-3 py-[3px] text-micro-tight uppercase tracking-[0.28em] text-amber-200">
          Unsaved
        </span>
      {/if}
      {#if savingState === 'saving'}
        <span class="rounded border border-primary-500/40 bg-primary-500/10 px-3 py-[3px] text-micro-tight uppercase tracking-[0.28em] text-primary-200">
          Saving…
        </span>
      {/if}
      {#if savingState === 'error'}
        <span class="rounded border border-error-500/40 bg-error-500/5 px-3 py-[3px] text-micro-tight uppercase tracking-[0.28em] text-error-200">
          Save failed
        </span>
      {/if}
    </div>
  </div>
  {#if profileError}
    <p class="text-xs text-error-300">{profileError}</p>
  {/if}
  <div class="flex flex-wrap items-center justify-between gap-3 border-t border-surface-800/60 pt-3">
    <PipelineOverview
      {autosaveChipState}
      {revisionChipValue}
      {shortPipelineId}
      metaChipBase={metaChipBase}
      metaChipTones={metaChipTones}
      onRetrySave={onRetrySave}
    />
    <PipelineActions
      actionChipBase={actionChipBase}
      actionChipIconBase={actionChipIconBase}
      bind:graphSearchQuery={graphSearchQuery}
      {normalizedGraphSearchQuery}
      {hasPipelineWarnings}
      {warningsPanelOpen}
      warningsCount={warningsCount}
      {canOrganizeGraph}
      {savingState}
      {canShowEngineConfig}
      {engineConfigOpen}
      {syncOverlayEnabled}
      {gpuOverlayEnabled}
      {heatmapEnabled}
      {metricsInspectorActive}
      {onOpenProfiler}
      onToggleWarnings={onToggleWarnings}
      onOrganize={onOrganize}
      onAssign={onAssign}
      onExport={onExport}
      onToggleSyncOverlay={onToggleSyncOverlay}
      onToggleGpuOverlay={onToggleGpuOverlay}
      onToggleHeatmap={onToggleHeatmap}
      onToggleEngineConfig={onToggleEngineConfig}
      onClearSearch={onClearSearch}
    />
  </div>
</header>
