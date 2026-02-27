<script lang="ts">
import { createEventDispatcher } from 'svelte';
import { toaster } from '$lib';
import { applyPaletteToGraphPlan, emptyPipelineGraphPlan } from '$lib/features/pipelines/graph';
import type { InspectorTabKey } from '$lib/features/pipelines/controller';
import type { StreamInfo } from '$lib/ts-bindings/http/client';
  import type { GraphEdgeSelection, GraphPoint, PipelineDetailContext, PipelineOutputEntry, PipelinePortEntry } from './types';
import type {
  ChannelPolicy,
  PipelineDataType,
  PipelineGraphPlan,
  PipelineInputQueueConfig,
  PipelineNodeLayout,
  PipelineNodeSyncConfig,
  PipelineNodeValue,
  PipelineConnectionStyle,
  PipelineOutputSinkConfig,
  PipelineRegistryEntry,
  PipelineStatus,
  PipelineTypeDescriptor,
  PipelineDiagnosticWarning,
  PipelineOverviewPipeline
} from '$lib/types/pipeline';
	import NodeInspector from '$lib/features/pipelines/workbench/inspector/NodeInspector.svelte';
	import ConnectionInspector from '$lib/features/pipelines/workbench/inspector/ConnectionInspector.svelte';
	import BoundaryInspector from '$lib/features/pipelines/workbench/inspector/BoundaryInspector.svelte';
	import MetricsInspector from '$lib/features/pipelines/workbench/inspector/MetricsInspector.svelte';
	import PipelineDetailHeaderSection from '$lib/components/pipelines/detail/PipelineDetailHeaderSection.svelte';
	import PipelineDetailGraphContainer from '$lib/components/pipelines/detail/PipelineDetailGraphContainer.svelte';
	import PipelineProfilerModal from '$lib/components/pipelines/PipelineProfilerModal.svelte';
	import { revisionDisplay } from '$lib/components/pipelines/detail/pipelineDetailMetricsUtils';
	import { resolveStreamLabel } from '$lib/utils/streamLabels';
  type PipelineBreadcrumb = {
    id: string;
    name: string;
    status?: 'embedded' | 'linked' | 'mismatch' | 'unresolved';
    targetId?: string | null;
  };

  type CaptureDeviceStatus = StreamInfo;

  const props = $props<{
    context?: PipelineDetailContext;
    typePalette?: Record<string, PipelineTypeDescriptor>;
    registryEntries?: PipelineRegistryEntry[];
    pipelineInputEntries?: PipelinePortEntry[];
    pipelineOutputEntries?: PipelineOutputEntry[];
    inspectorTab?: InspectorTabKey;
    captureDevices?: CaptureDeviceStatus[];
    plan?: PipelineGraphPlan | null;
    breadcrumbs?: PipelineBreadcrumb[];
    pipelines?: PipelineOverviewPipeline[];
  }>();

  const defaultDetailContext: PipelineDetailContext = {
    pipeline: null,
    dirty: false,
    savingState: 'idle',
    validation: null,
    pipelineInputs: [],
    pipelineOutputs: [],
    graphSelectionNodeId: null,
    graphSelectionEdgeId: null,
    detachBusyMap: {},
    metrics: null,
    metricsStatus: 'idle',
    metricsError: null,
    metricsUpdatedAt: null
  };

  let graphEditor = $state<any>(null);

const context = $derived.by<PipelineDetailContext>(() => props.context ?? defaultDetailContext);
const typePalette = $derived.by(() => props.typePalette ?? {});
const registryEntries = $derived.by(() => props.registryEntries ?? []);
const pipelines = $derived.by(() => props.pipelines ?? []);
const pipelineInputEntries = $derived.by(() => props.pipelineInputEntries ?? []);
const pipelineOutputEntries = $derived.by(() => props.pipelineOutputEntries ?? []);
const graphPlan = $derived.by<PipelineGraphPlan>(() => props.plan ?? context.pipeline?.graph ?? emptyPipelineGraphPlan());
const breadcrumbs = $derived.by(() => props.breadcrumbs ?? []);
let syncOverlayEnabled = $state(false);
	let graphSearchQuery = $state('');
	let engineConfigOpen = $state(false);
	const captureDevicesList = $derived.by<readonly CaptureDeviceStatus[]>(() => props.captureDevices ?? []);
	let profilerOpen = $state(false);
	let profilerStreamId = $state<string | null>(null);

	const isDaedalusPlan = $derived.by(() => Boolean(graphPlan?.format === 'daedalus' || graphPlan?.daedalus));
