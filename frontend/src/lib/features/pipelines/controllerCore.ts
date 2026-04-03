import { browser } from '$app/environment';
import { get, writable } from 'svelte/store';
import { toaster } from '$lib';
import { StreamsApi } from '$lib/api/streamsApi';
import {
  fetchPipelineTemplate,
  listCaptureBackends,
  listPipelineRegistry,
  uploadPipelineGraph,
  validatePipelineGraph
} from './controller/io';
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
import { fetchPipelinePagePayload } from '$lib/api/pipelinesPayload';
import { createPipelinesStore } from './pipelinesStore';
import {
  clonePipeline as clonePipelineModel,
  cloneRegistryEntry as cloneRegistryEntryModel,
  cloneGraphPlan,
  cloneDataType as cloneDataTypeModel,
  cloneNodeValue as cloneNodeValueModel,
  cloneNodeSyncConfig
} from './model';
import { fromApiGraphPlan } from './graphConverters';
import { getDataTypeVariants, resolveDataTypeKey } from './valueFormatting';
import {
  refreshPipelineIoCaches
} from './boundary';
import { describeError, generateNodeId } from './controller/utils';
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
  let pipelineRefreshPromise: Promise<void> | null = null;

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

	  function openCreateModal() {
	    createModalOpen.set(true);
	    createName.set('');
	    createMode.set('blank');
	    createSourcePipelineId.set(get(pipelines)[0]?.id ?? null);
      createSourceTemplateId.set(get(templates)[0]?.templateId ?? null);
	    createError.set(null);
	  }

  function closeCreateModal() {
    if (get(createBusy)) return;
    createModalOpen.set(false);
  }

  function pipelineForSource(): PipelineOverviewPipeline | null {
    if (get(createMode) !== 'existing') return null;
    const sourceId = get(createSourcePipelineId);
    if (!sourceId) return null;
    return get(pipelines).find((pipeline) => pipeline.id === sourceId) ?? null;
  }

  function templateForSource(): PipelineTemplateSummary | null {
    if (get(createMode) !== 'template') return null;
    const sourceId = get(createSourceTemplateId);
    if (!sourceId) return null;
    return get(templates).find((template) => template.templateId === sourceId) ?? null;
  }

  async function createPipeline() {
    if (get(createMode) === 'import') {
      return;
    }
    const name = get(createName).trim();
    const requestedName = name.length ? name : null;
	    createBusy.set(true);
	    createError.set(null);
	    try {
	      const mode = get(createMode);
	      let baseGraph: unknown = { nodes: [], edges: [], metadata: {} };
	      if (mode === 'existing') {
	        const source = pipelineForSource();
	        if (!source) {
          throw new Error('Source pipeline not found.');
        }
        baseGraph = serializeGraphPlan(source.graph, { minimal: true });
      }
      if (mode === 'template') {
        const source = templateForSource();
        if (!source) {
          throw new Error('Template not found.');
        }
        const templateDoc = await fetchPipelineTemplate(source.templateId);
        baseGraph = templateDoc.graph ?? { nodes: [], edges: [], metadata: {} };
      }

      const response = await uploadPipelineGraph(baseGraph, requestedName);
      const graphPlan = fromApiGraphPlan(response.graph ?? {});
      hydrateGraphWithRegistry(graphPlan, get(registry));
      const decorated = applyPaletteToGraphPlan(graphPlan, get(dataTypes));
      refreshPipelineIoCaches(decorated);
      const createdAt = Math.floor((response.updated_at_ms ?? Date.now()) / 1000);
      const responseName = typeof response?.name === 'string' && response.name.trim().length ? response.name.trim() : null;
      const displayName = responseName ?? requestedName ?? 'Pipeline';
      const pipeline: PipelineOverviewPipeline = {
        id: response.id,
        name: displayName,
        alias: displayName,
        status: 'draft',
        revision: response.updated_at_ms ? String(response.updated_at_ms) : null,
        planHash: null,
        createdAt,
        updatedAt: createdAt,
        graph: decorated,
        attachments: [],
        diagnostics: null
      };
      const next = [...get(pipelines), pipeline].sort((a, b) => a.name.localeCompare(b.name));
      pipelineStore.setAll(next);
      selectedPipelineId.set(pipeline.id);
      createModalOpen.set(false);
      toaster.success({
        title: 'Pipeline created',
        description: `${pipeline.name} is ready to edit.`
      });
    } catch (error) {
      console.error('Failed to create pipeline', error);
      createError.set(describeError(error));
    } finally {
      createBusy.set(false);
    }
  }

  function openAssignModal() {
    assignError.set(null);
    selectedCaptureSessionId.set(null);
    assignModalOpen.set(true);
    void refreshCaptureDevices();
  }

  function closeAssignModal() {
    if (get(assignBusy)) return;
    assignModalOpen.set(false);
  }

  // Template/external pipeline creation routes through `PipelinesApi.uploadGraph`.

  function runPipelineRefresh(options: { preserveDirty?: boolean } = {}) {
    if (!pipelineRefreshPromise) {
      pipelineRefreshPromise = refreshPipelineOverview(options).finally(() => {
        pipelineRefreshPromise = null;
      });
    }
    return pipelineRefreshPromise;
  }

  async function refreshPipelineOverview(options: { preserveDirty?: boolean } = {}) {
    if (!browser) {
      return;
    }
    try {
      const preserveDirty = options.preserveDirty ?? true;
      const previousPipelinesById = new Map(get(pipelines).map((pipeline) => [pipeline.id, pipeline]));
      const previousDirtyState: Record<string, boolean> = preserveDirty ? get(dirtyState) : {};
      const payload = await fetchPipelinePagePayload();
      applyPipelinePayload(payload, { preserveDirty });
      const activePipelineId = get(selectedPipelineId);
      if (activePipelineId) {
        const previousPipeline = previousPipelinesById.get(activePipelineId) ?? null;
        const payloadPipeline =
          payload.pipelines.find((pipeline) => pipeline.id === activePipelineId) ?? null;
        const revisionChanged = (previousPipeline?.revision ?? null) !== (payloadPipeline?.revision ?? null);
        const shouldReloadSelectedPipeline =
          preserveDirty &&
          !previousDirtyState[activePipelineId] &&
          Boolean(previousPipeline && !isStubOverviewGraph(previousPipeline.graph)) &&
          Boolean(payloadPipeline && isStubOverviewGraph(payloadPipeline.graph)) &&
          revisionChanged;
        if (shouldReloadSelectedPipeline) {
          void reloadPipelineGraph(activePipelineId);
        } else {
          void ensurePipelineGraph(activePipelineId);
        }
      }
    } catch (error) {
      console.error('Failed to refresh pipeline overview', error);
      const message = describeError(error);
      loadError.set(message);
    }
  }

  function refreshPipelines(options: { preserveDirty?: boolean } = {}) {
    return runPipelineRefresh(options);
  }

  async function refreshCaptureDevices() {
    if (!get(selectedPipeline)) return;
    assignBusy.set(true);
    try {
      const response = await listCaptureBackends();
      captureDevices.set(Array.isArray(response) ? response : []);
    } catch (error) {
      console.error('Failed to fetch capture devices', error);
      const message = describeError(error);
      assignError.set(message);
      toaster.error({
        title: 'Failed to load capture devices',
        description: message
      });
    } finally {
      assignBusy.set(false);
    }
  }

  async function attachPipelineToDevice() {
    const pipeline = get(selectedPipeline);
    const streamId = get(selectedCaptureSessionId);
    if (!pipeline || !streamId) return;
    if (get(assignBusy)) return;
    assignBusy.set(true);
    assignError.set(null);
    try {
      const graph = serializeGraphPlan(pipeline.graph);
      await StreamsApi.setPipelineGraph({
        id: streamId,
        requestBody: { graph, pipeline_id: pipeline.id, output: null }
      });
      const smoke = await StreamsApi.smokePipelineGraph({ id: streamId, timeoutMs: 1500 });
      if (!smoke.ok) {
        const message = Array.isArray(smoke.errors) && smoke.errors.length ? smoke.errors.join('\n') : 'Pipeline runtime error.';
        reportError({
          title: 'Pipeline runtime error',
          error: new Error(message),
          fallback: message
        });
      }
      toaster.success({
        title: 'Pipeline applied',
        description: `Assigned ${pipeline.name} to stream ${streamId.slice(0, 8)}`
      });
      try {
        const updatedStreams = await listCaptureBackends();
        captureDevices.set(Array.isArray(updatedStreams) ? updatedStreams : []);
      } catch (error) {
        console.error('Failed to refresh capture devices after assignment', error);
      }
      try {
        await refreshPipelineOverview({ preserveDirty: true });
      } catch (error) {
        console.error('Failed to refresh pipelines after assignment', error);
      }
      assignModalOpen.set(false);
      selectedCaptureSessionId.set(null);
    } catch (error) {
      console.error('Failed to assign pipeline', error);
      const message = describeError(error);
      assignError.set(message);
      reportError({
        title: 'Failed to assign pipeline',
        error,
        fallback: message
      });
    } finally {
      assignBusy.set(false);
    }
  }

  async function detachAttachment() {
    toaster.error({
      title: 'Not supported',
      description: 'Detaching pipelines is disabled with the new pipeline API.'
    });
    return;
  }

  async function setPipelineAppearance(pipelineId: string, payload: { icon?: string; color?: string }) {
    if (!pipelineId) return;
    updatePipeline(pipelineId, (next) => {
      const appearance = { ...(next.appearance ?? {}) };
      if (payload.icon !== undefined) {
        appearance.icon = payload.icon ?? null;
      }
      if (payload.color !== undefined) {
        appearance.color = payload.color ?? null;
      }
      next.appearance = appearance;
    });
  }

  const slugifyPipelineAlias = (value: string): string => {
    const lower = value.toLowerCase();
    let slug = '';
    let pendingDash = false;
    for (const char of lower) {
      if ((char >= 'a' && char <= 'z') || (char >= '0' && char <= '9')) {
        if (pendingDash && slug.length > 0) {
          slug += '-';
        }
        pendingDash = false;
        slug += char;
      } else {
        pendingDash = true;
      }
    }
    slug = slug.replace(/^-+|-+$/g, '');
    if (!slug) {
      const random = typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function' ? crypto.randomUUID().slice(0, 8) : Math.random().toString(16).slice(2, 10);
      return `pipeline-${random}`;
    }
    return slug;
  };

  async function renamePipeline(pipelineId: string, payload: { name?: string; alias?: string }) {
    if (!pipelineId) return;
    const trimmedName = payload.name?.trim();
    const trimmedAlias = payload.alias?.trim();
    if (!trimmedName && !trimmedAlias) {
      return;
    }
    const normalizedAlias = trimmedAlias ? slugifyPipelineAlias(trimmedAlias) : '';
    updatePipeline(pipelineId, (next) => {
      if (trimmedName) {
        next.name = trimmedName;
      }
      if (payload.alias !== undefined) {
        next.alias = normalizedAlias;
      }
    });
    toaster.success({
      title: 'Pipeline updated',
      description: payload.alias !== undefined && trimmedName ? 'Name and alias saved.' : trimmedName ? 'Name saved.' : 'Alias saved.'
    });
  }

  function setSelectedCaptureSession(sessionId: string | null) {
    selectedCaptureSessionId.set(sessionId);
  }

  function setPipelineSearch(value: string) {
    pipelineSearch.set(value);
  }

  function dispose() {
    disposeRegistry();
    registryState.dispose();
    disposePipelineListState();
    disposeValidation();
    pipelineRefreshPromise = null;
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
