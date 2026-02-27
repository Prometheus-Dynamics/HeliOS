<script lang="ts">
  const { ctx } = $props<{ ctx: any }>();

  const PipelineDetailPanelComponent = $derived.by(() => ctx.PipelineDetailPanelComponent);
  const PipelineGraphContextMenu = $derived.by(() => ctx.PipelineGraphContextMenu);
  const PipelineGraphWorkspace = $derived.by(() => ctx.PipelineGraphWorkspace);
  const PipelineInspectorPanel = $derived.by(() => ctx.PipelineInspectorPanel);
  const PipelineTunePanel = $derived.by(() => ctx.PipelineTunePanel);

  const activeTab = ctx.activeTab;
  const loadError = ctx.loadError;
  const registryLoading = ctx.registryLoading;
  const registryError = ctx.registryError;
  const registry = ctx.registry;
  const detailContext = ctx.detailContext;
  const editingPlan = ctx.editingPlan;
  const editingBreadcrumbs = ctx.editingBreadcrumbs;
  const dataTypes = ctx.dataTypes;
  const pipelines = ctx.pipelines;
  const pipelineInputEntries = ctx.pipelineInputEntries;
  const pipelineOutputEntries = ctx.pipelineOutputEntries;
  const inspectorTab = ctx.inspectorTab;
  const captureDevices = ctx.captureDevices;
  const selectedPipeline = ctx.selectedPipeline;
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
  closeAssignModal,
  closeGraphContextMenu,
  clearValidation,
  controlMax,
  controlMin,
  controlStep,
  contextRegistryOptions,
  createModalOpen,
  createMode,
  createName,
  createSourcePipelineId,
  createSourceTemplateId,
  createBusy,
  createError,
  customNodeSearch,
  deleteModalBusy,
  deleteModalError,
  deleteModalOpen,
  deleteModalPipeline,
  detailPanelRef,
  diffDaedalusNodeValues,
  displayControlValue,
  dropTuneMultiplexOn,
  emptyPipelineGraphPlan,
  ensureTuneControlSocket,
  ensureTunePipelinesApplied,
  exportCurrentPipeline,
  extractControlValue,
  extractNodeOverridesFromGraph,
  fetchTuneMetricsSnapshots,
  fetchTuneStreams,
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
  isInitialLoading,
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
  pipelineSearch,
  pipelinesRefreshing,
  readTuneNodeDraft,
  readTuneStreamNodeDraft,
  refreshPipelineMetrics,
  removeBoundaryDraftPort,
  removeGraphConnection,
  removeGraphNode,
  registryDrawerOpen,
  resolveTunePipelineGraph,
  resolveTunePipelineOutput,
  resetTunePipelineUi,
  safeClonePlan,
  saveCurrentPipeline,
  saveTunePipelineUi,
  scheduleControlApply,
  scheduleTuneGlobalAutoSave,
  scheduleTuneMultiplexAutoApply,
  scheduleTuneStreamAutoApply,
  setActiveTab,
  setBoundaryDraftDirection,
  setBoundaryDraftPortName,
  setBoundaryDraftPortType,
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
  setTuneNodeDraft,
  setTuneNodeError,
  setTuneOutputKeyForCell,
  setTuneOutputSelectionForPipeline,
  setTuneStreamNodeDraft,
  setTuneStreamNodeError,
  startTuneMultiplexDrag,
  streamLabel,
  ungroupSelection,
  graphBindings,
  tuneBindings,
  tuneActiveStreamId,
  tuneAllowDrop,
  tuneConstantGroups,
  tuneConstantSearch,
  tuneControlAppliedState,
  tuneControlBusy,
  tuneControlSocket,
  tuneControlState,
  tuneControlsError,
  tuneControlsLoadedStreamId,
  tuneControlsLoading,
  tuneControlsQuery,
  tuneControlsRequestId,
  tuneFilteredConstantGroups,
  tuneFilteredControls,
  tuneMetricsError,
  tuneMetricsInFlight,
  tuneMetricsPollTimer,
  tuneMetricsRefreshPipelineId,
  tuneMetricsRequestId,
  tuneMetricsSnapshots,
  tuneMetricsStatus,
  tuneMetricsUpdatedAt,
  tuneMultiplexAutoApplyTimer,
  tuneMultiplexBusy,
  tuneMultiplexColumnIndices,
  tuneMultiplexColumns,
  tuneMultiplexDirty,
  tuneMultiplexDragPipelineId,
  tuneMultiplexDragSource,
  tuneMultiplexError,
  tuneMultiplexGridIsSingle,
  tuneMultiplexHydratedSignature,
  tuneMultiplexHydratedStreamId,
  tuneMultiplexLayoutSignature,
  tuneMultiplexLastAppliedSignature,
  tuneMultiplexOutputOptionsCache,
  tuneMultiplexPalettePipelineIds,
  tuneMultiplexRowIndices,
  tuneMultiplexRows,
  tuneMultiplexSlotOutputs,
  tuneMultiplexSlots,
  tuneNodeDescriptors,
  tuneNodeDrafts,
  tuneNodeErrors,
  tuneOutputKeyForCell,
  tuneOutputSelectionForPipeline,
  tunePerformanceTab,
  tunePipelineAssignDraft,
  tunePipelineAssignFilteredGraphs,
  tunePipelineAssignModalOpen,
  tunePipelineAssignQuery,
  tunePipelineForCell,
  tunePipelineGraphs,
  tunePipelineRemoveCandidateId,
  tunePipelineRemoveModalOpen,
  tunePipelineUiDraft,
  tunePipelineUiSearch,
  tunePlan,
  tunePreviewStream,
  tuneRuntimeFromStats,
  tuneScopeTab,
  tuneSelectedPipelineOutput,
  tuneSelectedStream,
  tuneShowReadOnlyControls,
  tuneStreamApplyBusyById,
  tuneStreamApplyErrorById,
  tuneStreamApplyQueuedById,
  tuneStreamApplyRafById,
  tuneStreamAutoApplyTimerById,
  tuneStreamControls,
  tuneStreamInputOverridesById,
  tuneStreamLastAppliedNodeOverridesById,
  tuneStreamLastAppliedSignatureById,
  tuneStreamNodeDraftsById,
  tuneStreamNodeErrorsById,
  tuneStreamNodeOverridesById,
  tuneStreamOverridesLoaded,
  tuneStreams,
  tuneStreamsError,
  tuneStreamsForPipeline,
  tuneStreamsLoaded,
  tuneStreamsLoading,
  tuneUiActiveTabId,
  tuneUiEditMode,
  tuneUiMode,
  tuneUiSelectedItemAnchor,
  tuneUiSelectedItemId,
  updateGlobalNodeValue,
  updateStreamNodeValue
} = ctx}

  <PipelineInspectorPanel activeTab={$activeTab} onSelectTab={setActiveTab}>
  {#snippet pipeline()}
    <PipelineGraphWorkspace
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
    <PipelineGraphContextMenu
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
      setBoundaryDraftPortType={setBoundaryDraftPortType}
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
  {/snippet}

  {#snippet tune()}
    <PipelineTunePanel
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
  {/snippet}

</PipelineInspectorPanel>
{/snippet}

{@render content()}
