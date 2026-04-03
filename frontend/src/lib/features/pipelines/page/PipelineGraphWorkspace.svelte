<script lang="ts">
  import type PipelineDetailPanel from '$lib/components/pipelines/PipelineDetailPanel.svelte';
  import type { StreamInfo } from '$lib/api/client';
  import type { InspectorTabKey } from '$lib/features/pipelines/controller/types';
  import type { PipelineDetailContext, PipelineOutputEntry, PipelinePortEntry } from '$lib/components/pipelines/types';
  import type { PipelineGraphPlan, PipelineOverviewPipeline, PipelineRegistryEntry, PipelineTypeDescriptor } from '$lib/types/pipeline';

  type PipelineGraphWorkspaceEvent = CustomEvent<Record<string, unknown>>;
  type PipelineBreadcrumb = {
    id: string;
    name: string;
    status?: 'embedded' | 'linked' | 'mismatch' | 'unresolved';
    targetId?: string | null;
  };

  type PipelineGraphWorkspaceProps = {
    loadError?: string | null;
    registryLoading?: boolean;
    registryError?: string | null;
    registryEntries?: PipelineRegistryEntry[];
    detailPanelComponent?: typeof PipelineDetailPanel | null;
    detailPanelRef?: unknown;
    detailContext?: PipelineDetailContext;
    editingPlan?: PipelineGraphPlan | null;
    editingBreadcrumbs?: PipelineBreadcrumb[];
    dataTypes?: Record<string, PipelineTypeDescriptor>;
    pipelines?: PipelineOverviewPipeline[];
    pipelineInputEntries?: PipelinePortEntry[];
    pipelineOutputEntries?: PipelineOutputEntry[];
    inspectorTab?: InspectorTabKey;
    captureDevices?: StreamInfo[];
    selectedPipeline?: PipelineOverviewPipeline | null;
    emptyPlan?: () => PipelineGraphPlan;
    onOrganize?: () => void;
    onAssign?: () => void;
    onSave?: () => void;
    onValidate?: () => void;
    onExport?: (inlineExternals: boolean) => void;
    onClearValidation?: () => void;
    onRefreshMetrics?: () => void;
    onPlanChange?: (event: PipelineGraphWorkspaceEvent) => void;
    onGraphSelect?: (event: PipelineGraphWorkspaceEvent) => void;
    onEnterEmbedded?: (event: PipelineGraphWorkspaceEvent) => void;
    onOpenPipeline?: (pipelineId: string) => void;
    onExitEmbedded?: () => void;
    onGraphContext?: (event: PipelineGraphWorkspaceEvent) => void;
    onGraphLayout?: (event: PipelineGraphWorkspaceEvent) => void;
    onAddPipelinePort?: (event: PipelineGraphWorkspaceEvent) => void;
    onAddHostIoPort?: (event: PipelineGraphWorkspaceEvent) => void;
    onRemovePipelinePort?: (event: PipelineGraphWorkspaceEvent) => void;
    onRemoveHostIoPort?: (event: PipelineGraphWorkspaceEvent) => void;
    onEditPipelinePort?: (event: PipelineGraphWorkspaceEvent) => void;
    onSetPipelinePortValue?: (event: PipelineGraphWorkspaceEvent) => void;
    onSetPipelinePortConfig?: (event: PipelineGraphWorkspaceEvent) => void;
    onSetNodeConstantValue?: (event: PipelineGraphWorkspaceEvent) => void;
    onSetNodeSyncConfig?: (event: PipelineGraphWorkspaceEvent) => void;
    onSetDaedalusNodeRuntime?: (event: PipelineGraphWorkspaceEvent) => void;
    onSetConnectionPolicy?: (event: PipelineGraphWorkspaceEvent) => void;
    onSetConnectionStyle?: (event: PipelineGraphWorkspaceEvent) => void;
    onRename?: (event: PipelineGraphWorkspaceEvent) => void;
    onSetNodeMetadata?: (event: PipelineGraphWorkspaceEvent) => void;
    onOpenRegistryDrawer?: () => void;
  };

  let {
    loadError = null,
    registryLoading = false,
    registryError = null,
    registryEntries = [],
    detailPanelComponent = null,
    detailPanelRef = $bindable(null),
    detailContext,
    editingPlan = null,
    editingBreadcrumbs = null,
    dataTypes = {},
    pipelines = [],
    pipelineInputEntries = [],
    pipelineOutputEntries = [],
    inspectorTab,
    captureDevices = [],
    selectedPipeline = null,
    emptyPlan = () => ({ nodes: {}, connections: [] }),
    onOrganize = () => {},
    onAssign = () => {},
    onSave = () => {},
    onValidate = () => {},
    onExport = () => {},
    onClearValidation = () => {},
    onRefreshMetrics = () => {},
    onPlanChange = () => {},
    onGraphSelect = () => {},
    onEnterEmbedded = () => {},
    onOpenPipeline = () => {},
    onExitEmbedded = () => {},
    onGraphContext = () => {},
    onGraphLayout = () => {},
    onAddPipelinePort = () => {},
    onAddHostIoPort = () => {},
    onRemovePipelinePort = () => {},
    onRemoveHostIoPort = () => {},
    onEditPipelinePort = () => {},
    onSetPipelinePortValue = () => {},
    onSetPipelinePortConfig = () => {},
    onSetNodeConstantValue = () => {},
    onSetNodeSyncConfig = () => {},
    onSetDaedalusNodeRuntime = () => {},
    onSetConnectionPolicy = () => {},
    onSetConnectionStyle = () => {},
    onRename = () => {},
    onSetNodeMetadata = () => {},
    onOpenRegistryDrawer = () => {}
  }: PipelineGraphWorkspaceProps = $props();

  export type $$Props = PipelineGraphWorkspaceProps;