const allowedDaedalusTabs: InspectorTabKey[] = ['pipeline', 'node', 'boundary', 'run', 'metrics'];
const requestedInspectorTab = $derived.by<InspectorTabKey>(() => props.inspectorTab ?? 'pipeline');
const activeInspectorTab = $derived.by<InspectorTabKey>(() => {
  if (!isDaedalusPlan) return requestedInspectorTab;
  return allowedDaedalusTabs.includes(requestedInspectorTab) ? requestedInspectorTab : 'pipeline';
});
const metricsInspectorActive = $derived.by(() => activeInspectorTab === 'metrics');
const normalizedGraphSearchQuery = $derived.by(() => graphSearchQuery.trim());
	const canShowEngineConfig = $derived.by(() => graphPlan?.format === 'daedalus');

	type ProfilerStreamOption = { id: string; label: string };
	const streamUsesPipeline = (device: any, pipelineId: string): boolean => {
	  if (!device || !pipelineId) return false;
	  const manifest: any = device?.manifest ?? null;
	  if (!manifest) return false;
	  const norm = (value: unknown) => (typeof value === 'string' ? value.trim() : '');
	  const target = norm(pipelineId);
	  if (!target) return false;
	  if (norm(manifest?.active_pipeline_id) === target) return true;
	  if (norm(manifest?.pipeline_id) === target) return true;
	  const pipelines = manifest?.pipelines;
	  if (Array.isArray(pipelines)) {
	    for (const entry of pipelines) {
	      const id = norm(entry?.pipeline_id ?? entry?.pipelineId ?? entry?.id);
	      if (id === target) return true;
	    }
	  }
	  return false;
	};

	const profilerStreamOptions = $derived.by<ProfilerStreamOption[]>(() => {
	  const pipelineId = (context.pipeline as any)?.id ?? null;
	  if (typeof pipelineId !== 'string' || !pipelineId.trim()) return [];
	  const options: ProfilerStreamOption[] = [];
	  const seen = new Set<string>();
	  for (const device of captureDevicesList as any[]) {
	    const id = typeof device?.id === 'string' ? device.id.trim() : '';
	    if (!id || seen.has(id)) continue;
	    if (!streamUsesPipeline(device, pipelineId)) continue;
	    seen.add(id);
	    options.push({ id, label: resolveStreamLabel(device, id) });
	  }
	  return options;
	});

	$effect(() => {
	  if (!profilerOpen) return;
	  const options = profilerStreamOptions;
	  if (options.length === 0) {
	    profilerStreamId = null;
	    return;
	  }
	  if (profilerStreamId && options.some((o) => o.id === profilerStreamId)) return;
	  profilerStreamId = options[0]?.id ?? null;
	});

$effect(() => {
  if (!canShowEngineConfig && engineConfigOpen) {
    engineConfigOpen = false;
  }
});

$effect(() => {
  if (!context.pipeline) {
    syncOverlayEnabled = false;
  }
});

