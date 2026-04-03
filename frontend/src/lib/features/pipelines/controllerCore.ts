import { browser } from '$app/environment';
import { get, writable } from 'svelte/store';
import { toaster } from '$lib';
import { StreamsApi } from '$lib/api/streamsApi';
import {
  fetchPipelineGraph,
  fetchPipelineTemplate,
  listCaptureBackends,
  listPipelineRegistry,
  uploadPipelineGraph,
  validatePipelineGraph
} from './controller/io';
import type {
  PipelineDataType,
  PipelineGraphPlan,
  PipelineDiagnosticWarning,
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
import { createPipelineValidationWorker } from '$lib/workers/factories';
import { getDataTypeVariants, resolveDataTypeKey } from './valueFormatting';
import {
  refreshPipelineIoCaches
} from './boundary';
import { describeError, truncatedPlanHashFromRevision, generateNodeId } from './controller/utils';
import { buildPipelineInputEntries, buildPipelineOutputEntries } from './controller/planEntries';
import { normalizeDaedalusRegistry } from './controller/daedalusRegistry/normalization';
import { reportError } from '$lib/ui/errorPolicy';
import { createPipelineAutosaveManager } from './controller/autosave';
import {
  createPipelineMetricsManager,
  type PipelineMetricsState
} from './controller/pipelineMetrics';
import { createPlanMutations, createRegistryEffects } from './controller/effects';
import { createPipelineDetailContext, createPipelineListState } from './controller/state';
import { createPipelinePayloadApplier } from './controller/pipelinePayload';
import { hydrateGraphWithRegistry } from './styleHydration';
import { createTypeHelpers } from './controller/typeHelpers';
import { createPlanSignature } from '$lib/components/flow/pipeline-graph/editorUtils';
import { resolveNodeOrder } from './daedalusGraph';
import { createPipelineMetricsState, removePipelineMetricsState } from './controller/metrics';
import { createPipelineMutations } from './controller/mutations';
import { ensureHostIoNodeShape, findHostIoNodeId, resolveHostIoDirection } from './controller/hostIo';
import { createPipelineRegistryState, resolveRegistryEntryForBackendId } from './controller/registry';
import { createPipelineGraphContext } from './controller/graphContext';

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
  let validationWorker: Worker | null = null;
  let validationRequestId = 0;
  const validationResolvers = new Map<number, (warnings: string[]) => void>();

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

  const RUNTIME_WARNING_PREFIX = 'RuntimeError:';
  const runtimeDiagnosticsSignature = new Map<string, string>();

  const buildRuntimeDiagnosticWarnings = (state: PipelineMetricsState | null | undefined): PipelineDiagnosticWarning[] => {
    const snapshots = state?.metrics ?? null;
    if (!snapshots || snapshots.length === 0) return [];
    const out: PipelineDiagnosticWarning[] = [];
    const seen = new Set<string>();

    for (const snapshot of snapshots) {
      const streamLabelRaw = String(snapshot?.streamPath ?? snapshot?.streamId ?? '').trim();
      const streamLabel = streamLabelRaw || 'unknown_stream';

      const pushWarning = (nodeIdRaw: string, errorRaw: string) => {
        const nodeId = String(nodeIdRaw ?? '').trim();
        const error = String(errorRaw ?? '').trim();
        if (!nodeId || !error) return;
        const message = `${RUNTIME_WARNING_PREFIX} ${streamLabel}: ${error}`;
        const key = `${streamLabel}::${nodeId}::${error}`;
        if (seen.has(key)) return;
        seen.add(key);
        out.push({ message, nodeId });
      };

      for (const [nodeId, metrics] of Object.entries(snapshot?.metrics ?? {})) {
        const err = metrics?.lastError ?? null;
        if (typeof err === 'string' && err.trim()) {
          pushWarning(nodeId, err);
        }
      }
      for (const [nodeId, metrics] of Object.entries(snapshot?.groups ?? {})) {
        const err = metrics?.lastError ?? null;
        if (typeof err === 'string' && err.trim()) {
          pushWarning(nodeId, err);
        }
      }
    }

    return out;
  };

  const isRuntimeDiagnosticWarning = (warning: PipelineDiagnosticWarning | null | undefined): boolean => {
    const message = typeof warning?.message === 'string' ? warning.message : '';
    return message.startsWith(RUNTIME_WARNING_PREFIX);
  };

  const mergeRuntimeDiagnostics = (
    existing: PipelineOverviewPipeline['diagnostics'] | null | undefined,
    runtimeWarnings: PipelineDiagnosticWarning[],
  ): PipelineOverviewPipeline['diagnostics'] | null => {
    const baseWarnings = (existing?.warnings ?? []).filter((warning) => !isRuntimeDiagnosticWarning(warning));
    const mergedWarnings = [...baseWarnings, ...runtimeWarnings];
    const error = existing?.error ?? null;
    if (mergedWarnings.length === 0 && !error) {
      return null;
    }
    return {
      warnings: mergedWarnings,
      ...(error ? { error } : {}),
    };
  };

  const unsubscribeRuntimeDiagnostics = pipelineMetricsState.subscribe(($metrics) => {
    for (const [pipelineId, state] of Object.entries($metrics ?? {})) {
      // Ignore teardown resets (idle/null/never-updated) so runtime warnings on the
      // pipeline list are not cleared just because selection changed.
      const isTeardownReset =
        state?.status === 'idle' &&
        !state?.metrics &&
        !state?.error &&
        state?.updatedAt == null;
      if (isTeardownReset) {
        continue;
      }
      const runtimeWarnings = buildRuntimeDiagnosticWarnings(state);
      const signature = runtimeWarnings
        .map((w) => `${w.nodeId ?? ''}|${w.port ?? ''}|${w.message}`)
        .sort()
        .join('\n');
      if (runtimeDiagnosticsSignature.get(pipelineId) === signature) {
        continue;
      }
      runtimeDiagnosticsSignature.set(pipelineId, signature);
      pipelineStore.update(pipelineId, (current) => {
        const clone = clonePipeline(current);
        clone.diagnostics = mergeRuntimeDiagnostics(clone.diagnostics ?? null, runtimeWarnings);
        return clone;
      });
    }
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

  // helper functions moved to controller submodules

  const graphLoadCache = new Map<string, Promise<void>>();
  const pipelineNodeLayouts = new Map<string, PipelineNodeLayout>();
  function setSelectedPipeline(id: string | null) {
    selectedPipelineId.set(id);
    editingPath.set([]);
    graphSelection.set({ nodeId: null, nodes: [], edge: null });
    if (id) {
      void ensurePipelineGraph(id);
    }
  }

  function markSaving(pipelineId: string, state: 'idle' | 'saving' | 'error') {
    saveState.update((map) => {
      if (map[pipelineId] === state) return map;
      return { ...map, [pipelineId]: state };
    });
  }

  function clearValidation(pipelineId: string) {
    validationMessages.update((map) => {
      if (!map[pipelineId]) return map;
      const next = { ...map };
      delete next[pipelineId];
      return next;
    });
    pipelineStore.update(pipelineId, (pipeline) => {
      const clone = clonePipeline(pipeline);
      clone.diagnostics = null;
      return clone;
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

  function updateGraphLayout(pipelineId: string | null | undefined, layout: PipelineNodeLayout | undefined) {
    if (!pipelineId) return;
    const pipeline = get(pipelines).find((entry) => entry.id === pipelineId);
    if (!pipeline) return;
    const planNodeIds = new Set(Object.keys(pipeline.graph?.nodes ?? {}));
    if (planNodeIds.size === 0 || !layout) {
      pipelineNodeLayouts.delete(pipelineId);
      return;
    }
    const normalizedEntries = Object.entries(layout)
      .filter(([nodeId]) => planNodeIds.has(nodeId))
      .map<[string, PipelineNodeLayout[string]]>(([nodeId, dimensions]) => {
        const width =
          typeof dimensions?.width === 'number' && Number.isFinite(dimensions.width) ? dimensions.width : undefined;
        const height =
          typeof dimensions?.height === 'number' && Number.isFinite(dimensions.height) ? dimensions.height : undefined;
        const result: PipelineNodeLayout[string] = {};
        if (width !== undefined) result.width = width;
        if (height !== undefined) result.height = height;
        return [nodeId, result];
      })
      .filter(([, dims]) => Object.keys(dims).length > 0);

    if (normalizedEntries.length === 0) {
      pipelineNodeLayouts.delete(pipelineId);
      return;
    }

    pipelineNodeLayouts.set(pipelineId, Object.fromEntries(normalizedEntries));
  }

  function hasPopulatedPorts(plan: PipelineGraphPlan): boolean {
    return Object.values(plan.nodes ?? {}).some((node) => {
      const inputs = Object.keys(node.inputs ?? {});
      const outputs = Object.keys(node.outputs ?? {});
      return inputs.length > 0 || outputs.length > 0;
    });
  }

  function isStubOverviewGraph(plan: PipelineGraphPlan | null | undefined): boolean {
    if (!plan) return true;
    const nodes = plan.nodes ?? {};
    const connections = plan.connections ?? [];
    const pipelineInputs = plan.pipelineInputs ?? {};
    const pipelineOutputs = plan.pipelineOutputs ?? {};
    return (
      Object.keys(nodes).length === 0 &&
      connections.length === 0 &&
      Object.keys(pipelineInputs).length === 0 &&
      Object.keys(pipelineOutputs).length === 0
    );
  }

  function ensurePipelineGraph(pipelineId: string): Promise<void> | undefined {
    const pipeline = get(pipelines).find((entry) => entry.id === pipelineId);
    if (!pipeline) return undefined;

    if (get(dirtyState)[pipelineId]) {
      return undefined;
    }

    const hasNodes = Object.keys(pipeline.graph?.nodes ?? {}).length > 0;
    const hasPorts = hasNodes && hasPopulatedPorts(pipeline.graph);
    if (hasPorts) {
      return undefined;
    }

    const existing = graphLoadCache.get(pipelineId);
    if (existing) {
      return existing;
    }

    const promise = (async () => {
      try {
        const snapshot = await fetchPipelineGraph(pipelineId);
        const revision = snapshot.updated_at_ms ? String(snapshot.updated_at_ms) : null;
        const graph = fromApiGraphPlan(snapshot.graph ?? {});
        hydrateGraphWithRegistry(graph, get(registry));
        const plan = applyPaletteToGraphPlan(graph, get(dataTypes));
        refreshPipelineIoCaches(plan);
        updatePipeline(pipelineId, (next) => {
          next.graph = clonePlan(plan);
          next.revision = revision;
          next.planHash = truncatedPlanHashFromRevision(revision);
          next.updatedAt = Math.floor(Date.now() / 1000);
          next.diagnostics = null;
        });
        markDirty(pipelineId, false);
      } catch (error) {
        console.error('Failed to load pipeline graph', error);
        loadError.set((error as Error)?.message ?? 'Unable to load pipeline graph');
      } finally {
        graphLoadCache.delete(pipelineId);
      }
    })();

    graphLoadCache.set(pipelineId, promise);
    return promise;
  }

  function reloadPipelineGraph(pipelineId: string): Promise<void> | undefined {
    graphLoadCache.delete(pipelineId);
    return ensurePipelineGraph(pipelineId);
  }

  // Defer pipeline graph loading until the user selects one.

  function handleGraphSelect(detail: { nodeId: string | null; nodes?: string[]; edge: PipelineGraphEdgeSelection }) {
    const nodes = Array.isArray(detail.nodes)
      ? detail.nodes.filter(Boolean)
      : detail.nodeId
        ? [detail.nodeId]
        : [];
    graphSelection.set({ nodeId: detail.nodeId, nodes, edge: detail.edge });
  }

  function enterEmbeddedNode(nodeId: string | null) {
    const pipeline = get(selectedPipeline);
    const plan = get(editingPlan);
    if (!plan || !nodeId || !pipeline) return false;
    const node = plan.nodes?.[nodeId];
    if (!node) return false;
    if (node.embedded) {
      editingPath.update((path) => [...path, nodeId]);
      graphSelection.set({ nodeId: null, nodes: [], edge: null });
      closeGraphContextMenu();
      return true;
    }

    const externalId = node.external?.pipelineId?.trim();
    if (externalId) {
      const available = get(pipelines).some((entry) => entry.id === externalId);
      if (!available) {
        toaster.error({
          title: 'Pipeline unavailable',
          description: 'The referenced pipeline is not available locally.'
        });
        return false;
      }
      setSelectedPipeline(externalId);
      return true;
    }
    return false;
  }

  function exitEmbedded() {
    editingPath.update((path) => (path.length === 0 ? path : path.slice(0, path.length - 1)));
    graphSelection.set({ nodeId: null, nodes: [], edge: null });
    closeGraphContextMenu();
  }

  function closeGraphContextMenu() {
    graphContextMenu.set({ visible: false, port: null });
    graphContextSearch.set('');
  }

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

  const resolveDiagnosticNodeId = (
    plan: PipelineGraphPlan,
    spanNode: string | null,
    spanPort: string | null
  ): string | null => {
    if (!spanNode) return null;
    const trimmed = spanNode.trim();
    if (!trimmed) return null;
    if ((plan.format === 'daedalus' || plan.daedalus) && /^\d+$/u.test(trimmed)) {
      const index = Number.parseInt(trimmed, 10);
      if (Number.isFinite(index)) {
        const order = resolveNodeOrder(plan);
        const candidate = order[index];
        if (candidate) return candidate;
      }
    }
    if (plan.nodes?.[trimmed]) return trimmed;
    const normalized = trimmed.toLowerCase();
    const candidates = Object.entries(plan.nodes ?? {}).filter(([, node]) => {
      const backendId = node.backendId?.trim().toLowerCase();
      const nodeId = node.id?.trim().toLowerCase();
      return normalized === backendId || normalized === nodeId;
    });
    if (candidates.length === 0) {
      if (spanPort) {
        const normalizedPort = spanPort.trim().toLowerCase();
        const byPort = Object.entries(plan.nodes ?? {}).filter(([, node]) => {
          const inputs = Object.keys(node.inputs ?? {}).some((key) => key.trim().toLowerCase() === normalizedPort);
          const outputs = Object.keys(node.outputs ?? {}).some((key) => key.trim().toLowerCase() === normalizedPort);
          return inputs || outputs;
        });
        if (byPort.length === 1) return byPort[0]?.[0] ?? null;
      }
      return null;
    }
    if (candidates.length === 1) return candidates[0]?.[0] ?? null;
    if (spanPort) {
      const normalizedPort = spanPort.trim().toLowerCase();
      const byPort = candidates.filter(([, node]) => {
        const inputs = Object.keys(node.inputs ?? {}).some((key) => key.trim().toLowerCase() === normalizedPort);
        const outputs = Object.keys(node.outputs ?? {}).some((key) => key.trim().toLowerCase() === normalizedPort);
        return inputs || outputs;
      });
      if (byPort.length === 1) return byPort[0]?.[0] ?? null;
      if (byPort.length > 0) return byPort[0]?.[0] ?? null;
    }
    return candidates[0]?.[0] ?? null;
  };

  async function validateCurrentPipeline(
    planOverride?: PipelineGraphPlan | null,
    options?: { quiet?: boolean }
  ) {
    const pipeline = get(selectedPipeline);
    if (!pipeline) return;
    const plan = planOverride ?? pipeline.graph;
    try {
      const response = await validatePipelineGraph({
        graph: serializeGraphPlan(plan),
        enable_lints: true,
        active_features: [],
        graph_id: pipeline.id
      });
      const diagnostics = Array.isArray(response?.diagnostics) ? response.diagnostics : [];
      const warnings = await formatValidationWarnings(diagnostics);
      const nodeIds = Array.isArray(response?.node_ids)
        ? response.node_ids.filter((id): id is string => typeof id === 'string' && id.trim().length > 0)
        : [];
      const gpuSegments = Array.isArray(response?.gpu_segments)
        ? response.gpu_segments
            .map((segment) => {
              const bufferId = typeof segment?.buffer_id === 'number' && Number.isFinite(segment.buffer_id) ? segment.buffer_id : null;
              const nodes = Array.isArray(segment?.nodes)
                ? segment.nodes
                    .map((idx) => (typeof idx === 'number' && Number.isInteger(idx) && idx >= 0 ? nodeIds[idx] ?? null : null))
                    .filter((id): id is string => typeof id === 'string' && id.trim().length > 0)
                : [];
              if (bufferId == null) return null;
              if (nodes.length === 0) return null;
              return { bufferId, nodes };
            })
            .filter((segment): segment is { bufferId: number; nodes: string[] } => Boolean(segment))
        : [];
      const gpuEdges = Array.isArray(response?.gpu_edges)
        ? response.gpu_edges
            .map((edge) => {
              const edgeIndex = typeof edge?.edge_index === 'number' && Number.isFinite(edge.edge_index) ? edge.edge_index : null;
              if (edgeIndex == null) return null;
              return {
                edgeIndex,
                gpuFastPath: Boolean(edge?.gpu_fast_path),
                bufferId: typeof edge?.buffer_id === 'number' && Number.isFinite(edge.buffer_id) ? edge.buffer_id : null
              };
            })
            .filter((edge): edge is { edgeIndex: number; gpuFastPath: boolean; bufferId: number | null } => Boolean(edge))
        : [];
      const diagnosticWarnings = diagnostics
        .map((diag) => {
          const code = typeof diag?.code === 'string' ? diag.code.trim() : '';
          const message = typeof diag?.message === 'string' ? diag.message.trim() : '';
          const nodeIdRaw = typeof diag?.span?.node === 'string' ? diag.span.node.trim() : '';
          const port = typeof diag?.span?.port === 'string' ? diag.span.port.trim() : '';
          const nodeId = resolveDiagnosticNodeId(plan, nodeIdRaw, port);
          const summary = code && message ? `${code}: ${message}` : message || code;
          if (!summary) return null;
          return {
            message: summary,
            ...(nodeId ? { nodeId } : {}),
            ...(port ? { port } : {})
          };
        })
        .filter(Boolean);
      pipelineStore.update(pipeline.id, (current) => {
        const clone = clonePipeline(current);
        clone.diagnostics = {
          warnings: diagnosticWarnings as Array<{ message: string; nodeId?: string; port?: string }>,
          gpu: {
            segments: gpuSegments,
            edges: gpuEdges
          },
          ...(!response.ok ? { error: 'Graph failed validation' } : {})
        };
        return clone;
      });
      validationMessages.update((map) => ({
        ...map,
        [pipeline.id]: { ok: Boolean(response.ok), warnings }
      }));
      if (!options?.quiet) {
        toaster.success({
          title: 'Validation completed',
          description: response.ok ? 'No issues detected' : `${warnings.length} warnings`
        });
      }
    } catch (error) {
      console.error('Pipeline validation failed', error);
      const message = (error as Error)?.message ?? 'Unknown error';
      if (!options?.quiet) {
        reportError({
          title: 'Validation failed',
          error,
          fallback: message
        });
      }
    }
  }

  async function formatValidationWarnings(diagnostics: Array<{ code: string; message: string; span?: { node?: string | null; port?: string | null } | null }>): Promise<string[]> {
    const formatFallback = () =>
      diagnostics.map((diag) => {
        const nodeId = diag?.span?.node ?? null;
        const port = diag?.span?.port ?? null;
        const at = nodeId ? ` (node ${nodeId}${port ? `:${port}` : ''})` : '';
        return `${diag.code}: ${diag.message}${at}`;
      });

    if (typeof Worker === 'undefined') {
      return formatFallback();
    }
    if (!validationWorker) {
      validationWorker = createPipelineValidationWorker();
      validationWorker.onmessage = (event) => {
        const payload = event.data as { requestId: number; warnings?: string[] };
        const resolver = validationResolvers.get(payload.requestId);
        if (!resolver) return;
        validationResolvers.delete(payload.requestId);
        resolver(Array.isArray(payload.warnings) ? payload.warnings : []);
      };
    }
    const requestId = ++validationRequestId;
    const result = new Promise<string[]>((resolve) => {
      validationResolvers.set(requestId, resolve);
    });
    validationWorker.postMessage({ requestId, diagnostics });
    return result;
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
    if (validationWorker) {
      validationWorker.terminate();
      validationWorker = null;
    }
    validationResolvers.clear();
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
