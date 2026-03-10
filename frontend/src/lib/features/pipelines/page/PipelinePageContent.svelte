<script lang="ts">
  const { ctx } = $props<{ ctx: Record<string, unknown> }>();

  const PipelineDetailPanelComponent = $derived.by(() => ctx.PipelineDetailPanelComponent);
  const PipelineGraphContextMenu = $derived.by(() => ctx.PipelineGraphContextMenu);
  const PipelineGraphWorkspace = $derived.by(() => ctx.PipelineGraphWorkspace);
  const PipelineInspectorPanel = $derived.by(() => ctx.PipelineInspectorPanel);
  const PipelineTunePanel = $derived.by(() => ctx.PipelineTunePanel);

  const activeTab = $derived.by(() => ctx.activeTab);
  const loadError = $derived.by(() => ctx.loadError);
  const registryLoading = $derived.by(() => ctx.registryLoading);
  const registryError = $derived.by(() => ctx.registryError);
  const registry = $derived.by(() => ctx.registry);
  const detailContext = $derived.by(() => ctx.detailContext);
  const editingPlan = $derived.by(() => ctx.editingPlan);
  const editingBreadcrumbs = $derived.by(() => ctx.editingBreadcrumbs);
  const dataTypes = $derived.by(() => ctx.dataTypes);
  const pipelines = $derived.by(() => ctx.pipelines);
  const pipelineInputEntries = $derived.by(() => ctx.pipelineInputEntries);
  const pipelineOutputEntries = $derived.by(() => ctx.pipelineOutputEntries);
  const inspectorTab = $derived.by(() => ctx.inspectorTab);
  const captureDevices = $derived.by(() => ctx.captureDevices);
  const selectedPipeline = $derived.by(() => ctx.selectedPipeline);
</script>