$effect(() => {
  if (!breadcrumbs.length) return;
  if (typeof graphEditor?.focusOnGraphCenter === 'function') {
    const handle = setTimeout(() => graphEditor?.focusOnGraphCenter(), 10);
    return () => clearTimeout(handle);
  }
});
  const ACTION_CHIP_BASE =
    'inline-flex items-center gap-1.5 rounded-md border border-surface-700/60 bg-surface-900/30 px-2.5 py-1.5 text-[0.7rem] font-semibold text-white transition hover:border-primary-400/70 hover:bg-primary-500/10 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary-300';
  const ACTION_CHIP_ICON_BASE =
    'flex h-6 w-6 items-center justify-center rounded-full bg-surface-800/70 text-[0.7rem] text-surface-100';
  type StatusChipTone = 'default' | 'info' | 'success' | 'warning' | 'error';
  const META_CHIP_BASE =
    'inline-flex items-center gap-1 rounded-full border px-2.5 py-[3px] text-micro-tight uppercase tracking-[0.25em]';
  const META_CHIP_TONES: Record<StatusChipTone, string> = {
    default: 'border-surface-700/70 bg-surface-900/30 text-surface-300',
    info: 'border-primary-500/60 bg-primary-500/10 text-primary-100',
    success: 'border-emerald-500/60 bg-emerald-500/10 text-emerald-100',
    warning: 'border-amber-500/60 bg-amber-500/10 text-amber-100',
    error: 'border-error-500/60 bg-error-500/5 text-error-100'
  };
  type AutosaveChipState = {
    value: string;
    tone: StatusChipTone;
    showRetry: boolean;
  };

  const dispatch = createEventDispatcher<{
    organize: void;
    assign: void;
    save: void;
    validate: void;
    clearValidation: void;
    export: { inlineExternals?: boolean };
    refreshMetrics: void;
    planChange: { plan: PipelineGraphPlan };
    graphSelect: { nodeId: string | null; nodes: string[]; edge: GraphEdgeSelection | unknown };
    enterEmbedded: { nodeId: string };
    openPipeline: { pipelineId: string };
    exitEmbedded: void;
    graphContext: {
      type: 'pane' | 'node' | 'palette' | 'port';
      position: GraphPoint;
      flowPosition: GraphPoint;
      nodeId?: string | null;
      port?: string | null;
      direction?: 'input' | 'output';
    };
    addPipelinePort: { direction: 'input' | 'output'; name: string; dataTypeKey: string };
    removePipelinePort: { direction: 'input' | 'output'; name: string };
    editPipelinePort: { direction: 'input' | 'output'; nodeId: string; name: string; oldName?: string; dataTypeKey: string };
    setPipelinePortValue: { direction: 'input' | 'output'; name: string; value: PipelineNodeValue | null };
    setNodeConstantValue: { nodeId: string; port: string; value: PipelineNodeValue | null };
    setNodeSyncConfig: { nodeId: string; config: PipelineNodeSyncConfig | null };
    setDaedalusNodeRuntime: { nodeId: string; syncGroups: unknown[] };
    graphLayout: { pipelineId: string; layout: PipelineNodeLayout };
    setConnectionPolicy: { connection: GraphEdgeSelection; policy: ChannelPolicy };
    setConnectionStyle: { connection: GraphEdgeSelection; style: PipelineConnectionStyle };
    setPipelinePortConfig:
      | { direction: 'input'; name: string; config: PipelineInputQueueConfig }
      | { direction: 'output'; name: string; config: PipelineOutputSinkConfig };
    rename: { pipelineId: string; name: string; alias: string };
    relinkExternal: { nodeId: string; pipelineId: string };
    setNodeMetadata: { nodeId: string; name?: string; summary?: string };
    addHostIoPort: { nodeId: string; name: string; dataTypeKey: string };
    removeHostIoPort: { nodeId: string; name: string };
  }>();

  const DEFAULT_TIMER_INTERVAL_MS = 33;
  const autosaveChipState = $derived.by<AutosaveChipState>(() => {
    if (context.savingState === 'saving') {
      return { value: 'Saving…', tone: 'info', showRetry: false };
    }
    if (context.savingState === 'error') {
      return { value: 'Save failed', tone: 'error', showRetry: true };
    }
    if (context.dirty) {
      return { value: 'Pending…', tone: 'warning', showRetry: false };
    }
    return { value: 'Up to date', tone: 'success', showRetry: false };
  });

  const revisionChipValue = $derived.by(() =>
    revisionDisplay(context.pipeline?.revision ?? null)
  );


  const shortPipelineId = $derived.by(() => context.pipeline?.id?.slice(0, 8) ?? '—');

  function focusDiagnosticWarning(warning: PipelineDiagnosticWarning) {
    if (!warning.nodeId || typeof graphEditor?.focusOnNode !== 'function') {
      return;
    }
    graphEditor.focusOnNode(warning.nodeId, { port: warning.port ?? null });
  }

  export function focusOnNode(nodeId: string, options?: { port?: string | null }) {
    if (!nodeId || typeof graphEditor?.focusOnNode !== 'function') return;
    graphEditor.focusOnNode(nodeId, { port: options?.port ?? null });
  }

  const pipelineWarnings = $derived.by<PipelineDiagnosticWarning[]>(() => {
    const warnings = context.pipeline?.diagnostics?.warnings ?? [];
    return Array.isArray(warnings) ? warnings : [];
  });

  const hasPipelineWarnings = $derived.by(() => pipelineWarnings.length > 0);

  let warningsPanelOpen = $state(false);
  let lastAutoValidationPipelineId = $state<string | null>(null);
  let lastAutoValidationIssueCount = $state<number | null>(null);

  $effect(() => {
    const warnings = pipelineWarnings;
    if (!context.pipeline) {
      warningsPanelOpen = false;
    }
  });

  $effect(() => {
    const pipeline = context.pipeline;
    if (!pipeline) {
      lastAutoValidationPipelineId = null;
      lastAutoValidationIssueCount = null;
      return;
    }
    const issueCount = Number.isFinite(pipeline.issueCount)
      ? Math.max(0, Math.floor(pipeline.issueCount ?? 0))
      : 0;
    const diagnostics = pipeline.diagnostics;
    const hasDiagnostics =
      Array.isArray(diagnostics?.warnings)
        ? diagnostics.warnings.length > 0
        : Boolean(diagnostics?.error);
    if (issueCount <= 0 || hasDiagnostics) return;
    if (
      pipeline.id === lastAutoValidationPipelineId &&
      issueCount === lastAutoValidationIssueCount
    ) {
      return;
    }
    lastAutoValidationPipelineId = pipeline.id;
    lastAutoValidationIssueCount = issueCount;
    dispatch('validate');
  });

  let previousSavingState: PipelineDetailContext['savingState'] = 'idle';
  let lastToastPipelineId: string | null = null;
  let lastSavedMarker: string | null = null;
  let heatmapEnabled = $state(false);
  let gpuOverlayEnabled = $state(false);
  let graphContainer: { toggleHeatmapNodeExclusion?: (nodeId: string | null | undefined) => void; isHeatmapNodeExcluded?: (nodeId: string | null | undefined) => boolean } | null =
    $state(null);

  export function toggleHeatmapNodeExclusion(nodeId: string | null | undefined) {
    graphContainer?.toggleHeatmapNodeExclusion?.(nodeId);
  }

  export function isHeatmapNodeExcluded(nodeId: string | null | undefined): boolean {
    return graphContainer?.isHeatmapNodeExcluded?.(nodeId) ?? false;
  }

  const saveMarker = (pipeline: PipelineDetailContext['pipeline'] | null): string | null => {
    if (!pipeline) return null;
    if (pipeline.revision) return pipeline.revision;
    if (pipeline.updatedAt) return String(pipeline.updatedAt);
    return null;
  };

  $effect(() => {
    const pipelineId = context.pipeline?.id ?? null;
    if (pipelineId !== lastToastPipelineId) {
      lastToastPipelineId = pipelineId;
      lastSavedMarker = saveMarker(context.pipeline);
      previousSavingState = context.savingState;
    }
  });

  $effect(() => {
    if (
      previousSavingState === 'saving' &&
      context.savingState === 'idle' &&
      !context.dirty &&
      context.pipeline
    ) {
      const marker = saveMarker(context.pipeline);
      if (marker && marker !== lastSavedMarker) {
        lastSavedMarker = marker;
      }
    }
    previousSavingState = context.savingState;
  });
  const canOrganizeGraph = $derived.by(() => {
    const pipeline = context.pipeline;
    if (!pipeline) return false;
    const nodes = pipeline.graph?.nodes ?? {};
    return Object.keys(nodes).length > 0;
  });
  let profileNameDraft = $state('');
  let profileAliasDraft = $state('');
  let profileError = $state<string | null>(null);

  const profileHasChanges = $derived.by(() => {
    const pipeline = context.pipeline;
    if (!pipeline) return false;
    return (
      profileNameDraft.trim() !== (pipeline.name ?? '') ||
      profileAliasDraft.trim() !== (pipeline.alias ?? '')
    );
  });

  $effect(() => {
    const pipeline = context.pipeline;
    if (!pipeline) {
      profileNameDraft = '';
      profileAliasDraft = '';
      profileError = null;
      return;
    }
    profileNameDraft = pipeline.name ?? '';
    profileAliasDraft = pipeline.alias ?? '';
    profileError = null;
  });

  function submitProfileUpdate() {
    if (!context.pipeline || !profileHasChanges) return;
    const trimmedName = profileNameDraft.trim();
    const trimmedAlias = profileAliasDraft.trim();
    if (!trimmedName) {
      profileError = 'Pipeline name is required.';
      return;
    }
    profileError = null;
    dispatch('rename', {
      pipelineId: context.pipeline.id,
      name: trimmedName,
      alias: trimmedAlias
    });
  }

