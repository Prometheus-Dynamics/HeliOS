<script lang="ts">
  import { browser } from '$app/environment';
  import { onDestroy, onMount, untrack } from 'svelte';
  import { derived, get, writable } from 'svelte/store';
  import { toaster, OpenAPI } from '$lib';
  import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';
  import { PipelinesApi } from '$lib/api/pipelinesApi';
  import { loadOwnedStreamCapabilities } from '$lib/api/streamResources';
  import { StreamsApi } from '$lib/api/streamsApi';
  import { createPipelinePageStore } from '$lib/features/pipelines/pageStore';
  import type { PageData } from '../../../../routes/pipelines/$types';
  import type {
    PipelineDataType,
    PipelineGraphPlan,
    PipelineOverviewPipeline,
  } from '$lib/types/pipeline';
  import { serializeGraphPlan, applyPaletteToGraphPlan, emptyPipelineGraphPlan } from '$lib/features/pipelines/graph';
  import { buildDaedalusGraphPatch } from '$lib/features/pipelines/daedalusGraph';
  import { collectPipelineOutputs } from '$lib/features/pipelines/boundary';
  import { refreshPipelineIoCaches } from '$lib/features/pipelines/boundary';
  import { fromApiGraphPlan } from '$lib/features/pipelines/graphConverters';
  import { createRegistryResolver } from '$lib/components/flow/pipeline-graph/registry';
  import {
    DEFAULT_PIPELINE_COLOR,
    DEFAULT_PIPELINE_ICON_ID
  } from '$lib/features/pipelines/iconCatalog';
  import type { PipelinePagePayload } from '$lib/types/pipeline';
  import {
    buildNodeValueFromInput,
    formatPipelineValue,
    getDataTypeVariants,
    resolveDataTypeKey
  } from '$lib/features/pipelines/valueFormatting';
  import { accessBadgeClass, accessLabel, clampNumber } from './pipelineStreamControlUtils';
  import { createPipelineCrudActions } from './pipelinePageCrud';
  import { createPipelineGraphHandlers } from './pipelinePageGraphHandlers';
  import { createPipelineImportExport, PIPELINE_EXPORT_VERSION, SUPPORTED_PIPELINE_EXPORT_VERSIONS } from './pipelineImportExport';
  import { createPipelinePageHelpers } from './pipelinePageHelpers';
  import { createPipelineIdeState } from './pipelineIdeState.svelte';
  import { setupPipelineRegistryState } from './pipelineRegistryState.svelte';
  import { setupPipelineGraphState } from './pipelineGraphState.svelte';
  import { createPipelinePageViewState } from './pipelinePageViewState.svelte';
  import PipelineTuneWorkspace from '$lib/features/pipelines/page/PipelineTuneWorkspace.svelte';
  import {
    isPipelineNodeValue,
    isRecord,
    numberFromMetadata,
    portMetadataFromFlatKeys
  } from './pipelineTuneHelpers';
  import {
    extractInputValues,
    isDaedalusPlan,
    extractTuneConstantEntries as extractTuneConstantEntriesBase,
    portMetadataForConstant as portMetadataForConstantBase,
    safeClonePlan
  } from './pipelineTuneConstantUtils';
  import {
    PIPELINE_FOCUS_REQUEST_KEY,
    createPipelinePageComponentLoaders,
    createPipelinePageIconModalSupport,
    createPipelinePageRuntime,
    graphPlanHasWorkspaceData,
    handlePipelineCardKeydown as handlePipelineCardKeydownSupport
  } from './pipelinePageSupport';
  import { createPipelinePageBaseContext } from './pipelinePageContext';
  import type { PipelinePageBaseContext, PipelinePageRouteChildrenSnippet } from './pipelinePageTypes';

  const {
    data,
    initialPipelineId = null,
    children: routeChildren
  } = $props<{ data: PageData; initialPipelineId?: string | null; children?: PipelinePageRouteChildrenSnippet }>();
  const initial = untrack(() => data as PipelinePagePayload);
  const hasInitialPipelineData = Boolean(
    initial.registry.length ||
      initial.templates.length ||
      Object.keys(initial.dataTypes ?? {}).length ||
      initial.pipelines.some((pipeline) => graphPlanHasWorkspaceData(pipeline.graph))
  );

  const pageStore = untrack(() => createPipelinePageStore(initial));
  const { controller, pipelineUpdates, activeTab, registryDrawerOpen, dispose: disposePageStore } = pageStore;
  const pipelineUpdatesReady = pipelineUpdates.pipelineReady;
  const streamUpdatesReadyById = pipelineUpdates.streamReadyById;
  const getStreamUpdatesSocket = pipelineUpdates.getStreamUpdatesSocket;
  const { pipeline: pipelineStores, graph: graphStores, inspector: inspectorStores, registry: registryStores, modals: modalStores } =
    controller.stores;
  const pageViewState = createPipelinePageViewState(browser);
  const {
    loadPipelineDetailPanel,
    loadPipelineShellComponents,
    loadPipelineGraphComponents,
    loadPipelineTunePanel,
    loadPipelineModals
  } = pageViewState;
  const { registry: registryHelpers } = controller.helpers;

  const RAW_STREAM_PIPELINE_ID = '__raw__';
  let rawStreamPipelineUuid = $state('');

  const {
    pipelineLabelById,
    pipelineForSource,
    templateForSource,
    streamUsesPipeline,
    streamLabel,
    streamGraphForPipeline,
    outputOptionsForPipeline
  } = createPipelinePageHelpers({
    getPipelines: () => $pipelines,
    getTemplates: () => $templates,
    getCreateMode: () => $createMode,
    getCreateSourcePipelineId: () => $createSourcePipelineId,
    getCreateSourceTemplateId: () => $createSourceTemplateId,
    collectPipelineOutputs,
    isDaedalusPlan,
    RAW_STREAM_PIPELINE_ID
  });

  const {
    pipelines,
    selectedPipelineId,
    pipelineSearch,
    loadError,
    saveState,
    dirtyState,
    dataTypes,
    pipelineListItems,
    selectedPipeline,
    templates
  } = pipelineStores;
  const pipelineMap = derived(pipelines, ($pipelines) =>
    Object.fromEntries($pipelines.map((pipeline) => [pipeline.id, pipeline])) as Record<string, PipelineOverviewPipeline>
  );

  const {
    detailContext,
    validationMessages,
    pipelineInputEntries,
    pipelineOutputEntries,
    tab: inspectorTab
  } = inspectorStores;

  const {
    graphSelection,
    graphContextMenu,
    graphContextSearch,
    contextRegistryOptions,
    editingPlan,
    editingBreadcrumbs
  } = graphStores;

  const {
    entries: registry,
    sort: registrySort,
    view: registryView,
    loading: registryLoading,
    error: registryError,
  } = registryStores;

  const resolveRegistryEntryForNode = $derived(createRegistryResolver($registry));
  const portMetadataForConstant = (
    node: PipelineGraphPlan['nodes'][string] | undefined,
    portKey: string
  ) => portMetadataForConstantBase(node, portKey, resolveRegistryEntryForNode);
  const extractTuneConstantEntries = (plan: PipelineGraphPlan) =>
    extractTuneConstantEntriesBase(plan, resolveRegistryEntryForNode);

	  const {
	    create: {
	      modalOpen: createModalOpen,
	      name: createName,
	      mode: createMode,
	      sourcePipelineId: createSourcePipelineId,
        sourceTemplateId: createSourceTemplateId,
	      busy: createBusy,
	      error: createError
	    },
    assign: {
      modalOpen: assignModalOpen,
      captureDevices,
      selectedCaptureSessionId,
      busy: assignBusy,
      error: assignError
    }
  } = modalStores;

  const {
    setSelectedPipeline,
    handlePlanChange,
    organizeCurrentGraph,
    handleGraphSelect,
    updateGraphLayout,
    openGraphContext,
    openRegistryPalette,
    openActionMenu,
    openBoundaryMenu,
    openBoundaryEditor,
    setBoundaryDraftPortName,
    setBoundaryDraftPortType,
    addBoundaryDraftPort,
    removeBoundaryDraftPort,
    setBoundaryDraftDirection,
    applyBoundaryDraft,
    openGroupEditor,
    setGroupDraftName,
    setGroupDraftSummary,
    setGroupDraftColor,
    applyGroupDraft,
    closeGraphContextMenu,
    addNodeFromRegistry,
    removeGraphNode,
    removeGraphConnection,
    groupSelection,
    ungroupSelection,
    setGraphConnectionPolicy,
    setGraphConnectionStyle,
    addPipelinePort,
    addHostIoPort,
    removeHostIoPort,
    removePipelinePort,
    editPipelinePort,
    setPipelineInputValue,
    setPipelinePortConfig,
    renamePipeline,
    setPipelineAppearance,
    setNodeConstantValue,
    setNodeSyncConfig,
    setDaedalusNodeRuntime,
    enterEmbeddedNode,
    exitEmbedded,
    saveCurrentPipeline,
    clearValidation,
    openCreateModal,
    closeCreateModal,
    createPipeline,
    openAssignModal,
    closeAssignModal,
    attachPipelineToDevice,
    setSelectedCaptureSession,
    updateRegistryFilters,
    selectRegistryGroup,
    resetRegistryFilters,
    refreshRegistry,
    refreshPipelines,
    scheduleRegistryRefresh,
    refreshPipelineMetrics,
    refreshCaptureDevices,
    setNodeMetadata
  } = controller.actions;

  const { validateCurrentPipeline } = controller.actions;

  let pipelineRefreshCount = $state(0);
  let pipelinesBootstrapped = $state(hasInitialPipelineData);
  let pipelineUiReady = $state(false);
  const pipelinesRefreshing = $derived(pipelineRefreshCount > 0);
  const isInitialLoading = $derived(!pipelineUiReady || (!pipelinesBootstrapped && pipelinesRefreshing));

  const {
    state: ideState,
    visiblePlugins,
    refreshIdeInfo,
    refreshIdeProjects,
    openPluginProjectModal,
    closePluginProjectModal,
    createPluginProject,
    openIde,
    openPluginInIde
  } = createPipelineIdeState();
  const ideBindings = $state({
    get customNodeSearch() {
      return ideState.customNodeSearch;
    },
    set customNodeSearch(value: string) {
      ideState.customNodeSearch = value;
    },
    get pluginProjectName() {
      return ideState.pluginProjectName;
    },
    set pluginProjectName(value: string) {
      ideState.pluginProjectName = value;
    },
    get pluginProjectLanguage() {
      return ideState.pluginProjectLanguage;
    },
    set pluginProjectLanguage(value: string) {
      ideState.pluginProjectLanguage = value;
    }
  });
  const graphBindings = pageViewState.graphBindings;
  const graphState = setupPipelineGraphState({
    graphContextMenu,
    graphSelection,
    closeGraphContextMenu,
    removeGraphNode,
    removeGraphConnection
  });
  const setGraphContextMenuElement = (element: HTMLDivElement | null) => {
    graphState.graphContextMenuElement = element;
  };
  const deleteModalOpen = writable(false);
  const deleteModalPipeline = writable<PipelineOverviewPipeline | null>(null);
  const deleteModalBusy = writable(false);
  const deleteModalError = writable<string | null>(null);
  let pendingPipelineFocus = $state<{
    pipelineId: string;
    nodeId?: string | null;
    port?: string | null;
  } | null>(null);

  async function loadPipelineOverview(options: { bootstrap?: boolean; preserveDirty?: boolean } = {}): Promise<void> {
    const { bootstrap = false, preserveDirty = true } = options;
    pipelineRefreshCount = pipelineRefreshCount + 1;
    try {
      await refreshPipelines({ preserveDirty });
      loadError.set(null);
    } finally {
      pipelineRefreshCount = Math.max(0, pipelineRefreshCount - 1);
      if (bootstrap || !pipelinesBootstrapped) {
        pipelinesBootstrapped = true;
      }
    }
  }

  async function loadStreamCapabilities(): Promise<void> {
    try {
      const capabilities = await loadOwnedStreamCapabilities();
      const normalized = String(capabilities?.rawPipelineId ?? '').trim().toLowerCase();
      if (normalized.length) {
        rawStreamPipelineUuid = normalized;
      }
    } catch {
      // Keep previously loaded IDs; avoid local hardcoded fallback IDs.
    }
  }

  const modalBindings = pageViewState.modalBindings;

  function organizeGraphNodes() {
    organizeCurrentGraph();
  }

  const { openPipelineIconModal, closePipelineIconModal, savePipelineIconSelection } =
    createPipelinePageIconModalSupport({
      getPipelines: () => $pipelines,
      pipelineLabelById,
      setPipelineAppearance,
      buildErrorMessage,
      onSaveSuccess: (pipelineLabel) => {
        toaster.success({
          title: 'Appearance updated',
          description: pipelineLabel
        });
      },
      getIconModalOpen: () => pageViewState.iconModalOpen,
      setIconModalOpen: (open) => {
        pageViewState.iconModalOpen = open;
      },
      getIconModalPipelineId: () => pageViewState.iconModalPipelineId,
      setIconModalPipelineId: (pipelineId) => {
        pageViewState.iconModalPipelineId = pipelineId;
      },
      getIconModalIconId: () => pageViewState.iconModalIconId,
      setIconModalIconId: (iconId) => {
        pageViewState.iconModalIconId = iconId;
      },
      getIconModalColor: () => pageViewState.iconModalColor,
      setIconModalColor: (color) => {
        pageViewState.iconModalColor = color;
      },
      setIconModalSaving: (saving) => {
        pageViewState.iconModalSaving = saving;
      },
      setIconModalError: (message) => {
        pageViewState.iconModalError = message;
      }
    });

  const {
    deletePipelineById,
    openDeleteModal,
    closeDeleteModal,
    confirmDeletePipeline,
    handlePipelineRename
  } = createPipelineCrudActions({
    getPipelines: () => $pipelines,
    getSelectedPipeline: () => $selectedPipeline ?? null,
    setSelectedPipeline,
    loadPipelineOverview,
    renamePipeline,
    PipelinesApi,
    toaster,
    reportError,
    dirtyState,
    saveState,
    validationMessages,
    getDeleteModalPipeline: () => get(deleteModalPipeline),
    setDeleteModalPipeline: (pipeline) => {
      deleteModalPipeline.set(pipeline);
    },
    getDeleteModalOpen: () => get(deleteModalOpen),
    setDeleteModalOpen: (open) => {
      deleteModalOpen.set(open);
    },
    getDeleteModalBusy: () => get(deleteModalBusy),
    setDeleteModalBusy: (busy) => {
      deleteModalBusy.set(busy);
    },
    getDeleteModalError: () => get(deleteModalError),
    setDeleteModalError: (message) => {
      deleteModalError.set(message);
    }
  });

  const {
    handlePanelPlanChange,
    handlePanelGraphSelect,
    handlePipelineValidate,
    handlePanelGraphContext,
    handlePanelGraphLayout,
    handleEnterEmbedded,
    handleExitEmbedded,
    handlePipelinePortAdd,
    handleHostIoPortAdd,
    handlePipelinePortRemove,
    handleHostIoPortRemove,
    handlePipelinePortEdit,
    handlePipelinePortValue,
    handlePipelinePortConfig,
    handleNodeConstantValue,
    handleEdgePolicy,
    handleEdgeStyle
  } = createPipelineGraphHandlers({
    handlePlanChange,
    handleGraphSelect,
    validateCurrentPipeline,
    getEditingPlan: () => $editingPlan ?? null,
    getSelectedPipelineGraph: () => $selectedPipeline?.graph ?? null,
    openGraphContext,
    updateGraphLayout,
    enterEmbeddedNode,
    exitEmbedded,
    addPipelinePort,
    addHostIoPort,
    removePipelinePort,
    removeHostIoPort,
    editPipelinePort,
    setPipelineInputValue,
    setPipelinePortConfig,
    setNodeConstantValue,
    setGraphConnectionPolicy,
    setGraphConnectionStyle,
    toaster
  });

  function setActiveTab(tab: 'pipeline' | 'tune') {
    activeTab.set(tab);
    if (tab !== 'pipeline') {
      registryDrawerOpen.set(false);
      pageViewState.iconModalOpen = false;
      deleteModalOpen.set(false);
      closeAssignModal();
      closeGraphContextMenu();
    }
  }

  const pipelinePageRuntime = createPipelinePageRuntime({
    browser,
    hasInitialPipelineData,
    getInitialPipelineId: () => initialPipelineId,
    getSelectedPipelineId: () => $selectedPipelineId ?? null,
    getSelectedPipeline: () => $selectedPipeline ?? null,
    setSelectedPipeline,
    setActiveTab,
    getPendingPipelineFocus: () => pendingPipelineFocus,
    setPendingPipelineFocus: (request) => {
      pendingPipelineFocus = request;
    },
    getDetailPanelRef: () => pageViewState.detailPanelRef,
    loadPipelineOverview,
    loadStreamCapabilities,
    refreshCaptureDevices,
    scheduleRegistryRefresh,
    loadPipelineModals,
    loadPipelineShellComponents,
    loadPipelineGraphComponents,
    loadPipelineTunePanel,
    getPipelineUiReady: () => pipelineUiReady,
    setPipelineUiReady: (ready) => {
      pipelineUiReady = ready;
    },
    getActiveTab: () => $activeTab,
    getRegistryDrawerOpen: () => $registryDrawerOpen,
    getIconModalOpen: () => pageViewState.iconModalOpen,
    getPluginProjectModalOpen: () => ideState.pluginProjectModalOpen,
    getCreateModalOpen: () => $createModalOpen,
    getDeleteModalOpen: () => $deleteModalOpen,
    getAssignModalOpen: () => $assignModalOpen,
    disposePageStore
  });

  setupPipelineRegistryState({
    activeTab,
    registryDrawerOpen,
    registry,
    registryLoading,
    registryError,
    scheduleRegistryRefresh
  });

  function describePortType(value: PipelineDataType): string {
    if (typeof value === 'string') return value;
    const label = value.label ?? value.descriptor?.label ?? null;
    const kind = value.kind ?? value.descriptor?.id ?? null;
    if (label && kind && label !== kind) {
      return `${label} (${kind})`;
    }
    return label ?? kind ?? 'Unknown';
  }

  function triggerPipelineImport() {
    pageViewState.importInput?.click();
  }

  const { handlePipelineImport, exportCurrentPipeline } = createPipelineImportExport({
    PipelinesApi,
    applyPaletteToGraphPlan,
    fromApiGraphPlan,
    serializeGraphPlan,
    refreshPipelineIoCaches,
    loadPipelineOverview,
    setSelectedPipeline,
    closeCreateModal,
    reportError,
    toaster,
    getDataTypes: () => $dataTypes,
    getSelectedPipeline: () => $selectedPipeline ?? null,
    setLoadError: (value) => {
      loadError.set(value);
    },
    updateDirtyState: (updater) => {
      dirtyState.update(updater);
    },
    updateSaveState: (updater) => {
      saveState.update(updater);
    },
    updateValidationMessages: (updater) => {
      validationMessages.update(updater);
    }
  });

  onMount(() => {
    pipelinePageRuntime.mount();
  });

  $effect(() => {
    pipelinePageRuntime.syncPendingPipelineFocus();
  });

  $effect(() => {
    pipelinePageRuntime.syncInitialPipelineSelection();
  });

  $effect(() => {
    pipelinePageRuntime.warmShellComponents();
  });

  $effect(() => {
    pipelinePageRuntime.warmActiveTabComponents();
  });

  $effect(() => {
    pipelinePageRuntime.warmModalComponents();
  });

  const handlePipelineCardKeydown = (event: KeyboardEvent, pipelineId: string) =>
    handlePipelineCardKeydownSupport(event, pipelineId, setSelectedPipeline);

  onDestroy(() => {
    pipelinePageRuntime.destroy();
  });

  const baseCtx = $derived.by<PipelinePageBaseContext>(() =>
    createPipelinePageBaseContext({
      PipelineDetailPanelComponent: pageViewState.PipelineDetailPanelComponent,
      PipelineGraphContextMenu: pageViewState.PipelineGraphContextMenuComponent,
      PipelineGraphWorkspace: pageViewState.PipelineGraphWorkspaceComponent,
      PipelineInspectorPanel: pageViewState.PipelineInspectorPanelComponent,
      PipelineListPanel: pageViewState.PipelineListPanelComponent,
      PipelineModals: pageViewState.PipelineModalsComponent,
      PipelineTunePanel: pageViewState.PipelineTunePanelComponent,
      activeTab,
      assignBusy,
      assignError,
      assignModalOpen,
      captureDevices,
      closeAssignModal,
      closeCreateModal,
      closeDeleteModal,
      closePipelineIconModal,
      closePluginProjectModal,
      confirmDeletePipeline,
      createBusy,
      createError,
      createModalOpen,
      createMode,
      createName,
      createPipeline,
      createPluginProject,
      createSourcePipelineId,
      createSourceTemplateId,
      dataTypes: dataTypes as PipelinePageBaseContext['dataTypes'],
      deleteModalBusy,
      deleteModalError,
      deleteModalOpen,
      deleteModalPipeline,
      detailContext: detailContext as PipelinePageBaseContext['detailContext'],
      editingBreadcrumbs,
      editingPlan,
      handlePipelineCardKeydown,
      handlePipelineImport,
      iconModalError: pageViewState.iconModalError,
      iconModalOpen: pageViewState.iconModalOpen,
      iconModalPipelineId: pageViewState.iconModalPipelineId,
      iconModalSaving: pageViewState.iconModalSaving,
      ideBindings: {
        customNodeSearch: ideState.customNodeSearch,
        pluginProjectLanguage: ideState.pluginProjectLanguage,
        pluginProjectName: ideState.pluginProjectName
      },
      inspectorTab,
      isInitialLoading,
      loadError,
      modalBindings: {
        iconModalColor: pageViewState.iconModalColor,
        iconModalIconId: pageViewState.iconModalIconId,
        importInput: pageViewState.importInput
      },
      openCreateModal,
      openDeleteModal,
      openPipelineIconModal,
      pipelineForSource,
      pipelineInputEntries: pipelineInputEntries as PipelinePageBaseContext['pipelineInputEntries'],
      pipelineLabelById,
      pipelineListItems,
      pipelineMap,
      pipelineOutputEntries: pipelineOutputEntries as PipelinePageBaseContext['pipelineOutputEntries'],
      pipelineSearch,
      pipelines,
      pipelinesRefreshing,
      pluginProjectBusy: ideState.pluginProjectBusy,
      pluginProjectError: ideState.pluginProjectError,
      pluginProjectModalOpen: ideState.pluginProjectModalOpen,
      refreshRegistry,
      registry: registryStores.entries,
      registryDrawerOpen,
      registryError: registryStores.error,
      registryLoading: registryStores.loading,
      registrySort: registryStores.sort,
      registryStores,
      registryView: registryStores.view,
      resetRegistryFilters,
      savePipelineIconSelection,
      selectRegistryGroup,
      selectedCaptureSessionId,
      selectedPipeline,
      selectedPipelineId,
      setSelectedCaptureSession,
      setSelectedPipeline,
      templateForSource,
      templates,
      triggerPipelineImport,
      updateRegistryFilters,
      addNodeFromRegistry,
      attachPipelineToDevice
    })
  );
</script>
<PipelineTuneWorkspace
  {activeTab}
  {pipelineUpdates}
  {pipelineUpdatesReady}
  {streamUpdatesReadyById}
  {getStreamUpdatesSocket}
  {selectedPipeline}
  {editingPlan}
  {detailContext}
  {pipelines}
  {pipelineLabelById}
  {streamUsesPipeline}
  {streamLabel}
  {streamGraphForPipeline}
  {outputOptionsForPipeline}
  {handlePlanChange}
  {setNodeConstantValue}
  {saveCurrentPipeline}
  {openAssignModal}
  {assignBusy}
  {refreshPipelineMetrics}
  {resolveDataTypeKey}
  {getDataTypeVariants}
  {resolveRegistryEntryForNode}
  {buildNodeValueFromInput}
  {extractTuneConstantEntries}
  {RAW_STREAM_PIPELINE_ID}
  RAW_STREAM_PIPELINE_UUID={rawStreamPipelineUuid}
  {PipelinesApi}
  {StreamsApi}
>
  {#snippet children({ tune })}
    {@render routeChildren?.({ base: baseCtx, tune })}
  {/snippet}
</PipelineTuneWorkspace>