</script>

<section class="flex min-h-0 flex-1 flex-col gap-6">
  {#if loadError}
    <div class="rounded border border-amber-700/60 bg-amber-950/50 p-4 text-sm text-amber-100">
      {loadError}
    </div>
  {/if}
  {#if !registryLoading && !registryError && registryEntries.length === 0}
    <div class="rounded border border-amber-700/60 bg-amber-950/40 p-3 text-xs text-amber-100">
      Node registry is empty — port types will fall back to Generic. Check API connectivity and reload.
    </div>
  {/if}
  <div class="relative flex min-h-0 flex-1 flex-col">
    <div class="flex min-h-0 flex-1 flex-col p-4">
      {#if detailPanelComponent}
        {@const DetailPanel = detailPanelComponent}
        <DetailPanel
          bind:this={detailPanelRef}
          context={detailContext}
          plan={editingPlan ?? (selectedPipeline?.graph ?? emptyPlan())}
          breadcrumbs={editingBreadcrumbs}
          typePalette={dataTypes}
          registryEntries={registryEntries}
          pipelines={pipelines}
          pipelineInputEntries={pipelineInputEntries}
          pipelineOutputEntries={pipelineOutputEntries}
          inspectorTab={inspectorTab}
          captureDevices={captureDevices}
          on:organize={onOrganize}
          on:assign={onAssign}
          on:save={onSave}
          on:validate={onValidate}
          on:export={(event) => onExport(event.detail?.inlineExternals ?? false)}
          on:clearValidation={onClearValidation}
          on:refreshMetrics={onRefreshMetrics}
          on:planChange={onPlanChange}
          on:graphSelect={onGraphSelect}
          on:enterEmbedded={onEnterEmbedded}
          on:openPipeline={(event) => onOpenPipeline(event.detail.pipelineId)}
          on:exitEmbedded={onExitEmbedded}
          on:graphContext={onGraphContext}
          on:graphLayout={onGraphLayout}
          on:addPipelinePort={onAddPipelinePort}
          on:addHostIoPort={onAddHostIoPort}
          on:removePipelinePort={onRemovePipelinePort}
          on:removeHostIoPort={onRemoveHostIoPort}
          on:editPipelinePort={onEditPipelinePort}
          on:setPipelinePortValue={onSetPipelinePortValue}
          on:setPipelinePortConfig={onSetPipelinePortConfig}
          on:setNodeConstantValue={onSetNodeConstantValue}
          on:setNodeSyncConfig={onSetNodeSyncConfig}
          on:setDaedalusNodeRuntime={onSetDaedalusNodeRuntime}
          on:setConnectionPolicy={onSetConnectionPolicy}
          on:setConnectionStyle={onSetConnectionStyle}
          on:rename={onRename}
          on:setNodeMetadata={onSetNodeMetadata}
        />
      {:else}
        <div class="flex flex-1 items-center justify-center text-sm text-surface-500">Loading editor...</div>
      {/if}
    </div>
    <div class="pointer-events-none absolute bottom-6 left-6 z-10">
      <button
        class="pointer-events-auto btn btn-sm preset-filled-error-500 uppercase tracking-[0.3em] shadow-lg shadow-error-500/30"
        type="button"
        onclick={onOpenRegistryDrawer}
      >
        Node registry
      </button>
    </div>
  </div>
</section>
