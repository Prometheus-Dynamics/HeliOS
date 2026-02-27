<script lang="ts">
  import { PipelineGraphEditor } from '$lib';
  import type { PipelineDetailContext } from '$lib/components/pipelines/types';
  import type { PipelineDiagnosticWarning, PipelineGraphPlan, PipelineNodeLayout, PipelineNodeSyncConfig, PipelineRegistryEntry } from '$lib/types/pipeline';
  import type { PipelineGraphHeatmap } from '$lib/components/flow/pipeline-graph/types';
  import PipelineEngineConfigPanel from '$lib/components/pipelines/detail/PipelineEngineConfigPanel.svelte';
  import PipelineWarningsPanel from '$lib/components/pipelines/detail/PipelineWarningsPanel.svelte';
  import PipelineStreams from '$lib/components/pipelines/detail/PipelineStreams.svelte';
  import SyncPolicyOverlay from '$lib/features/pipelines/workbench/SyncPolicyOverlay.svelte';
  import DaedalusEngineConfigPanel from '$lib/features/pipelines/workbench/DaedalusEngineConfigPanel.svelte';

  type Breadcrumb = {
    id: string;
    name: string;
    status?: 'embedded' | 'linked' | 'mismatch' | 'unresolved';
    targetId?: string | null;
  };

  type HeatmapStreamOption = { id: string; label: string; hasMetrics: boolean };

  type HeatmapViewMode = { id: string; label: string; description: string };

  type GpuOverlaySummary = {
    totalGpuNodes: number;
    sharedSegments: number;
    segments: Array<{ id: string; color: string; nodes: string[]; labels: string[]; shared?: boolean }>;
    ungroupedLabels: string[];
  };

  type Props = {
    context: PipelineDetailContext;
    graphPlan: PipelineGraphPlan;
    registryEntries: PipelineRegistryEntry[];
    graphEditor: unknown;
    graphViewportElement: HTMLDivElement | null;
    graphViewportHeight: number;
    normalizedGraphSearchQuery: string;
    graphHeatmap: PipelineGraphHeatmap | null;
    heatmapEnabled: boolean;
    gpuOverlayEnabled: boolean;
    gpuOverlaySegments: Array<{ id: number; nodes: string[] }> | null;
    runtimeWarnings: Record<string, { message: string; at: number | null }>;
    syncOverlayEnabled: boolean;
    warningsPanelOpen: boolean;
    pipelineWarnings: PipelineDiagnosticWarning[];
    canShowEngineConfig: boolean;
    engineConfigOpen: boolean;
    heatmapStreamId: string | null;
    heatmapStreamOptions: HeatmapStreamOption[];
    selectedHeatmapStreamLabel: string | null;
    heatmapViewModes: HeatmapViewMode[];
    heatmapViewMode: string;
    heatmapActiveFilterCount: number;
    heatmapFilterQuery: string;
    heatmapMinAverageInput: string;
    heatmapMinSamplesInput: string;
    heatmapExcludedCount: number;
    gpuOverlaySummary: GpuOverlaySummary;
    metricsUpdatedAt: number | null;
    metricsStatus: string;
    breadcrumbs: Breadcrumb[];
    onExitEmbedded: () => void;
    onCloseEngineConfig: () => void;
    onCloseWarnings: () => void;
    onFocusWarning: (warning: PipelineDiagnosticWarning) => void;
    onHeatmapStreamChange: (event: Event) => void;
    onHeatmapSelectorEnter: () => void;
    onHeatmapSelectorLeave: () => void;
    onSetHeatmapViewMode: (mode: string) => void;
    onClearHeatmapFilters: () => void;
    formatHeatDuration: (value: number | null | undefined) => string;
    formatTimestamp: (value: number | null | undefined) => string;
    onPlanChange: (plan: PipelineGraphPlan) => void;
    onGraphSelect: (payload: { nodeId: string | null; nodes: string[]; edge: any }) => void;
    onEnterEmbedded: (nodeId: string) => void;
    onGraphContext: (payload: {
      type: 'pane' | 'node' | 'palette' | 'port';
      position: { x: number; y: number };
      flowPosition: { x: number; y: number };
      nodeId?: string | null;
      port?: string | null;
      direction?: 'input' | 'output';
    }) => void;
    onGraphLayout: (layout: PipelineNodeLayout) => void;
    onRuntime: (payload: { nodeId: string; syncGroups: unknown[] }) => void;
    onSetSyncConfig: (payload: { nodeId: string; config: PipelineNodeSyncConfig | null }) => void;
    onSetDaedalusNodeRuntime: (payload: { nodeId: string; syncGroups: unknown[] }) => void;
  };

  let {
    context,
    graphPlan,
    registryEntries,
    graphEditor = $bindable(),
    graphViewportElement = $bindable(),
    graphViewportHeight,
    normalizedGraphSearchQuery,
    graphHeatmap,
    heatmapEnabled,
    gpuOverlayEnabled,
    gpuOverlaySegments,
    runtimeWarnings,
    syncOverlayEnabled,
    warningsPanelOpen,
    pipelineWarnings,
    canShowEngineConfig,
    engineConfigOpen,
    heatmapStreamId,
    heatmapStreamOptions,
    selectedHeatmapStreamLabel,
    heatmapViewModes,
    heatmapViewMode,
    heatmapActiveFilterCount,
    heatmapFilterQuery = $bindable(),
    heatmapMinAverageInput = $bindable(),
    heatmapMinSamplesInput = $bindable(),
    heatmapExcludedCount,
    gpuOverlaySummary,
    metricsUpdatedAt,
    metricsStatus,
    breadcrumbs,
    onExitEmbedded,
    onCloseEngineConfig,
    onCloseWarnings,
    onFocusWarning,
    onHeatmapStreamChange,
    onHeatmapSelectorEnter,
    onHeatmapSelectorLeave,
    onSetHeatmapViewMode,
    onClearHeatmapFilters,
    formatHeatDuration,
    formatTimestamp,
    onPlanChange,
    onGraphSelect,
    onEnterEmbedded,
    onGraphContext,
    onGraphLayout,
    onRuntime,
    onSetSyncConfig,
    onSetDaedalusNodeRuntime
  }: Props = $props();
