<script lang="ts">
  import PipelineHeader from '$lib/components/pipelines/detail/PipelineHeader.svelte';
  import PipelineConstants from '$lib/components/pipelines/detail/PipelineConstants.svelte';
  import type { PipelineDetailContext } from '$lib/components/pipelines/types';

  type Props = {
    context: PipelineDetailContext;
    profileNameDraft: string;
    profileAliasDraft: string;
    profileError: string | null;
    autosaveChipState: { value: string; tone: 'default' | 'info' | 'success' | 'warning' | 'error'; showRetry: boolean };
    revisionChipValue: string;
    shortPipelineId: string;
    graphSearchQuery: string;
    normalizedGraphSearchQuery: string;
    metaChipBase: string;
    metaChipTones: Record<string, string>;
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
    context,
    profileNameDraft = $bindable(),
    profileAliasDraft = $bindable(),
    profileError,
    autosaveChipState,
    revisionChipValue,
    shortPipelineId,
    graphSearchQuery = $bindable(),
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

  const pipeline = $derived(context.pipeline);
</script>

{#if pipeline}
  <PipelineHeader
    bind:profileNameDraft={profileNameDraft}
    bind:profileAliasDraft={profileAliasDraft}
    {profileError}
    isDirty={context.dirty}
    savingState={context.savingState}
    {autosaveChipState}
    {revisionChipValue}
    {shortPipelineId}
    bind:graphSearchQuery={graphSearchQuery}
    {normalizedGraphSearchQuery}
    metaChipBase={metaChipBase}
    metaChipTones={metaChipTones}
    actionChipBase={actionChipBase}
    actionChipIconBase={actionChipIconBase}
    {hasPipelineWarnings}
    {warningsPanelOpen}
    {warningsCount}
    {canOrganizeGraph}
    {canShowEngineConfig}
    {engineConfigOpen}
    {syncOverlayEnabled}
    {gpuOverlayEnabled}
    {heatmapEnabled}
    {metricsInspectorActive}
    onOpenProfiler={onOpenProfiler}
    onSubmitProfile={onSubmitProfile}
    onClearProfileError={onClearProfileError}
    onRetrySave={onRetrySave}
    onClearSearch={onClearSearch}
    onToggleWarnings={onToggleWarnings}
    onOrganize={onOrganize}
    onAssign={onAssign}
    onExport={onExport}
    onToggleSyncOverlay={onToggleSyncOverlay}
    onToggleGpuOverlay={onToggleGpuOverlay}
    onToggleHeatmap={onToggleHeatmap}
    onToggleEngineConfig={onToggleEngineConfig}
  />
  <PipelineConstants error={pipeline.diagnostics?.error} />
{/if}
