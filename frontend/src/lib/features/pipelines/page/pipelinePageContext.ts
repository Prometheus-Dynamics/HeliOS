type AnyRecord = Record<string, any>;
type PipelinePageModalStores = {
  create: {
    modalOpen: unknown;
    mode: unknown;
    name: unknown;
    sourcePipelineId: unknown;
    sourceTemplateId: unknown;
    busy: unknown;
    error: unknown;
  };
  assign: {
    modalOpen: unknown;
    captureDevices: unknown;
    selectedCaptureSessionId: unknown;
    busy: unknown;
    error: unknown;
  };
};
type PipelinePageBaseContextArgs = {
  config: AnyRecord;
  components: AnyRecord;
  controller: {
    stores: {
      pipeline: AnyRecord;
      graph: AnyRecord;
      inspector: AnyRecord;
      registry: AnyRecord;
      modals: PipelinePageModalStores;
    };
    actions: AnyRecord;
    helpers: {
      registry: AnyRecord;
    };
  };
  state: AnyRecord;
  ide: AnyRecord;
  graph: AnyRecord;
  localActions: AnyRecord;
  localHelpers: AnyRecord;
};

export function createPipelinePageBaseContext(args: PipelinePageBaseContextArgs) {
  const { config, components, controller, state, ide, graph, localActions, localHelpers } = args;
  const pipelineStores = controller.stores.pipeline;
  const graphStores = controller.stores.graph;
  const inspectorStores = controller.stores.inspector;
  const registryStores = controller.stores.registry;
  const modalStores = controller.stores.modals;
  const controllerActions = controller.actions;
  const controllerRegistryHelpers = controller.helpers.registry;

  return {
    ...config,
    ...components,
    ...state,
    ...ide,
    ...graph,
    ...localActions,
    ...localHelpers,
    activeTab: state.activeTab,
    registryDrawerOpen: state.registryDrawerOpen,
    pipelines: pipelineStores.pipelines,
    pipelineListItems: pipelineStores.pipelineListItems,
    selectedPipelineId: pipelineStores.selectedPipelineId,
    pipelineSearch: pipelineStores.pipelineSearch,
    loadError: pipelineStores.loadError,
    dataTypes: pipelineStores.dataTypes,
    selectedPipeline: pipelineStores.selectedPipeline,
    templates: pipelineStores.templates,
    detailContext: inspectorStores.detailContext,
    editingPlan: graphStores.editingPlan,
    editingBreadcrumbs: graphStores.editingBreadcrumbs,
    pipelineInputEntries: inspectorStores.pipelineInputEntries,
    pipelineOutputEntries: inspectorStores.pipelineOutputEntries,
    inspectorTab: inspectorStores.tab,
    captureDevices: modalStores.assign.captureDevices,
    registry: registryStores.entries,
    registryLoading: registryStores.loading,
    registryError: registryStores.error,
    registrySort: registryStores.sort,
    registryView: registryStores.view,
    registryStores,
    registryHelpers: state.registryHelpers,
    setSelectedPipeline: controllerActions.setSelectedPipeline,
    openAssignModal: controllerActions.openAssignModal,
    closeAssignModal: controllerActions.closeAssignModal,
    attachPipelineToDevice: controllerActions.attachPipelineToDevice,
    saveCurrentPipeline: controllerActions.saveCurrentPipeline,
    clearValidation: controllerActions.clearValidation,
    setNodeConstantValue: controllerActions.setNodeConstantValue,
    handlePlanChange: controllerActions.handlePlanChange,
    refreshPipelineMetrics: controllerActions.refreshPipelineMetrics,
    setNodeSyncConfig: controllerActions.setNodeSyncConfig,
    setDaedalusNodeRuntime: controllerActions.setDaedalusNodeRuntime,
    setNodeMetadata: controllerActions.setNodeMetadata,
    createModalOpen: modalStores.create.modalOpen,
    createMode: modalStores.create.mode,
    createName: modalStores.create.name,
    createSourcePipelineId: modalStores.create.sourcePipelineId,
    createSourceTemplateId: modalStores.create.sourceTemplateId,
    createBusy: modalStores.create.busy,
    createError: modalStores.create.error,
    assignModalOpen: modalStores.assign.modalOpen,
    assignError: modalStores.assign.error,
    assignBusy: modalStores.assign.busy,
    selectedCaptureSessionId: modalStores.assign.selectedCaptureSessionId,
    addNodeFromRegistry: controllerActions.addNodeFromRegistry,
    addBoundaryDraftPort: controllerActions.addBoundaryDraftPort,
    applyBoundaryDraft: controllerActions.applyBoundaryDraft,
    applyGroupDraft: controllerActions.applyGroupDraft,
    closeGraphContextMenu: controllerActions.closeGraphContextMenu,
    contextRegistryOptions: graphStores.contextRegistryOptions,
    removeBoundaryDraftPort: controllerActions.removeBoundaryDraftPort,
    removeGraphConnection: controllerActions.removeGraphConnection,
    removeGraphNode: controllerActions.removeGraphNode,
    ungroupSelection: controllerActions.ungroupSelection,
    setSelectedCaptureSession: controllerActions.setSelectedCaptureSession,
    openCreateModal: controllerActions.openCreateModal,
    closeCreateModal: controllerActions.closeCreateModal,
    createPipeline: controllerActions.createPipeline,
    updateRegistryFilters: controllerActions.updateRegistryFilters,
    selectRegistryGroup: controllerActions.selectRegistryGroup,
    resetRegistryFilters: controllerActions.resetRegistryFilters,
    refreshRegistry: controllerActions.refreshRegistry,
    setGraphContextSearch: controllerActions.setGraphContextSearch,
    handleGraphSelect: controllerActions.handleGraphSelect,
    updateGraphLayout: controllerActions.updateGraphLayout,
    openGraphContext: controllerActions.openGraphContext,
    openRegistryPalette: controllerActions.openRegistryPalette,
    openActionMenu: controllerActions.openActionMenu,
    openBoundaryMenu: controllerActions.openBoundaryMenu,
    openBoundaryEditor: controllerActions.openBoundaryEditor,
    setBoundaryDraftPortName: controllerActions.setBoundaryDraftPortName,
    setBoundaryDraftPortType: controllerActions.setBoundaryDraftPortType,
    setBoundaryDraftDirection: controllerActions.setBoundaryDraftDirection,
    openGroupEditor: controllerActions.openGroupEditor,
    setGroupDraftName: controllerActions.setGroupDraftName,
    setGroupDraftSummary: controllerActions.setGroupDraftSummary,
    setGroupDraftColor: controllerActions.setGroupDraftColor,
    renamePipeline: controllerActions.renamePipeline,
    setPipelineAppearance: controllerActions.setPipelineAppearance,
    enterEmbeddedNode: controllerActions.enterEmbeddedNode,
    exitEmbedded: controllerActions.exitEmbedded,
    groupSelection: controllerActions.groupSelection,
    removeHostIoPort: controllerActions.removeHostIoPort,
    removePipelinePort: controllerActions.removePipelinePort,
    editPipelinePort: controllerActions.editPipelinePort,
    setPipelineInputValue: controllerActions.setPipelineInputValue,
    setPipelinePortConfig: controllerActions.setPipelinePortConfig,
    refreshPipelines: controllerActions.refreshPipelines,
    scheduleRegistryRefresh: controllerActions.scheduleRegistryRefresh,
    refreshCaptureDevices: controllerActions.refreshCaptureDevices,
    refreshIdeInfo: ide.refreshIdeInfo,
    refreshIdeProjects: ide.refreshIdeProjects,
    openIde: ide.openIde,
    openPluginInIde: ide.openPluginInIde,
    openPluginProjectModal: ide.openPluginProjectModal,
    closePluginProjectModal: ide.closePluginProjectModal,
    createPluginProject: ide.createPluginProject,
    resetRegistryFiltersHelper: controllerRegistryHelpers.resetFilters,
    refreshRegistryHelper: controllerRegistryHelpers.refresh
  };
}
