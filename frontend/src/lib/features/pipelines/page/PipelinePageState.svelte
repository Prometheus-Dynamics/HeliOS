<script lang="ts">

  import { browser } from '$app/environment';
  import { onDestroy, onMount, untrack, type Snippet } from 'svelte';
  import { derived, get, writable } from 'svelte/store';
  import { toaster, OpenAPI } from '$lib';
  import { subscribeDomainInvalidations } from '$lib/api/invalidation';
  import { scheduleAfterPaint, scheduleWhenIdle } from '$lib/utils/browserSchedule';
  import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';
  import { PipelinesApi } from '$lib/api/pipelinesApi';
  import { realtimeUpdateMatchesKind, type RealtimeUpdateEvent } from '$lib/api/realtimeUpdates';
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
    DEFAULT_PIPELINE_ICON_ID,
    defaultColorForPipeline,
    defaultIconIdForPipeline,
    resolvePipelineIconOption
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
  import { createPipelineIdeState } from './pipelineIdeState';
  import { setupPipelineRegistryState } from './pipelineRegistryState';
  import { setupPipelineGraphState } from './pipelineGraphState';
  import PipelineTuneState from '$lib/features/pipelines/page/PipelineTuneState.svelte';
  import {
    isPipelineNodeValue,
    isRecord,
    numberFromMetadata,
    portMetadataFromFlatKeys
  } from './pipelineTuneState';
  import {
    extractInputValues,
    isDaedalusPlan,
    extractTuneConstantEntries as extractTuneConstantEntriesBase,
    portMetadataForConstant as portMetadataForConstantBase,
    safeClonePlan
  } from './pipelineTuneConstantUtils';

  const {
    data,
    initialPipelineId = null,
    children: routeChildren
  } = $props<{ data: PageData; initialPipelineId?: string | null; children?: Snippet<[ { ctx: Record<string, unknown> } ]> }>();
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
  const {
    pipeline: pipelineStores,
    graph: graphStores,
    inspector: inspectorStores,
    registry: registryStores,
    modals: modalStores
  } = controller.stores;
  type DetailPanelHandle = {
    focusOnNode?: (nodeId: string, options?: { port?: string | null }) => void;
  };
  let detailPanelRef = $state<DetailPanelHandle | null>(null);

  let PipelineDetailPanelComponent = $state<
    (typeof import('$lib/components/pipelines/PipelineDetailPanel.svelte'))['default'] | null
  >(null);
  let PipelineListPanelComponent = $state<
    (typeof import('$lib/features/pipelines/page/PipelineListPanel.svelte'))['default'] | null
  >(null);
  let PipelineInspectorPanelComponent = $state<
    (typeof import('$lib/features/pipelines/page/PipelineInspectorPanel.svelte'))['default'] | null
  >(null);
  let PipelineModalsComponent = $state<
    (typeof import('$lib/features/pipelines/page/PipelineModals.svelte'))['default'] | null
  >(null);
  let PipelineGraphWorkspaceComponent = $state<
    (typeof import('$lib/features/pipelines/page/PipelineGraphWorkspace.svelte'))['default'] | null
  >(null);
  let PipelineGraphContextMenuComponent = $state<
    (typeof import('$lib/features/pipelines/page/PipelineGraphContextMenu.svelte'))['default'] | null
  >(null);
  let PipelineTunePanelComponent = $state<
    (typeof import('$lib/features/pipelines/page/PipelineTunePanel.svelte'))['default'] | null
  >(null);

  const componentLoads: Partial<Record<
    | 'detail'
    | 'list'
    | 'inspector'
    | 'modals'
    | 'graph-workspace'
    | 'graph-context'
    | 'tune',
    Promise<void>
  >> = {};

  function loadComponentOnce(
    key: 'detail' | 'list' | 'inspector' | 'modals' | 'graph-workspace' | 'graph-context' | 'tune',
    loader: () => Promise<void>
  ): Promise<void> {
    const inFlight = componentLoads[key];
    if (inFlight) {
      return inFlight;
    }
    const next = loader().finally(() => {
      componentLoads[key] = undefined;
    });
    componentLoads[key] = next;
    return next;
  }

  async function loadPipelineDetailPanel(): Promise<void> {
    if (!browser || PipelineDetailPanelComponent) return;
    await loadComponentOnce('detail', async () => {
      const module = await import('$lib/components/pipelines/PipelineDetailPanel.svelte');
      PipelineDetailPanelComponent = module.default;
    });
  }

  async function loadPipelineShellComponents(): Promise<void> {
    await Promise.all([
      PipelineListPanelComponent
        ? Promise.resolve()
        : loadComponentOnce('list', async () => {
            const module = await import('$lib/features/pipelines/page/PipelineListPanel.svelte');
            PipelineListPanelComponent = module.default;
          }),
      PipelineInspectorPanelComponent
        ? Promise.resolve()
        : loadComponentOnce('inspector', async () => {
            const module = await import('$lib/features/pipelines/page/PipelineInspectorPanel.svelte');
            PipelineInspectorPanelComponent = module.default;
          })
    ]);
  }

  async function loadPipelineGraphComponents(): Promise<void> {
    await Promise.all([
      PipelineGraphWorkspaceComponent
        ? Promise.resolve()
        : loadComponentOnce('graph-workspace', async () => {
            const module = await import('$lib/features/pipelines/page/PipelineGraphWorkspace.svelte');
            PipelineGraphWorkspaceComponent = module.default;
          }),
      PipelineGraphContextMenuComponent
        ? Promise.resolve()
        : loadComponentOnce('graph-context', async () => {
            const module = await import('$lib/features/pipelines/page/PipelineGraphContextMenu.svelte');
            PipelineGraphContextMenuComponent = module.default;
          }),
      loadPipelineDetailPanel()
    ]);
  }

  async function loadPipelineTunePanel(): Promise<void> {
    if (PipelineTunePanelComponent) return;
    await loadComponentOnce('tune', async () => {
      const module = await import('$lib/features/pipelines/page/PipelineTunePanel.svelte');
      PipelineTunePanelComponent = module.default;
    });
  }

  async function loadPipelineModals(): Promise<void> {
    if (PipelineModalsComponent) return;
    await loadComponentOnce('modals', async () => {
      const module = await import('$lib/features/pipelines/page/PipelineModals.svelte');
      PipelineModalsComponent = module.default;
    });
  }
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

  let importInput = $state<HTMLInputElement | null>(null);
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
  const graphBindings = $state({
    get detailPanelRef() {
      return detailPanelRef;
    },
    set detailPanelRef(value: DetailPanelHandle | null) {
      detailPanelRef = value;
    }
  });
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
  const PIPELINE_FOCUS_REQUEST_KEY = 'helios.pipelines.focus_request';
  const LIVE_UPDATES_REFRESH_DEBOUNCE_MS = 300;
  type PipelineFocusRequest = { pipelineId: string; nodeId?: string | null; port?: string | null };
  let pendingPipelineFocus = $state<PipelineFocusRequest | null>(null);
  let stopLiveUpdates: (() => void) | null = null;
  let liveUpdatesRefreshHandle: number | null = null;
  let cancelPipelineUiBoot: (() => void) | null = null;
  let cancelPipelineBootstrapRefresh: (() => void) | null = null;
  let cancelPipelineIdleWarmup: (() => void) | null = null;
  let cancelPipelineGraphWarmup: (() => void) | null = null;
  let cancelPipelineTuneWarmup: (() => void) | null = null;
  let initialSelectionApplied = $state(false);

  function graphPlanHasWorkspaceData(plan: PipelineGraphPlan | null | undefined): boolean {
    if (!plan || typeof plan !== 'object') return false;
    if (Object.keys(plan.nodes ?? {}).length > 0) return true;
    if ((plan.connections?.length ?? 0) > 0) return true;
    if (Object.keys(plan.pipelineInputs ?? {}).length > 0) return true;
    if (Object.keys(plan.pipelineOutputs ?? {}).length > 0) return true;
    return false;
  }

  function shouldApplyLiveUpdate(event: RealtimeUpdateEvent): boolean {
    if (
      event.path.startsWith('/v1/pipelines') ||
      event.path.startsWith('/v1/streams') ||
      event.path.startsWith('/v1/ws/pipelines') ||
      event.path.startsWith('/v1/ws/streams')
    ) {
      return true;
    }
    if (realtimeUpdateMatchesKind(event, 'api')) {
      return false;
    }
    return (
      realtimeUpdateMatchesKind(event, 'pipelines') ||
      realtimeUpdateMatchesKind(event, 'streams') ||
      realtimeUpdateMatchesKind(event, 'device') ||
      realtimeUpdateMatchesKind(event, 'settings')
    );
  }

  function scheduleLiveUpdatesRefresh(): void {
    if (!browser) return;
    if (liveUpdatesRefreshHandle != null) return;
    liveUpdatesRefreshHandle = window.setTimeout(() => {
      liveUpdatesRefreshHandle = null;
      if (document.hidden) return;
      void loadPipelineOverview({ preserveDirty: true });
      void refreshCaptureDevices();
    }, LIVE_UPDATES_REFRESH_DEBOUNCE_MS);
  }

  function consumePipelineFocusRequest(): void {
    if (!browser) return;
    const raw = window.sessionStorage.getItem(PIPELINE_FOCUS_REQUEST_KEY);
    if (!raw) return;
    window.sessionStorage.removeItem(PIPELINE_FOCUS_REQUEST_KEY);
    try {
      const parsed = JSON.parse(raw) as PipelineFocusRequest;
      if (!parsed?.pipelineId) return;
      pendingPipelineFocus = parsed;
      setActiveTab('pipeline');
      setSelectedPipeline(parsed.pipelineId);
    } catch {
      // ignore
    }
  }

  onMount(() => {
    if (!browser) return;
    consumePipelineFocusRequest();
  });

  $effect(() => {
    const req = pendingPipelineFocus;
    if (!req || !$selectedPipeline || $selectedPipeline.id !== req.pipelineId) return;
    const nodeId = req.nodeId ?? null;
    if (!nodeId) {
      pendingPipelineFocus = null;
      return;
    }
    const detailPanel = detailPanelRef;
    if (typeof detailPanel?.focusOnNode !== 'function') return;
    detailPanel.focusOnNode(nodeId, { port: req.port ?? null });
    pendingPipelineFocus = null;
  });

  $effect(() => {
    if (initialSelectionApplied) return;
    const pipelineId = typeof initialPipelineId === 'string' ? initialPipelineId.trim() : '';
    if (!pipelineId) {
      initialSelectionApplied = true;
      return;
    }
    if ($selectedPipelineId === pipelineId) {
      initialSelectionApplied = true;
      return;
    }
    setSelectedPipeline(pipelineId);
    initialSelectionApplied = true;
  });

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

  const loadStreamCapabilities = async (): Promise<void> => {
    try {
      const capabilities = await StreamsApi.streamCapabilities();
      const normalized = String(capabilities?.rawPipelineId ?? '').trim().toLowerCase();
      if (normalized.length) {
        rawStreamPipelineUuid = normalized;
      }
    } catch {
      // Keep previously loaded IDs; avoid local hardcoded fallback IDs.
    }
  };

  let iconModalOpen = $state(false);
  let iconModalPipelineId = $state<string | null>(null);
  let iconModalIconId = $state<string>(DEFAULT_PIPELINE_ICON_ID);
  let iconModalColor = $state<string>(DEFAULT_PIPELINE_COLOR);
  let iconModalSaving = $state(false);
  let iconModalError = $state<string | null>(null);
  const modalBindings = $state({
    get iconModalIconId() {
      return iconModalIconId;
    },
    set iconModalIconId(value: string) {
      iconModalIconId = value;
    },
    get iconModalColor() {
      return iconModalColor;
    },
    set iconModalColor(value: string) {
      iconModalColor = value;
    },
    get importInput() {
      return importInput;
    },
    set importInput(value: HTMLInputElement | null) {
      importInput = value;
    }
  });

  function organizeGraphNodes() {
    organizeCurrentGraph();
  }

  function openPipelineIconModal(pipelineId: string) {
    const pipeline = $pipelines.find((entry) => entry.id === pipelineId);
    const appearance = pipeline?.appearance ?? null;
    iconModalPipelineId = pipelineId;
    const selectedIconId = appearance?.icon ?? defaultIconIdForPipeline(pipelineId);
    const resolvedIcon = resolvePipelineIconOption(selectedIconId);
    iconModalIconId = resolvedIcon.id;
    iconModalColor = appearance?.color ?? defaultColorForPipeline(pipelineId);
    iconModalError = null;
    iconModalSaving = false;
    iconModalOpen = true;
  }

  function closePipelineIconModal() {
    iconModalOpen = false;
    iconModalPipelineId = null;
    iconModalError = null;
    iconModalSaving = false;
  }

  async function savePipelineIconSelection() {
    if (!iconModalPipelineId) return;
    iconModalSaving = true;
    iconModalError = null;
    try {
      const pipelineLabel = pipelineLabelById(iconModalPipelineId);
      await setPipelineAppearance(iconModalPipelineId, {
        icon: iconModalIconId,
        color: iconModalColor
      });
      closePipelineIconModal();
      toaster.success({
        title: 'Appearance updated',
        description: pipelineLabel
      });
    } catch (error) {
      iconModalError = buildErrorMessage({ error, fallback: 'Failed to update appearance' });
    } finally {
      iconModalSaving = false;
    }
  }

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
      iconModalOpen = false;
      deleteModalOpen.set(false);
      closeAssignModal();
      closeGraphContextMenu();
    }
  }

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
    importInput?.click();
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
    cancelPipelineUiBoot = scheduleAfterPaint(() => {
      pipelineUiReady = true;
    }, 1);
    if (hasInitialPipelineData) {
      cancelPipelineBootstrapRefresh = scheduleWhenIdle(() => {
        if (document.hidden) return;
        void loadStreamCapabilities();
      }, { timeoutMs: 1800, fallbackMs: 700 });
    } else {
      cancelPipelineBootstrapRefresh = scheduleAfterPaint(() => {
        void loadStreamCapabilities();
        void loadPipelineOverview({ bootstrap: true, preserveDirty: false });
      }, 2);
    }
    stopLiveUpdates = subscribeDomainInvalidations(
      ['pipelines', 'streams', 'device', 'settings'],
      (event) => {
        if (document.hidden) return;
        if (!shouldApplyLiveUpdate(event)) return;
        scheduleLiveUpdatesRefresh();
      },
      { debounceMs: LIVE_UPDATES_REFRESH_DEBOUNCE_MS }
    );
    cancelPipelineIdleWarmup = scheduleWhenIdle(() => {
      scheduleRegistryRefresh();
      void loadPipelineModals();
    }, { timeoutMs: 2000, fallbackMs: 1200 });
  });

  $effect(() => {
    if (!pipelineUiReady) return;
    void loadPipelineShellComponents();
  });

  $effect(() => {
    cancelPipelineGraphWarmup?.();
    cancelPipelineGraphWarmup = null;
    cancelPipelineTuneWarmup?.();
    cancelPipelineTuneWarmup = null;
    if (!pipelineUiReady) return;
    if ($activeTab === 'pipeline') {
      if (!$selectedPipeline) return;
      cancelPipelineGraphWarmup = scheduleAfterPaint(() => {
        void loadPipelineGraphComponents();
      }, 1);
      return;
    }
    if (!$selectedPipeline) return;
    cancelPipelineTuneWarmup = scheduleAfterPaint(() => {
      void loadPipelineTunePanel();
    }, 1);
  });

  $effect(() => {
    if (
      $registryDrawerOpen ||
      iconModalOpen ||
      ideState.pluginProjectModalOpen ||
      $createModalOpen ||
      $deleteModalOpen ||
      $assignModalOpen
    ) {
      void loadPipelineModals();
    }
  });

  function handlePipelineCardKeydown(event: KeyboardEvent, pipelineId: string) {
    if (event.key !== 'Enter' && event.key !== ' ') {
      return;
    }
    event.preventDefault();
    setSelectedPipeline(pipelineId);
  }

  onDestroy(() => {
    cancelPipelineUiBoot?.();
    cancelPipelineUiBoot = null;
    cancelPipelineBootstrapRefresh?.();
    cancelPipelineBootstrapRefresh = null;
    cancelPipelineIdleWarmup?.();
    cancelPipelineIdleWarmup = null;
    cancelPipelineGraphWarmup?.();
    cancelPipelineGraphWarmup = null;
    cancelPipelineTuneWarmup?.();
    cancelPipelineTuneWarmup = null;
    stopLiveUpdates?.();
    stopLiveUpdates = null;
    if (liveUpdatesRefreshHandle != null) {
      clearTimeout(liveUpdatesRefreshHandle);
      liveUpdatesRefreshHandle = null;
    }
    disposePageStore();
  });

  const baseCtx = $derived.by(() => ({
    IDE_ENABLED_FALLBACK: ideState.IDE_ENABLED_FALLBACK,
    IDE_PORT_FALLBACK: ideState.IDE_PORT_FALLBACK,
    IDE_PROJECTS_DIR_FALLBACK: ideState.IDE_PROJECTS_DIR_FALLBACK,
    IDE_WORKSPACE_DIR_FALLBACK: ideState.IDE_WORKSPACE_DIR_FALLBACK,
    OpenAPI,
    PIPELINE_EXPORT_VERSION,
    PIPELINE_FOCUS_REQUEST_KEY,
    PipelineDetailPanelComponent,
    PipelineGraphContextMenu: PipelineGraphContextMenuComponent,
    PipelineGraphWorkspace: PipelineGraphWorkspaceComponent,
    PipelineInspectorPanel: PipelineInspectorPanelComponent,
    PipelineListPanel: PipelineListPanelComponent,
    PipelineModals: PipelineModalsComponent,
    PipelineTunePanel: PipelineTunePanelComponent,
    PipelinesApi,
    RAW_STREAM_PIPELINE_ID,
    RAW_STREAM_PIPELINE_UUID: rawStreamPipelineUuid,
    SUPPORTED_PIPELINE_EXPORT_VERSIONS,
    StreamsApi,
    accessBadgeClass,
    accessLabel,
    applyPaletteToGraphPlan,
    browser,
    buildDaedalusGraphPatch,
    buildErrorMessage,
    clampNumber,
    closeDeleteModal,
    closePipelineIconModal,
    closePluginProjectModal,
    collectPipelineOutputs,
    confirmDeletePipeline,
    consumePipelineFocusRequest,
    createPipelinePageStore,
    createPluginProject,
    createRegistryResolver,
    customNodeSearch: ideState.customNodeSearch,
    ideBindings,
    deleteModalBusy,
    deleteModalError,
    deleteModalOpen,
    deleteModalPipeline,
    deletePipelineById,
    derived,
    describePortType,
    detailPanelRef,
    graphBindings,
    graphContextMenu,
    graphContextSearch,
    emptyPipelineGraphPlan,
    exportCurrentPipeline,
    extractInputValues,
    fromApiGraphPlan,
    get,
    getStreamUpdatesSocket,
    graphContextMenuElement: graphState.graphContextMenuElement,
    graphSelection,
    groupSelection,
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
    handlePipelineCardKeydown,
    handlePipelineImport,
    handlePipelinePortAdd,
    handlePipelinePortConfig,
    handlePipelinePortEdit,
    handlePipelinePortRemove,
    handlePipelinePortValue,
    handlePipelineRename,
    handlePipelineValidate,
    iconModalColor,
    iconModalError,
    iconModalIconId,
    iconModalOpen,
    iconModalPipelineId,
    iconModalSaving,
    ideEnabled: ideState.ideEnabled,
    ideIframeUrl: ideState.ideIframeUrl,
    idePort: ideState.idePort,
    ideProjects: ideState.ideProjects,
    ideProjectsDir: ideState.ideProjectsDir,
    ideUrl: ideState.ideUrl,
    ideWorkspaceDir: ideState.ideWorkspaceDir,
    importInput,
    modalBindings,
    initial,
    isDaedalusPlan,
    isInitialLoading,
    isPipelineNodeValue,
    isRecord,
    loadPipelineDetailPanel,
    loadPipelineOverview,
    numberFromMetadata,
    onDestroy,
    onMount,
    openActionMenu,
    openBoundaryEditor,
    openBoundaryMenu,
    openDeleteModal,
    openIde,
    openPipelineIconModal,
    openRegistryPalette,
    openGroupEditor,
    openPluginInIde,
    openPluginProjectModal,
    organizeGraphNodes,
    outputOptionsForPipeline,
    pageStore,
    pendingPipelineFocus,
    pipelineForSource,
    pipelineLabelById,
    pipelineMap,
    pipelineRefreshCount,
    pipelineUpdatesReady,
    pipelinesBootstrapped,
    pipelinesRefreshing,
    pluginProjectBusy: ideState.pluginProjectBusy,
    pluginProjectError: ideState.pluginProjectError,
    pluginProjectLanguage: ideState.pluginProjectLanguage,
    pluginProjectModalOpen: ideState.pluginProjectModalOpen,
    pluginProjectName: ideState.pluginProjectName,
    portMetadataForConstant,
    portMetadataFromFlatKeys,
    refreshIdeInfo,
    refreshIdeProjects,
    refreshPipelineIoCaches,
    reportError,
    resolveRegistryEntryForNode,
    safeClonePlan,
    savePipelineIconSelection,
    serializeGraphPlan,
    setActiveTab,
    setBoundaryDraftDirection,
    setBoundaryDraftPortName,
    setBoundaryDraftPortType,
    setGraphContextMenuElement,
    setGroupDraftColor,
    setGroupDraftName,
    setGroupDraftSummary,
    streamGraphForPipeline,
    streamLabel,
    streamUpdatesReadyById,
    streamUsesPipeline,
    templateForSource,
    toaster,
    triggerPipelineImport,
    addNodeFromRegistry,
    addBoundaryDraftPort,
    applyBoundaryDraft,
    applyGroupDraft,
    closeGraphContextMenu,
    contextRegistryOptions,
    removeBoundaryDraftPort,
    removeGraphConnection,
    removeGraphNode,
    ungroupSelection,
    activeTab,
    registryDrawerOpen,
    pipelines,
    pipelineListItems,
    selectedPipelineId,
    pipelineSearch,
    loadError,
    dataTypes,
    selectedPipeline,
    templates,
    detailContext,
    editingPlan,
    editingBreadcrumbs,
    pipelineInputEntries,
    pipelineOutputEntries,
    inspectorTab,
    captureDevices,
    registry,
    registryLoading,
    registryError,
    registrySort,
    registryView,
    registryStores,
    registryHelpers,
    setSelectedPipeline,
    openAssignModal,
    closeAssignModal,
    attachPipelineToDevice,
    saveCurrentPipeline,
    clearValidation,
    setNodeConstantValue,
    handlePlanChange,
    refreshPipelineMetrics,
    setNodeSyncConfig,
    setDaedalusNodeRuntime,
    setNodeMetadata,
    closeCreateModal,
    createPipeline,
    updateRegistryFilters,
    selectRegistryGroup,
    resetRegistryFilters,
    refreshRegistry,
    formatPipelineValue,
    setSelectedCaptureSession,
    createModalOpen,
    createMode,
    createName,
    createSourcePipelineId,
    createSourceTemplateId,
    createBusy,
    createError,
    assignModalOpen,
    assignError,
    assignBusy,
    selectedCaptureSessionId,
    openCreateModal,
    visiblePlugins,
  }));
</script>


<PipelineTuneState
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
    {@const ctx = { ...baseCtx, ...tune }}
    {@render routeChildren?.({ ctx })}
  {/snippet}
</PipelineTuneState>
