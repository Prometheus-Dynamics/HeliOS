import { get, writable } from 'svelte/store';
import { toaster } from '$lib';
import { listPipelineRegistry, validatePipelineGraph } from './controller/io';
import type {
  PipelineDataType,
  PipelineGraphPlan,
  PipelineNodeLayout,
  PipelineNodeValue,
  PipelineOverviewPipeline,
  PipelinePagePayload,
  PipelineTemplateSummary,
  PipelineTypeDescriptor
} from '$lib/types/pipeline';
import type { StreamInfo } from '$lib/api/client';
import type { PipelineGraphEdgeSelection } from '$lib';
import type { PipelineOutputEntry, PipelinePortEntry } from '../../components/pipelines/types';
import { applyPaletteToGraphPlan, serializeGraphPlan } from './graph';
import { organizePipelineGraph } from './organize';
import { buildRegistryVariants } from './registryUtils';
import { createPipelinesStore } from './pipelinesStore';
import {
  clonePipeline as clonePipelineModel,
  cloneRegistryEntry as cloneRegistryEntryModel,
  cloneGraphPlan,
  cloneDataType as cloneDataTypeModel,
  cloneNodeValue as cloneNodeValueModel,
  cloneNodeSyncConfig
} from './model';
import { getDataTypeVariants, resolveDataTypeKey } from './valueFormatting';
import {
  refreshPipelineIoCaches
} from './boundary';
import { generateNodeId } from './controller/utils';
import { buildPipelineInputEntries, buildPipelineOutputEntries } from './controller/planEntries';
import { normalizeDaedalusRegistry } from './controller/daedalusRegistry/normalization';
import { reportError } from '$lib/ui/errorPolicy';
import { createPipelineAutosaveManager } from './controller/autosave';
import {
  createPipelineMetricsManager
} from './controller/pipelineMetrics';
import { createPlanMutations, createRegistryEffects } from './controller/effects';
import { createPipelineDetailContext, createPipelineListState } from './controller/state';
import { createPipelinePayloadApplier } from './controller/pipelinePayload';
import { hydrateGraphWithRegistry } from './styleHydration';
import { createTypeHelpers } from './controller/typeHelpers';
import { createPlanSignature } from '$lib/components/flow/pipeline-graph/editorUtils';
import { createPipelineMetricsState, removePipelineMetricsState } from './controller/metrics';
import { createPipelineMutations } from './controller/mutations';
import { ensureHostIoNodeShape, findHostIoNodeId, resolveHostIoDirection } from './controller/hostIo';
import { createPipelineRegistryState, resolveRegistryEntryForBackendId } from './controller/registry';
import { createPipelineGraphContext } from './controller/graphContext';
import { createPipelineSelectionActions } from './controller/selection';
import { createPipelineValidationRuntime, createRuntimeDiagnosticsSubscription } from './controller/validation';
import { createPipelineAdminActions } from './controller/pipelineAdmin';

import type { InspectorTabKey } from './controller/types';

export type PipelineController = ReturnType<typeof createPipelineController>;
export type { InspectorTabKey } from './controller/types';

type PipelineControllerOptions = {
  saveOverride?: (pipelineId: string, plan: PipelineGraphPlan, name: string | null) => Promise<{ updatedAtMs?: number } | null>;
};