{#snippet content()}
  {@const {
    RAW_STREAM_PIPELINE_ID,
    RAW_STREAM_PIPELINE_UUID,
    accessBadgeClass,
    accessLabel,
    addBoundaryDraftPort,
    applyStreamControl,
    applyBoundaryDraft,
    applyGroupDraft,
    addNodeFromRegistry,
    clearTuneMultiplexCell,
    clearTuneNodeDraft,
    closeGraphContextMenu,
    clearValidation,
    controlMax,
    controlMin,
    controlStep,
    contextRegistryOptions,
    displayControlValue,
    dropTuneMultiplexOn,
    emptyPipelineGraphPlan,
    exportCurrentPipeline,
    extractControlValue,
    groupSelection,
    graphContextMenu,
    graphContextSearch,
    graphSelection,
    handleEdgePolicy,
    handleEdgeStyle,
    handleEnterEmbedded,
    handleExitEmbedded,
    handleHostIoPortAdd,
    handleHostIoPortRemove,
    handleNodeConstantValue,
    handlePanelGraphContext,
    handlePanelGraphLayout,
    handlePanelGraphSelect,
    handlePanelPlanChange,
    handlePipelinePortAdd,
    handlePipelinePortConfig,
    handlePipelinePortEdit,
    handlePipelinePortRemove,
    handlePipelinePortValue,
    handlePipelineRename,
    handlePipelineValidate,
    handlePlanChange,
    handleTuneAssign,
    isDaedalusPlan,
    menuOptions,
    metricsSource,
    metricsStatusLabel,
    metricsUpdatedLabel,
    normalizePortKey,
    openActionMenu,
    openAssignModal,
    openBoundaryEditor,
    openBoundaryMenu,
    openGroupEditor,
    openRegistryPalette,
    organizeGraphNodes,
    pipelineLabelById,
    pipelineMetricsSummary,
    readTuneNodeDraft,
    readTuneStreamNodeDraft,
    refreshPipelineMetrics,
    removeBoundaryDraftPort,
    removeGraphConnection,
    removeGraphNode,
    registryDrawerOpen,
    resetTunePipelineUi,
    safeClonePlan,
    saveCurrentPipeline,
    saveTunePipelineUi,
    scheduleControlApply,
    scheduleTuneGlobalAutoSave,
    scheduleTuneMultiplexAutoApply,
    setActiveTab,
    setBoundaryDraftDirection,
    setBoundaryDraftPortName,
    setGraphContextMenuElement,
    setGroupDraftColor,
    setGroupDraftName,
    setGroupDraftSummary,
    setDaedalusNodeRuntime,
    setNodeConstantValue,
    setNodeMetadata,
    setNodeSyncConfig,
    setSelectedPipeline,
    setTuneLivePipelineOutput,
    setTuneMultiplexGridDimensions,
    setTuneNodeError,
    setTuneOutputKeyForCell,
    setTuneOutputSelectionForPipeline,
    startTuneMultiplexDrag,
    streamLabel,
    ungroupSelection,
    graphBindings,
    tuneBindings,
    tuneAllowDrop,
    tuneConstantGroups,
    tuneControlsError,
    tuneControlsLoading,
    tuneFilteredConstantGroups,
    tuneFilteredControls,
    tuneMultiplexColumnIndices,
    tuneMultiplexError,
    tuneMultiplexGridIsSingle,
    tuneMultiplexLayoutSignature,
    tuneMultiplexOutputOptionsCache,
    tuneMultiplexPalettePipelineIds,
    tuneMultiplexRowIndices,
    tuneNodeDescriptors,
    tuneNodeErrors,
    tuneOutputKeyForCell,
    tuneOutputSelectionForPipeline,
    tunePipelineAssignFilteredGraphs,
    tunePipelineForCell,
    tunePipelineGraphs,
    tunePlan,
    tunePreviewStream,
    tuneStreamApplyErrorById,
    tuneStreamControls,
    tuneStreamNodeErrorsById,
    tuneStreamNodeOverridesById,
    tuneStreamsError,
    tuneStreamsForPipeline,
    tuneStreamsLoading,
    tuneUiMode,
    updateGlobalNodeValue,
    updateStreamNodeValue
  } = ctx}

  {#if PipelineInspectorPanel}
    {@const InspectorPanel = PipelineInspectorPanel}
    <InspectorPanel activeTab={$activeTab} onSelectTab={setActiveTab}>
      {#snippet pipeline()}
        {#if PipelineGraphWorkspace}
          {@const GraphWorkspace = PipelineGraphWorkspace}
          <GraphWorkspace
            loadError={$loadError}
            registryLoading={$registryLoading}
            registryError={$registryError}
            registryEntries={$registry}
            detailPanelComponent={PipelineDetailPanelComponent}
            bind:detailPanelRef={graphBindings.detailPanelRef}
            detailContext={$detailContext}
            editingPlan={$editingPlan}
            editingBreadcrumbs={$editingBreadcrumbs}
            dataTypes={$dataTypes}
            pipelines={$pipelines}
            pipelineInputEntries={$pipelineInputEntries}
            pipelineOutputEntries={$pipelineOutputEntries}
            inspectorTab={$inspectorTab}
            captureDevices={$captureDevices}
            selectedPipeline={$selectedPipeline}
            emptyPlan={emptyPipelineGraphPlan}
            onOrganize={organizeGraphNodes}
            onAssign={openAssignModal}
            onSave={saveCurrentPipeline}
            onValidate={handlePipelineValidate}
            onExport={exportCurrentPipeline}
            onClearValidation={() => $selectedPipeline && clearValidation($selectedPipeline.id)}
            onRefreshMetrics={() => refreshPipelineMetrics()}
            onPlanChange={handlePanelPlanChange}
            onGraphSelect={handlePanelGraphSelect}
            onEnterEmbedded={handleEnterEmbedded}
            onOpenPipeline={(pipelineId) => setSelectedPipeline(pipelineId)}
            onExitEmbedded={handleExitEmbedded}
            onGraphContext={handlePanelGraphContext}
            onGraphLayout={handlePanelGraphLayout}
            onAddPipelinePort={handlePipelinePortAdd}
            onAddHostIoPort={handleHostIoPortAdd}
            onRemovePipelinePort={handlePipelinePortRemove}
            onRemoveHostIoPort={handleHostIoPortRemove}
            onEditPipelinePort={handlePipelinePortEdit}
            onSetPipelinePortValue={handlePipelinePortValue}
            onSetPipelinePortConfig={handlePipelinePortConfig}
            onSetNodeConstantValue={handleNodeConstantValue}
            onSetNodeSyncConfig={(event) => setNodeSyncConfig(event.detail.nodeId, event.detail.config)}
            onSetDaedalusNodeRuntime={(event) =>
              setDaedalusNodeRuntime(event.detail.nodeId, {
                syncGroups: event.detail.syncGroups
              })
            }
            onSetConnectionPolicy={handleEdgePolicy}
            onSetConnectionStyle={handleEdgeStyle}
            onRename={handlePipelineRename}
            onSetNodeMetadata={(event) =>
              setNodeMetadata(event.detail.nodeId, {
                name: event.detail.name,
                summary: event.detail.summary
              })
            }
            onOpenRegistryDrawer={() => registryDrawerOpen.set(true)}
          />
          {#if PipelineGraphContextMenu}
            {@const GraphContextMenu = PipelineGraphContextMenu}
            <GraphContextMenu
              editingPlan={editingPlan}
              graphContextMenu={graphContextMenu}
              graphContextSearch={graphContextSearch}
              contextRegistryOptions={contextRegistryOptions}
              graphSelection={graphSelection}
              openRegistryPalette={openRegistryPalette}
              openActionMenu={openActionMenu}
              openBoundaryMenu={openBoundaryMenu}
              openBoundaryEditor={openBoundaryEditor}
              openGroupEditor={openGroupEditor}
              closeGraphContextMenu={closeGraphContextMenu}
              addNodeFromRegistry={addNodeFromRegistry}
              addBoundaryDraftPort={addBoundaryDraftPort}
              removeBoundaryDraftPort={removeBoundaryDraftPort}
              setBoundaryDraftPortName={setBoundaryDraftPortName}
              setBoundaryDraftDirection={setBoundaryDraftDirection}
              applyBoundaryDraft={applyBoundaryDraft}
              setGroupDraftName={setGroupDraftName}
              setGroupDraftSummary={setGroupDraftSummary}
              setGroupDraftColor={setGroupDraftColor}
              applyGroupDraft={applyGroupDraft}
              setNodeMetadata={setNodeMetadata}
              removeGraphNode={removeGraphNode}
              removeGraphConnection={removeGraphConnection}
              groupSelection={groupSelection}
              ungroupSelection={ungroupSelection}
              setGraphContextMenuElement={setGraphContextMenuElement}
            />
          {/if}
        {:else if $selectedPipeline}
          <section class="flex min-h-0 flex-1 items-center justify-center rounded border border-surface-800/60 bg-surface-950/60 text-xs uppercase tracking-[0.24em] text-surface-400">
            Loading graph editor…
          </section>
        {:else}
          <section class="flex min-h-0 flex-1 items-center justify-center rounded border border-surface-800/60 bg-surface-950/60 text-center text-xs uppercase tracking-[0.24em] text-surface-400">
            Select a pipeline to open the graph editor
          </section>
        {/if}
      {/snippet}

      {#snippet tune()}
        {#if PipelineTunePanel}
          {@const TunePanel = PipelineTunePanel}
          <TunePanel
            bind:tuneUiEditMode={tuneBindings.tuneUiEditMode}
            bind:tuneScopeTab={tuneBindings.tuneScopeTab}
            bind:tunePipelineUiSearch={tuneBindings.tunePipelineUiSearch}
            bind:tunePipelineUiDraft={tuneBindings.tunePipelineUiDraft}
            bind:tuneUiActiveTabId={tuneBindings.tuneUiActiveTabId}
            bind:tuneUiSelectedItemId={tuneBindings.tuneUiSelectedItemId}
            bind:tuneUiSelectedItemAnchor={tuneBindings.tuneUiSelectedItemAnchor}
            bind:tuneConstantSearch={tuneBindings.tuneConstantSearch}
            bind:tunePerformanceTab={tuneBindings.tunePerformanceTab}
            bind:tuneControlsQuery={tuneBindings.tuneControlsQuery}
            bind:tuneShowReadOnlyControls={tuneBindings.tuneShowReadOnlyControls}
            bind:tuneControlState={tuneBindings.tuneControlState}
            bind:tuneControlAppliedState={tuneBindings.tuneControlAppliedState}
            bind:tuneControlBusy={tuneBindings.tuneControlBusy}
            bind:tuneMultiplexRows={tuneBindings.tuneMultiplexRows}
            bind:tuneMultiplexColumns={tuneBindings.tuneMultiplexColumns}
            bind:tuneSelectedPipelineOutput={tuneBindings.tuneSelectedPipelineOutput}
            bind:tunePipelineRemoveModalOpen={tuneBindings.tunePipelineRemoveModalOpen}
            bind:tunePipelineRemoveCandidateId={tuneBindings.tunePipelineRemoveCandidateId}
            bind:tunePipelineAssignModalOpen={tuneBindings.tunePipelineAssignModalOpen}
            bind:tunePipelineAssignQuery={tuneBindings.tunePipelineAssignQuery}
            bind:tunePipelineAssignDraft={tuneBindings.tunePipelineAssignDraft}
            selectedPipeline={$selectedPipeline}
            tuneStreamsForPipeline={tuneStreamsForPipeline}
            tuneStreamsLoading={tuneStreamsLoading}
            tuneStreamsError={tuneStreamsError}
            tunePlan={tunePlan}
            tuneNodeDescriptors={tuneNodeDescriptors}
            tuneNodeErrors={tuneNodeErrors}
            tuneConstantGroups={tuneConstantGroups}
            tuneFilteredConstantGroups={tuneFilteredConstantGroups}
            tuneStreamNodeOverridesById={tuneStreamNodeOverridesById}
            tuneStreamNodeErrorsById={tuneStreamNodeErrorsById}
            tuneStreamApplyErrorById={tuneStreamApplyErrorById}
            tunePreviewStream={tunePreviewStream}
            tuneUiMode={tuneUiMode}
            tuneStreamControls={tuneStreamControls}
            tuneControlsLoading={tuneControlsLoading}
            tuneControlsError={tuneControlsError}
            tuneFilteredControls={tuneFilteredControls}
            metricsStatusLabel={metricsStatusLabel}
            metricsUpdatedLabel={metricsUpdatedLabel}
            metricsSource={metricsSource}
            pipelineMetricsSummary={pipelineMetricsSummary}
            menuOptions={menuOptions}
            tuneMultiplexError={tuneMultiplexError}
            tuneMultiplexPalettePipelineIds={tuneMultiplexPalettePipelineIds}
            tuneMultiplexRowIndices={tuneMultiplexRowIndices}
            tuneMultiplexColumnIndices={tuneMultiplexColumnIndices}
            tuneMultiplexOutputOptionsCache={tuneMultiplexOutputOptionsCache}
            tuneMultiplexLayoutSignature={tuneMultiplexLayoutSignature}
            tunePipelineGraphs={tunePipelineGraphs}
            tunePipelineAssignFilteredGraphs={tunePipelineAssignFilteredGraphs}
            tuneMultiplexGridIsSingle={tuneMultiplexGridIsSingle}
            tuneAllowDrop={tuneAllowDrop}
            streamLabel={streamLabel}
            normalizePortKey={normalizePortKey}
            readTuneNodeDraft={readTuneNodeDraft}
            updateGlobalNodeValue={updateGlobalNodeValue}
            clearTuneNodeDraft={clearTuneNodeDraft}
            setTuneNodeError={setTuneNodeError}
            scheduleTuneGlobalAutoSave={scheduleTuneGlobalAutoSave}
            scheduleTuneMultiplexAutoApply={scheduleTuneMultiplexAutoApply}
            setNodeConstantValue={setNodeConstantValue}
            handlePlanChange={handlePlanChange}
            isDaedalusPlan={isDaedalusPlan}
            safeClonePlan={safeClonePlan}
            readTuneStreamNodeDraft={readTuneStreamNodeDraft}
            updateStreamNodeValue={updateStreamNodeValue}
            saveTunePipelineUi={saveTunePipelineUi}
            resetTunePipelineUi={resetTunePipelineUi}
            applyStreamControl={applyStreamControl}
            scheduleControlApply={scheduleControlApply}
            displayControlValue={displayControlValue}
            extractControlValue={extractControlValue}
            controlMin={controlMin}
            controlMax={controlMax}
            controlStep={controlStep}
            accessLabel={accessLabel}
            accessBadgeClass={accessBadgeClass}
            startTuneMultiplexDrag={startTuneMultiplexDrag}
            pipelineLabelById={pipelineLabelById}
            setTuneMultiplexGridDimensions={setTuneMultiplexGridDimensions}
            dropTuneMultiplexOn={dropTuneMultiplexOn}
            clearTuneMultiplexCell={clearTuneMultiplexCell}
            tunePipelineForCell={tunePipelineForCell}
            tuneOutputSelectionForPipeline={tuneOutputSelectionForPipeline}
            tuneOutputKeyForCell={tuneOutputKeyForCell}
            setTuneOutputSelectionForPipeline={setTuneOutputSelectionForPipeline}
            setTuneOutputKeyForCell={setTuneOutputKeyForCell}
            setTuneLivePipelineOutput={setTuneLivePipelineOutput}
            onRequestAssign={handleTuneAssign}
            RAW_STREAM_PIPELINE_ID={RAW_STREAM_PIPELINE_ID}
            RAW_STREAM_PIPELINE_UUID={RAW_STREAM_PIPELINE_UUID}
            pipelineOutputEntries={$pipelineOutputEntries}
            typePalette={$dataTypes}
          />
        {:else if $selectedPipeline}
          <section class="flex min-h-0 flex-1 items-center justify-center rounded border border-surface-800/60 bg-surface-950/60 text-xs uppercase tracking-[0.24em] text-surface-400">
            Loading tune tools…
          </section>
        {:else}
          <section class="flex min-h-0 flex-1 items-center justify-center rounded border border-surface-800/60 bg-surface-950/60 text-center text-xs uppercase tracking-[0.24em] text-surface-400">
            Select a pipeline to load tune tools
          </section>
        {/if}
      {/snippet}
    </InspectorPanel>
  {:else}
    <section class="flex min-h-0 flex-1 items-center justify-center rounded border border-surface-800/60 bg-surface-950/60 text-xs uppercase tracking-[0.24em] text-surface-400">
      Loading pipeline workspace…
    </section>
  {/if}
{/snippet}

{@render content()}