</script>

<div class="flex flex-1 min-h-0 min-w-0 flex-col gap-4 xl:flex-row">
  <div class="relative flex min-h-0 w-full min-w-0 flex-1" bind:this={graphViewportElement}>
    <PipelineEngineConfigPanel
      open={canShowEngineConfig && engineConfigOpen}
      onClose={onCloseEngineConfig}
    >
      {#snippet body()}
        <DaedalusEngineConfigPanel
          plan={graphPlan}
          onChange={(plan) => onPlanChange(plan)}
          open={true}
          showHeader={false}
        />
      {/snippet}
    </PipelineEngineConfigPanel>
    <PipelineWarningsPanel
      open={warningsPanelOpen}
      warnings={pipelineWarnings}
      onClose={onCloseWarnings}
      onFocus={onFocusWarning}
    />
    {#if syncOverlayEnabled}
      <SyncPolicyOverlay
        context={context}
        onSetSyncConfig={onSetSyncConfig}
        onSetDaedalusNodeRuntime={onSetDaedalusNodeRuntime}
      />
    {/if}
    <PipelineStreams
      heatmapEnabled={heatmapEnabled}
      gpuOverlayEnabled={gpuOverlayEnabled}
      {heatmapStreamId}
      {heatmapStreamOptions}
      {selectedHeatmapStreamLabel}
      {heatmapViewModes}
      {heatmapViewMode}
      {heatmapActiveFilterCount}
      bind:heatmapFilterQuery={heatmapFilterQuery}
      bind:heatmapMinAverageInput={heatmapMinAverageInput}
      bind:heatmapMinSamplesInput={heatmapMinSamplesInput}
      {heatmapExcludedCount}
      graphHeatmap={graphHeatmap}
      {metricsUpdatedAt}
      {gpuOverlaySummary}
      onHeatmapStreamChange={onHeatmapStreamChange}
      onHeatmapSelectorEnter={onHeatmapSelectorEnter}
      onHeatmapSelectorLeave={onHeatmapSelectorLeave}
      onSetHeatmapViewMode={onSetHeatmapViewMode}
      onClearHeatmapFilters={onClearHeatmapFilters}
      {formatHeatDuration}
      {formatTimestamp}
    />
    <div class="relative w-full">
      {#if breadcrumbs.length > 0}
        <div class="pointer-events-none absolute left-3 top-3 z-10 flex max-w-[360px] flex-col gap-2">
          <button
            type="button"
            class="pointer-events-auto inline-flex items-center gap-2 rounded-md border border-surface-700/70 bg-surface-900/80 px-3 py-1 text-[0.7rem] font-semibold text-white shadow-lg shadow-black/30 transition hover:border-primary-400/70 hover:bg-primary-500/10 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary-300"
            onclick={onExitEmbedded}
          >
            ← Back
          </button>
          <div class="pointer-events-none rounded-md border border-surface-700/70 bg-surface-900/70 px-3 py-2 text-micro text-surface-200 shadow-lg shadow-black/20">
            <div class="flex flex-wrap items-center gap-1">
              {#each breadcrumbs as crumb, index (crumb.id)}
                <span class="flex items-center gap-1" title={crumb.targetId ?? ''}>
                  <span class="font-semibold text-white">{crumb.name}</span>
                  {#if crumb.status}
                    <span
                      class={`rounded-full border px-2 py-[1px] text-micro-tight uppercase tracking-[0.2em] ${
                        crumb.status === 'linked'
                          ? 'border-primary-500/60 text-primary-100'
                          : crumb.status === 'embedded'
                            ? 'border-surface-600/60 text-surface-200'
                            : crumb.status === 'mismatch'
                              ? 'border-amber-500/60 text-amber-100'
                              : 'border-error-500/60 text-error-100'
                      }`}
                    >
                      {crumb.status === 'embedded'
                        ? 'Inline'
                        : crumb.status === 'linked'
                          ? 'Linked'
                          : crumb.status === 'mismatch'
                            ? 'Mismatch'
                            : 'Missing'}
                    </span>
                  {/if}
                </span>
                {#if index < breadcrumbs.length - 1}
                  <span class="text-surface-600">›</span>
                {/if}
              {/each}
            </div>
          </div>
        </div>
      {/if}
      {#key context.pipeline?.id ?? 'graph'}
        <PipelineGraphEditor
          bind:this={graphEditor}
          plan={graphPlan}
          interactive={true}
          height={graphViewportHeight}
          {registryEntries}
          className="h-full w-full"
          metricsHeatmap={graphHeatmap}
          heatmapMode={heatmapEnabled}
          gpuOverlayMode={gpuOverlayEnabled}
          {gpuOverlaySegments}
          {runtimeWarnings}
          diagnostics={context.pipeline?.diagnostics ?? null}
          searchQuery={normalizedGraphSearchQuery}
          syncInspector={{ enabled: syncOverlayEnabled, focusNodeId: context.graphSelectionNodeId }}
          on:change={(event) => onPlanChange(event.detail.plan)}
          on:select={(event) => onGraphSelect(event.detail)}
          on:edit={(event) => onEnterEmbedded(event.detail.nodeId)}
          on:context={(event) => {
            const detail = event.detail as {
              type: 'pane' | 'node' | 'palette' | 'port';
              position: { x: number; y: number };
              flowPosition: { x: number; y: number };
              nodeId?: string | null;
              port?: string | null;
              direction?: 'input' | 'output';
            };
            onGraphContext({
              type: detail.type,
              position: detail.position,
              flowPosition: detail.flowPosition,
              nodeId: detail.nodeId ?? null,
              port: detail.type === 'port' ? detail.port ?? null : null,
              direction: detail.type === 'port' ? detail.direction : undefined
            });
          }}
          selectedEdgeId={context.graphSelectionEdgeId}
          on:layout={(event) => onGraphLayout(event.detail.nodes)}
          on:runtime={(event) => onRuntime(event.detail)}
        />
      {/key}
    </div>
    {#if heatmapEnabled}
      {#if !graphHeatmap && context.metricsStatus === 'connecting'}
        <p class="mt-3 text-micro text-surface-500">Connecting to metrics stream…</p>
      {/if}
    {/if}
  </div>
</div>