export function createPipelineController(initial: PipelinePagePayload, options: PipelineControllerOptions = {}) {
  const dataTypes = writable<Record<string, PipelineTypeDescriptor>>({});
  const registryState = createPipelineRegistryState();
  const {
    graphContextMenu,
    graphContextSearch,
    contextRegistryOptions,
    registry,
    registrySearch,
    registryTag,
    registryCategory,
    registryProvider,
    registrySort,
    registryView,
    activeRegistryGroup,
    registryLoading,
    registryError,
    availableTags,
    availableCategories,
    availableProviders,
    hasActiveRegistryFilters,
    visibleRegistryEntries,
    registryGroups,
    setGraphContextSearch,
    updateRegistryFilters,
    selectRegistryGroup,
    resetRegistryFilters
  } = registryState;
  const templates = writable<PipelineTemplateSummary[]>(initial.templates ?? []);
  const pipelineStore = createPipelinesStore();
  const pipelines = pipelineStore.list;

  const cloneDataType = (value: PipelineDataType): PipelineDataType =>
    cloneDataTypeModel(value, get(dataTypes));
  const cloneNodeValue = (value: PipelineNodeValue): PipelineNodeValue =>
    cloneNodeValueModel(value, get(dataTypes));
  const decoratePlan = (plan: PipelineGraphPlan): PipelineGraphPlan =>
    applyPaletteToGraphPlan(plan, get(dataTypes));
  const clonePlan = (plan: PipelineGraphPlan): PipelineGraphPlan =>
    cloneGraphPlan(plan, get(dataTypes));
  const clonePipeline = (pipeline: PipelineOverviewPipeline): PipelineOverviewPipeline =>
    clonePipelineModel(pipeline, get(dataTypes));
  const { buildDataTypeFromKey } = createTypeHelpers({ dataTypes });

  const resolveRegistryEntry = (backendId: string | null | undefined) =>
    resolveRegistryEntryForBackendId(backendId, get(registry));

  const pipelineMetricsState = createPipelineMetricsState();
  const selectedPipelineId = writable<string | null>(null);
  const editingPath = writable<string[]>([]);

  function replacePipeline(oldId: string, next: PipelineOverviewPipeline) {
    pipelineStore.remove(oldId);
    pipelineStore.upsert(next, 'start');
    saveState.update((state) => {
      const copy = { ...state };
      delete copy[oldId];
      copy[next.id] = 'idle';
      return copy;
    });
    dirtyState.update((state) => {
      const copy = { ...state };
      delete copy[oldId];
      return copy;
    });
    validationMessages.update((state) => {
      const copy = { ...state };
      delete copy[oldId];
      return copy;
    });
    removePipelineMetricsState(pipelineMetricsState, oldId);
    if (get(selectedPipelineId) === oldId) {
      selectedPipelineId.set(next.id);
    }
  }

  const pipelineSearch = writable('');
  const loadError = writable<string | null>(initial.errorMessage ?? null);
  const validationMessages = writable<Record<string, { ok: boolean; warnings: string[] }>>({});
  const saveState = writable<Record<string, 'idle' | 'saving' | 'error'>>({});
  const dirtyState = writable<Record<string, boolean>>({});
  const { applyPipelinePayload } = createPipelinePayloadApplier({
    dataTypes,
    registry,
    templates,
    dirtyState,
    pipelineStore,
    clonePipelineModel,
    cloneRegistryEntryModel,
    setLoadError: (message: string | null) => loadError.set(message)
  });

  applyPipelinePayload(initial, { preserveDirty: false });

  const graphSelection = writable<{ nodeId: string | null; nodes: string[]; edge: { id: string } | null }>({
    nodeId: null,
    nodes: [],
    edge: null
  });

  const createModalOpen = writable(false);
  const createName = writable('');
  const createMode = writable<'blank' | 'existing' | 'import' | 'template'>('blank');
  const createSourcePipelineId = writable<string | null>(initial.pipelines[0]?.id ?? null);
  const createSourceTemplateId = writable<string | null>(get(templates)[0]?.templateId ?? null);
  const createBusy = writable(false);
  const createError = writable<string | null>(null);

  const assignModalOpen = writable(false);
  const captureDevices = writable<StreamInfo[]>([]);
  const selectedCaptureSessionId = writable<string | null>(null);
  const assignBusy = writable(false);
  const assignError = writable<string | null>(null);

  const detachBusyMap = writable<Record<string, boolean>>({});

  const pipelineInputEntries = writable<PipelinePortEntry[]>([]);
  const pipelineOutputEntries = writable<PipelineOutputEntry[]>([]);
  const inspectorTab = writable<InspectorTabKey>('pipeline');

  const { markDirty, cancelPipelineAutosave, performPipelineSave } = createPipelineAutosaveManager({
    dirtyState,
    pipelines,
    decoratePlan,
    saveOverride: options.saveOverride,
    markSaving,
    updatePipeline,
    replacePipeline,
    setLoadError: (message: string | null) => loadError.set(message),
    getRegistryEntries: () => get(registry),
    validatePipeline: (pipelineId, plan) => {
      const current = get(selectedPipeline);
      if (!current || current.id !== pipelineId) return;
      void validateCurrentPipeline(plan, { quiet: true });
    }
  });

  const {
    summary,
    filteredPipelines,
    pipelineListItems,
    selectedPipeline,
    editingPlan,
    editingBreadcrumbs,
    dispose: disposePipelineListState
  } = createPipelineListState({
    pipelines,
    pipelineSearch,
    selectedPipelineId,
    editingPath
  });

  const { refreshPipelineMetrics, teardownPipelineMetrics } = createPipelineMetricsManager({
    pipelineMetricsState,
    pipelines,
    selectedPipeline
  });

  const detailContext = createPipelineDetailContext({
    selectedPipeline,
    dirtyState,
    saveState,
    validationMessages,
    pipelineInputEntries,
    pipelineOutputEntries,
    graphSelection,
    detachBusyMap,
    pipelineMetricsState
  });

  selectedPipeline.subscribe((pipeline) => {
    if (pipeline) {
      pipelineInputEntries.set(
        buildPipelineInputEntries(pipeline.graph, {
          cloneDataType,
          cloneNodeValue,
          cloneNodeSyncConfig: (value) => cloneNodeSyncConfig(value) ?? null
        })
      );
      pipelineOutputEntries.set(
        buildPipelineOutputEntries(pipeline.graph, {
          cloneDataType
        })
      );
    } else {
      pipelineInputEntries.set([]);
      pipelineOutputEntries.set([]);
      graphSelection.set({ nodeId: null, nodes: [], edge: null });
      graphContextMenu.set({ visible: false });
    }
  });

  const { refreshRegistry, scheduleRegistryRefresh, disposeRegistry } = createRegistryEffects({
    getDataTypes: () => get(dataTypes),
    registry,
    registryLoading,
    registryError,
    pipelineStore,
    getSelectedPipelineId: () => get(selectedPipelineId),
    decoratePlan,
    clonePlan,
    clonePipeline,
    cloneRegistryEntry: cloneRegistryEntryModel,
    hydrateGraphWithRegistry,
    listPipelineRegistry,
    normalizeDaedalusRegistry,
    buildRegistryVariants,
    toaster
  });

  // Registry is fetched on-demand (drawer/palette) to avoid blocking initial load.

  function markSaving(pipelineId: string, state: 'idle' | 'saving' | 'error') {
    saveState.update((map) => {
      if (map[pipelineId] === state) return map;
      return { ...map, [pipelineId]: state };
    });
  }

  function updatePipeline(pipelineId: string, mutator: (pipeline: PipelineOverviewPipeline) => void) {
    pipelineStore.update(pipelineId, (current) => {
      const clone = clonePipeline(current);
      mutator(clone);
      refreshPipelineIoCaches(clone.graph);
      return clone;
    });
  }

  const {
    clearValidation,
    validateCurrentPipeline,
    disposeValidation
  } = createPipelineValidationRuntime({
    selectedPipeline,
    pipelineStore,
    validationMessages,
    clonePipeline,
    toaster
  });

  const unsubscribeRuntimeDiagnostics = createRuntimeDiagnosticsSubscription({
    pipelineMetricsState,
    pipelineStore,
    clonePipeline
  });

  const pipelineNodeLayouts = new Map<string, PipelineNodeLayout>();
  const {
    setSelectedPipeline,
    updateGraphLayout,
    ensurePipelineGraph,
    reloadPipelineGraph,
    handleGraphSelect,
    enterEmbeddedNode,
    exitEmbedded,
    closeGraphContextMenu,
    isStubOverviewGraph
  } = createPipelineSelectionActions({
    pipelines,
    dirtyState,
    selectedPipeline,
    selectedPipelineId,
    editingPath,
    graphSelection,
    graphContextMenu,
    graphContextSearch,
    pipelineNodeLayouts,
    updatePipeline,
    markDirty,
    clonePlan,
    dataTypes,
    registry,
    loadError
  });

  const { updateCurrentPlan, handlePlanChange, organizeCurrentGraph } = createPlanMutations({
    getSelectedPipeline: () => get(selectedPipeline),
    getEditingPath: () => get(editingPath),
    getEditingPlan: () => get(editingPlan),
    updatePipeline,
    markDirty,
    clearValidation,
    clonePlan,
    refreshPipelineIoCaches,
    createPlanSignature,
    organizePipelineGraph,
    pipelineNodeLayouts,
    hydrateGraphWithRegistry,
    getRegistry: () => get(registry)
  });

  const {
    inferPortDataType,
    addNodeFromRegistry,
    groupSelection,
    removeGraphNode,
    ungroupSelection,
    removeGraphConnection,
    setGraphConnectionPolicy,
    setGraphConnectionStyle,
    addPipelinePort,
    addHostIoPort,
    removeHostIoPort,
    createBoundaryFromPort,
    editPipelinePort,
    removePipelinePort,
    setPipelineInputValue,
    setPipelinePortConfig,
    setNodeConstantValue,
    setNodeSyncConfig,
    setDaedalusNodeRuntime,
    addFanInPort,
    setNodeMetadata
  } = createPipelineMutations({
    registry,
    selectedPipeline,
    editingPlan,
    graphSelection,
    graphContextMenu,
    updateCurrentPlan,
    closeGraphContextMenu,
    cloneDataType,
    cloneNodeValue,
    cloneNodeSyncConfig,
    buildDataTypeFromKey,
    getDataTypeVariants,
    resolveDataTypeKey,
    resolveRegistryEntryForBackendId: resolveRegistryEntry,
    findHostIoNodeId,
    resolveHostIoDirection,
    ensureHostIoNodeShape,
    generateNodeId
  });

  const {
    openGraphContext,
    openRegistryPalette,
    openActionMenu,
    openBoundaryMenu,
    setBoundaryDraftPortName,
    setBoundaryDraftPortType,
    addBoundaryDraftPort,
    removeBoundaryDraftPort,
    setBoundaryDraftDirection,
    openBoundaryEditor,
    applyBoundaryDraft,
    openGroupEditor,
    setGroupDraftName,
    setGroupDraftSummary,
    setGroupDraftColor,
    applyGroupDraft
  } = createPipelineGraphContext({
    editingPlan,
    selectedPipeline,
    graphContextMenu,
    graphContextSearch,
    graphSelection,
    registryLoading,
    registryError,
    registry,
    scheduleRegistryRefresh,
    closeGraphContextMenu,
    resolveDataTypeKey,
    generateNodeId,
    inferPortDataType,
    resolveHostIoDirection,
    addPipelinePort,
    addHostIoPort,
    editPipelinePort,
    removePipelinePort,
    setNodeMetadata
  });

  async function saveCurrentPipeline() {
    const pipeline = get(selectedPipeline);
    if (!pipeline) return;
    cancelPipelineAutosave(pipeline.id);
    await performPipelineSave(pipeline.id);
  }

  const {
    openCreateModal,
    closeCreateModal,
    pipelineForSource,
    templateForSource,
    createPipeline,
    openAssignModal,
    closeAssignModal,
    refreshPipelines,
    refreshCaptureDevices,
    attachPipelineToDevice,
    detachAttachment,
    setPipelineAppearance,
    renamePipeline,
    setSelectedCaptureSession,
    setPipelineSearch
  } = createPipelineAdminActions({
    pipelines,
    templates,
    selectedPipeline,
    selectedPipelineId,
    pipelineSearch,
    loadError,
    dirtyState,
    createModalOpen,
    createName,
    createMode,
    createSourcePipelineId,
    createSourceTemplateId,
    createBusy,
    createError,
    assignModalOpen,
    captureDevices,
    selectedCaptureSessionId,
    assignBusy,
    assignError,
    getRegistryEntries: () => get(registry),
    getDataTypes: () => get(dataTypes),
    setAllPipelines: (next) => pipelineStore.setAll(next),
    updatePipeline,
    applyPipelinePayload,
    ensurePipelineGraph,
    reloadPipelineGraph,
    isStubOverviewGraph
  });

  function dispose() {
    disposeRegistry();
    registryState.dispose();
    disposePipelineListState();
    disposeValidation();
    teardownPipelineMetrics();
    unsubscribeRuntimeDiagnostics();
  }

	  const pipelineStores = {
	    pipelines,
	    summary,
	    selectedPipelineId,
	    pipelineSearch,
	    loadError,
    saveState,
    dirtyState,
    filteredPipelines,
    pipelineListItems,
    selectedPipeline,
    templates,
    dataTypes
  };

  const graphStores = {
    graphSelection,
    graphContextMenu,
    graphContextSearch,
    contextRegistryOptions,
    editingPath,
    editingPlan,
    editingBreadcrumbs
  };

  const inspectorStores = {
    detailContext,
    validationMessages,
    pipelineInputEntries,
    pipelineOutputEntries,
    tab: inspectorTab
  };

  const metricsStores = {
    state: pipelineMetricsState
  };

  const registryStores = {
    entries: registry,
    search: registrySearch,
    tag: registryTag,
    category: registryCategory,
    provider: registryProvider,
    sort: registrySort,
    view: registryView,
    activeGroup: activeRegistryGroup,
    loading: registryLoading,
    error: registryError,
    availableTags,
    availableCategories,
    availableProviders,
    hasActiveFilters: hasActiveRegistryFilters,
    groups: registryGroups,
    visibleEntries: visibleRegistryEntries
  };

	  const modalStores = {
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
      error: assignError,
      detachBusyMap
    }
  };

  const inspectorHelpers = {
    selectTab: (tab: InspectorTabKey) => inspectorTab.set(tab),
    reset: () => inspectorTab.set('pipeline')
  };

  const graphHelpers = {
    clearSelection: () => graphSelection.set({ nodeId: null, nodes: [], edge: null }),
    closeContextMenu: () => closeGraphContextMenu()
  };

  const registryHelpers = {
    resetFilters: () => resetRegistryFilters(),
    refresh: () => scheduleRegistryRefresh()
  };

  return {
    stores: {
      pipeline: pipelineStores,
      graph: graphStores,
      inspector: inspectorStores,
      metrics: metricsStores,
      registry: registryStores,
      modals: modalStores
    },
    actions: {
      setSelectedPipeline,
      setPipelineSearch,
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
      setGraphConnectionPolicy,
      setGraphConnectionStyle,
      addPipelinePort,
      addHostIoPort,
      createBoundaryFromPort,
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
      addFanInPort,
      setNodeMetadata,
      enterEmbeddedNode,
      exitEmbedded,
      groupSelection,
      ungroupSelection,
      saveCurrentPipeline,
      validateCurrentPipeline,
      clearValidation,
      openCreateModal,
      closeCreateModal,
      createPipeline,
      openAssignModal,
      closeAssignModal,
      attachPipelineToDevice,
      detachAttachment,
      setGraphContextSearch,
      setSelectedCaptureSession,
      updateRegistryFilters,
      selectRegistryGroup,
      resetRegistryFilters,
      refreshPipelines,
      refreshRegistry,
      scheduleRegistryRefresh,
      refreshCaptureDevices,
      refreshPipelineMetrics,
      reloadPipelineGraph,
      dispose
    },
    helpers: {
      inspector: inspectorHelpers,
      graph: graphHelpers,
      registry: registryHelpers
    }
  };
}