</script>

{#if activeInspectorTab === 'node'}
  <NodeInspector
    context={context}
    typePalette={typePalette}
    registryEntries={registryEntries}
    pipelines={pipelines}
    onSetConstantValue={(payload) => dispatch('setNodeConstantValue', payload)}
    onSetSyncConfig={(payload) => dispatch('setNodeSyncConfig', payload)}
    onSetDaedalusNodeRuntime={(payload) => dispatch('setDaedalusNodeRuntime', payload)}
    onRelinkExternal={(payload) => {
      dispatch('relinkExternal', payload);
    }}
    onOpenExternal={(payload) => {
      dispatch('openPipeline', { pipelineId: payload.pipelineId });
    }}
    onSetMetadata={(payload) => dispatch('setNodeMetadata', payload)}
    onAddIoPort={(payload) => dispatch('addHostIoPort', payload)}
    onRemoveIoPort={(payload) => dispatch('removeHostIoPort', payload)}
  />
{:else if activeInspectorTab === 'connection'}
  <ConnectionInspector
    context={context}
    onSetPolicy={(payload) => dispatch('setConnectionPolicy', payload)}
    onSetStyle={(payload) => dispatch('setConnectionStyle', payload)}
  />
{:else if activeInspectorTab === 'boundary'}
  <BoundaryInspector
    context={context}
    inputs={pipelineInputEntries}
    outputs={pipelineOutputEntries}
    typePalette={typePalette}
    onRemovePort={(payload) => dispatch('removePipelinePort', payload)}
    onSetPortConfig={(payload) => dispatch('setPipelinePortConfig', payload)}
    onSetPortValue={(payload) => dispatch('setPipelinePortValue', payload)}
    onEditPort={(payload) => dispatch('editPipelinePort', payload)}
    onAddPort={(payload) => dispatch('addPipelinePort', payload)}
  />
{:else if activeInspectorTab === 'metrics'}
  <MetricsInspector context={context} onRefresh={() => dispatch('refreshMetrics')} />
{:else}
<div class="flex h-full min-h-0 flex-col gap-4 w-full">
  {#if !context.pipeline}
    <div class="flex flex-1 items-center justify-center rounded border border-dashed border-surface-700/70 bg-surface-950/60 p-6 text-sm text-surface-400">
      Select a pipeline to start editing.
    </div>
  {:else}
    {@const pipeline = context.pipeline}
	    <PipelineDetailHeaderSection
	      {context}
	      bind:profileNameDraft={profileNameDraft}
	      bind:profileAliasDraft={profileAliasDraft}
      {profileError}
      {autosaveChipState}
      {revisionChipValue}
      {shortPipelineId}
      bind:graphSearchQuery={graphSearchQuery}
      {normalizedGraphSearchQuery}
      metaChipBase={META_CHIP_BASE}
      metaChipTones={META_CHIP_TONES}
      actionChipBase={ACTION_CHIP_BASE}
      actionChipIconBase={ACTION_CHIP_ICON_BASE}
      {hasPipelineWarnings}
      {warningsPanelOpen}
      warningsCount={pipelineWarnings.length}
      {canOrganizeGraph}
      {canShowEngineConfig}
      {engineConfigOpen}
      {syncOverlayEnabled}
	      {gpuOverlayEnabled}
	      {heatmapEnabled}
	      {metricsInspectorActive}
	      onOpenProfiler={() => (profilerOpen = true)}
	      onSubmitProfile={submitProfileUpdate}
	      onClearProfileError={() => (profileError = null)}
	      onRetrySave={() => dispatch('save')}
	      onClearSearch={() => (graphSearchQuery = '')}
      onToggleWarnings={() => {
        dispatch('validate');
        warningsPanelOpen = !warningsPanelOpen;
      }}
      onOrganize={() => dispatch('organize')}
      onAssign={() => dispatch('assign')}
      onExport={() => dispatch('export', { inlineExternals: false })}
      onToggleSyncOverlay={() => (syncOverlayEnabled = !syncOverlayEnabled)}
      onToggleGpuOverlay={() => (gpuOverlayEnabled = !gpuOverlayEnabled)}
      onToggleHeatmap={() => {
        if (metricsInspectorActive) return;
        heatmapEnabled = !heatmapEnabled;
      }}
      onToggleEngineConfig={() => {
        if (!canShowEngineConfig) return;
        engineConfigOpen = !engineConfigOpen;
	      }}
	    />
	    <PipelineProfilerModal
	      open={profilerOpen}
	      title="Profiler"
	      pipelineId={(pipeline as any)?.id ?? null}
	      pipelineLabel={(pipeline as any)?.name ?? (pipeline as any)?.profileName ?? null}
	      bind:streamId={profilerStreamId}
	      streamOptions={profilerStreamOptions}
	      onClose={() => (profilerOpen = false)}
	    />
	    <PipelineDetailGraphContainer
	      {context}
	      {graphPlan}
	      {registryEntries}
      {breadcrumbs}
      {canShowEngineConfig}
      {engineConfigOpen}
      {warningsPanelOpen}
      pipelineWarnings={pipelineWarnings}
      {syncOverlayEnabled}
      {normalizedGraphSearchQuery}
      {metricsInspectorActive}
      captureDevices={captureDevicesList}
      bind:heatmapEnabled={heatmapEnabled}
      bind:gpuOverlayEnabled={gpuOverlayEnabled}
      bind:graphEditor={graphEditor}
      bind:this={graphContainer}
      onExitEmbedded={() => dispatch('exitEmbedded')}
      onCloseEngineConfig={() => (engineConfigOpen = false)}
      onCloseWarnings={() => (warningsPanelOpen = false)}
      onFocusWarning={focusDiagnosticWarning}
      onRefreshMetrics={() => dispatch('refreshMetrics')}
      onPlanChange={(plan) => dispatch('planChange', { plan })}
      onGraphSelect={(payload) => dispatch('graphSelect', payload)}
      onEnterEmbedded={(nodeId) => dispatch('enterEmbedded', { nodeId })}
      onGraphContext={(payload) => dispatch('graphContext', payload)}
      onGraphLayout={(layout) => {
        if (!context.pipeline) return;
        dispatch('graphLayout', { pipelineId: context.pipeline.id, layout });
      }}
      onRuntime={(payload) =>
        dispatch('setDaedalusNodeRuntime', {
          nodeId: payload.nodeId,
          syncGroups: payload.syncGroups
        })}
      onSetSyncConfig={(payload) => dispatch('setNodeSyncConfig', payload)}
      onSetDaedalusNodeRuntime={(payload) =>
        dispatch('setDaedalusNodeRuntime', {
          nodeId: payload.nodeId,
          syncGroups: payload.syncGroups
        })}
    />
  {/if}
</div>
{/if}

<style>
  :global(.warning-chip) {
    position: relative;
    transition: box-shadow 150ms ease, background-color 150ms ease, border-color 150ms ease;
  }
  :global(.warning-chip--active) {
    border-color: rgba(251, 191, 36, 0.7);
    background-color: rgba(120, 53, 15, 0.45);
    color: #fefce8;
    box-shadow: 0 0 12px rgba(251, 191, 36, 0.35);
  }
  :global(.warning-chip--inactive) {
    border-color: rgba(100, 116, 139, 0.5);
    background-color: rgba(15, 23, 42, 0.6);
    color: rgba(148, 163, 184, 0.85);
  }
  :global(.warning-chip--inactive:disabled) {
    cursor: default;
  }
  :global(.warning-chip--open) {
    box-shadow: 0 0 0 2px rgba(251, 191, 36, 0.45);
    background-color: rgba(120, 53, 15, 0.6);
    animation: none;
  }
  :global(.warning-chip--pulse) {
    animation: warningChipPulse 1.8s ease-in-out infinite;
  }
  @keyframes warningChipPulse {
    0%,
    100% {
      background-color: rgba(120, 53, 15, 0.38);
      box-shadow: 0 0 8px rgba(251, 191, 36, 0.4);
    }
    50% {
      background-color: rgba(251, 191, 36, 0.32);
      box-shadow: 0 0 22px rgba(251, 191, 36, 0.6);
    }
  }
</style>
