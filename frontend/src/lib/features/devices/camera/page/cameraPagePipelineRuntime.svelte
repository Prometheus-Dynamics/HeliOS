<script lang="ts" module>
  import { onDestroy, onMount, untrack } from 'svelte';
  import type { DaedalusRegistryResponse } from '$lib/ts-bindings/http/client';
  import type { StreamInfo, StreamManifest, StreamMetrics } from '$lib/api/httpClient';
  import { backendFeatures } from '$lib/api/backendFeatures';
  import type { PipelineUi } from '$lib/features/pipelines/pipelineUiTypes';
  import { createCameraPipelineState } from './cameraPipelineStore.svelte';
  import { createCameraPipelineMetricsController } from './cameraPipelineMetricsController';
  import { createCameraPageDerived } from './cameraPageDerived.svelte';
  import {
    PIPELINE_OUTPUT_CELL_KEY,
    RAW_PIPELINE_UUID,
    normalizeAssignedPipelineIds
  } from './cameraPipelineTuningController';
  import { createCameraPipelineLayoutRuntime } from './cameraPipelineLayoutRuntime';
  import { createCameraPipelineTuningRuntime } from './cameraPipelineTuningRuntime';

  type PipelineRuntimeDeps = {
    streamId: string;
    stream: () => StreamInfo | null;
    manifestState: () => StreamManifest | null;
    streamMetrics: () => StreamMetrics | null;
    refresh: () => Promise<void>;
    scheduleStreamPresetApply: () => void;
    onExternalLayoutApplied?: () => void;
    awaitStreamUpdatesSocket: (streamId: string, timeoutMs?: number) => Promise<unknown>;
    apiPath: (path: string) => string;
    apiBase: string;
    PIPELINE_UI_METADATA_KEY: string;
    DEFAULT_PIPELINE_UI: PipelineUi;
    RAW_PIPELINE_ID: string;
    RAW_LOOPBACK_GRAPH: unknown;
  };

  export function createCameraPagePipelineRuntime(deps: PipelineRuntimeDeps) {
    const {
      streamId,
      stream,
      manifestState,
      streamMetrics,
      refresh,
      scheduleStreamPresetApply,
      awaitStreamUpdatesSocket,
      apiPath,
      PIPELINE_UI_METADATA_KEY,
      DEFAULT_PIPELINE_UI,
      RAW_PIPELINE_ID,
      RAW_LOOPBACK_GRAPH
    } = deps;

    const pipelineState = createCameraPipelineState();

    const pipelineMetricsController = createCameraPipelineMetricsController({
      get manifestState() {
        return manifestState();
      },
      get streamMetrics() {
        return streamMetrics();
      }
    });

    const {
      normalizePipelineIdForMetrics,
      activePipelineWireId,
      pipelineMetricsForId,
      findOutputNodeMetrics,
      pipelineDurationMs,
      formatDurationMs,
      outputDurationForPipeline
    } = pipelineMetricsController;

    const asRecord = (value: unknown): Record<string, unknown> | null =>
      value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

    let pipelineRegistrySnapshot = $state<DaedalusRegistryResponse | null>(null);
    let pipelineOutputOptionsCache = $state<Record<string, string[]>>({});
    let pipelineGraphCache = $state<Record<string, unknown>>({});
    const derived = createCameraPageDerived({
      get selectedPipelineId() {
        return pipelineState.selectedPipelineId;
      },
      get assignedPipelineIds() {
        return pipelineState.assignedPipelineIds ?? [];
      },
      get pipelineGridSlots() {
        return pipelineState.pipelineGridSlots ?? {};
      },
      get manifestState() {
        return manifestState();
      },
      get pipelineGridRows() {
        return pipelineState.pipelineGridRows;
      },
      get pipelineGridColumns() {
        return pipelineState.pipelineGridColumns;
      },
      get pipelineAssignQuery() {
        return pipelineState.pipelineAssignQuery;
      },
      get pipelineGraphs() {
        return pipelineState.pipelineGraphs ?? [];
      },
      get pipelineTuningPipelineId() {
        return pipelineState.pipelineTuningPipelineId;
      },
      get pipelineGraphCache() {
        return pipelineGraphCache;
      },
      get pipelineTuningLiveGraph() {
        return pipelineState.pipelineTuningLiveGraph;
      },
      get pipelineTuningUiOverride() {
        return pipelineState.pipelineTuningUiOverride;
      },
      get pipelineTuningGraphOverride() {
        return pipelineState.pipelineTuningGraphOverride;
      },
      get pipelineNodeOverridesById() {
        return pipelineState.pipelineNodeOverridesById;
      },
      get pipelineRegistrySnapshot() {
        return pipelineRegistrySnapshot;
      },
      get PIPELINE_UI_METADATA_KEY() {
        return PIPELINE_UI_METADATA_KEY;
      },
      get DEFAULT_PIPELINE_UI() {
        return DEFAULT_PIPELINE_UI;
      },
      get RAW_PIPELINE_ID() {
        return RAW_PIPELINE_ID;
      },
      get RAW_PIPELINE_UUID() {
        return RAW_PIPELINE_UUID;
      },
      get RAW_LOOPBACK_GRAPH() {
        return RAW_LOOPBACK_GRAPH;
      }
    });

    let applyPipelineOverridesToGraphRef = (_pipelineId: string, graph: unknown) => graph;

    const pipelineLayoutController = createCameraPipelineLayoutRuntime({
      streamId,
      stream,
      manifestState,
      pipelineState,
      pipelineRegistrySnapshot: () => pipelineRegistrySnapshot,
      setPipelineRegistrySnapshot: (value) => {
        pipelineRegistrySnapshot = value;
      },
      pipelineGraphCache: () => pipelineGraphCache,
      setPipelineGraphCache: (value) => {
        pipelineGraphCache = value;
      },
      pipelineOutputOptionsCache: () => pipelineOutputOptionsCache,
      setPipelineOutputOptionsCache: (value) => {
        pipelineOutputOptionsCache = value;
      },
      refresh,
      scheduleStreamPresetApply,
      onExternalLayoutApplied: deps.onExternalLayoutApplied,
      applyPipelineOverridesToGraph: (pipelineId, graph) => applyPipelineOverridesToGraphRef(pipelineId, graph),
      apiPath,
      apiBase: deps.apiBase,
      layoutDebounceMs: pipelineState.PIPELINE_LAYOUT_DEBOUNCE_MS
    });

    const {
      extractGraphOutputPorts,
      ensurePipelineRegistry,
      ensurePipelineGraphAndOutputs,
      ensurePipelineOutputsLoaded,
      outputSelectionForPipeline,
      isMultiplexLayout,
      gridHasUnappliedPipelines,
      isPipelineApplied,
      buildPipelineLayoutPayload,
      applyPipelineGraphForId,
      applyUnappliedPipelines,
      schedulePipelineLayoutApply,
      setOutputSelectionForPipeline,
      refreshPipelineGraphs,
      refreshPipelineOutputs,
      setLivePipelineOutput,
      setFrameSourceForPipelineInstance,
      pipelineLabel,
      listPipelineTemplatesForAssign,
      createPipelineFromTemplateAndAssign,
      openPipelineAssignModal,
      closePipelineAssignModal,
      savePipelineAssignModal,
      dropPipelineEverywhere,
      applyAssignedPipelineIds,
      openPipelineRemoveModal,
      closePipelineRemoveModal,
      confirmPipelineRemove,
      setPipelineGridDimensions,
      gridKey,
      pipelineForCell,
      setPipelineForCell,
      outputKeyForCell,
      setOutputKeyForCell,
      handlePipelineDragStart,
      readDragPayload,
      allowDrop,
      dropOnCell,
      clearCell,
      hydratePipelineUi,
      persistPipelineUi,
      pipelineUiStorageKey,
      pipelineLayoutSignature
    } = pipelineLayoutController;

    const pipelineTuningController = createCameraPipelineTuningRuntime({
      streamId,
      stream,
      pipelineState,
      derived,
      ensurePipelineRegistry: async () => {
        await ensurePipelineRegistry();
      },
      ensurePipelineGraphAndOutputs,
      awaitStreamUpdatesSocket,
      pipelineGraphCache: () => pipelineGraphCache,
      pipelineRegistrySnapshot: () => pipelineRegistrySnapshot
    });

    const {
      readPipelineInputDraft,
      readPipelineNodeDraft,
      setPipelineInputDraft,
      clearPipelineInputDraft,
      setPipelineNodeDraft,
      clearPipelineNodeDraft,
      setPipelineInputError,
      setPipelineNodeError,
      setPipelineInputOverride,
      setPipelineNodeOverride,
      updatePipelineInputDraft,
      updatePipelineNodeDraft,
      updatePipelineNodeValue,
      schedulePipelineTuningApply,
      applyPipelineTuningOverrides,
      openPipelineTuningPanel,
      closePipelineTuningPanel,
      stopPipelineTuningPointerTracking,
      startPipelineTuningPointerTracking,
      onPipelineTuningPointerMove,
      onPipelineTuningPointerUp,
      startPipelineTuningDrag,
      startPipelineTuningResize,
      applyPipelineOverridesToGraph,
      nodeValueSignature,
      pipelineOverrideSignature,
      clamp
    } = pipelineTuningController;

    applyPipelineOverridesToGraphRef = applyPipelineOverridesToGraph;

    let pipelineRegistryPrefetchHint = false;
    let pipelineRegistryPrefetchIssued = false;
    const unsubscribeBackendFeatures = backendFeatures.subscribe((value) => {
      pipelineRegistryPrefetchHint = Boolean(value?.pipelineRegistryPrefetch);
      if (!pipelineRegistryPrefetchHint || pipelineRegistryPrefetchIssued) return;
      pipelineRegistryPrefetchIssued = true;
      void ensurePipelineRegistry();
    });

    onDestroy(() => {
      unsubscribeBackendFeatures();
      stopPipelineTuningPointerTracking();
    });

    onMount(() => {
      pipelineState.pipelineAssignDraft = normalizeAssignedPipelineIds(pipelineState.assignedPipelineIds);
      void refresh();
      void refreshPipelineGraphs();
      if (pipelineRegistryPrefetchHint && !pipelineRegistryPrefetchIssued) {
        pipelineRegistryPrefetchIssued = true;
        void ensurePipelineRegistry();
      }
    });

    $effect(() => {
      void pipelineState.pipelineGridSlots;
      const outputPipelineId = pipelineState.pipelineGridSlots[PIPELINE_OUTPUT_CELL_KEY] ?? null;
      if (pipelineState.selectedPipelineId !== outputPipelineId) {
        pipelineState.selectedPipelineId = outputPipelineId;
        pipelineState.selectedPipelineOutput = outputPipelineId ? outputSelectionForPipeline(outputPipelineId) : null;
      }
    });

    $effect(() => {
      void pipelineState.selectedPipelineId;
      if (pipelineState.selectedPipelineId === RAW_PIPELINE_ID) {
        if (pipelineState.selectedPipelineOutput == null) {
          pipelineState.selectedPipelineOutput = 'raw';
        }
        untrack(() => {
          setOutputSelectionForPipeline(RAW_PIPELINE_ID, pipelineState.selectedPipelineOutput);
        });
      }
    });

    $effect(() => {
      void pipelineState.pipelineUiHydrated;
      void pipelineState.assignedPipelineIds;
      void pipelineState.pipelineGridSlots;
      void pipelineState.selectedPipelineId;
      if (!pipelineState.pipelineUiHydrated) return;
      const ids = new Set<string>();
      (pipelineState.assignedPipelineIds ?? []).forEach((id) => {
        const normalized = String(id ?? '').trim();
        if (normalized.length) ids.add(normalized);
      });
      Object.values(pipelineState.pipelineGridSlots ?? {}).forEach((id) => {
        const normalized = typeof id === 'string' ? id.trim() : '';
        if (normalized.length) ids.add(normalized);
      });
      if (pipelineState.selectedPipelineId) ids.add(String(pipelineState.selectedPipelineId).trim());
      untrack(() => {
        ids.forEach((id) => void ensurePipelineOutputsLoaded(id));
      });
    });

    $effect(() => {
      const manifest = manifestState();
      if (!manifest) return;
      void pipelineOutputOptionsCache;
      const ids = new Set<string>();
      const add = (value: unknown) => {
        const raw = typeof value === 'string' ? value.trim() : '';
        if (!raw) return;
        const normalized = raw === RAW_PIPELINE_UUID ? RAW_PIPELINE_ID : raw;
        if (normalized === RAW_PIPELINE_ID) return;
        if (Object.prototype.hasOwnProperty.call(pipelineOutputOptionsCache, normalized)) return;
        ids.add(normalized);
      };
      const manifestRecord = asRecord(manifest);
      add(manifest.active_pipeline_id);
      add(manifestRecord?.pipeline_id);
      if (Array.isArray(manifestRecord?.pipelines)) {
        manifestRecord.pipelines.forEach((entry) => {
          const pipeline = asRecord(entry);
          add(pipeline?.pipeline_id ?? pipeline?.pipelineId ?? pipeline?.id);
        });
      }
      if (!ids.size) return;
      untrack(() => {
        ids.forEach((id) => void ensurePipelineOutputsLoaded(id));
      });
    });

    $effect(() => {
      void pipelineState.selectedPipelineId;
      untrack(() => {
        void refreshPipelineOutputs(pipelineState.selectedPipelineId);
      });
    });

    $effect(() => {
      void pipelineState.assignedPipelineIds;
      void pipelineState.pipelineGridRows;
      void pipelineState.pipelineGridColumns;
      void pipelineState.pipelineGridSlots;
      void pipelineState.pipelineOutputByPipelineId;
      persistPipelineUi();
    });

    let pipelineUiHydratedFor = $state<string | null>(null);
    $effect(() => {
      const key = String(stream()?.id ?? '').trim() || streamId;
      if (!key.length) return;
      if (pipelineUiHydratedFor === key) return;
      pipelineUiHydratedFor = key;
      pipelineState.pipelineUiHydrated = false;
      hydratePipelineUi(key);
      pipelineState.pipelineUiHydrated = true;
    });

    return {
      get pipelineState() {
        return pipelineState;
      },
      get pipelineRegistrySnapshot() {
        return pipelineRegistrySnapshot;
      },
      get pipelineOutputOptionsCache() {
        return pipelineOutputOptionsCache;
      },
      get pipelineGraphCache() {
        return pipelineGraphCache;
      },
      get pipelineUiHydratedFor() {
        return pipelineUiHydratedFor;
      },
      get telemetrySample() {
        return derived.telemetrySample;
      },
      get activePipelineIds() {
        return derived.activePipelineIds;
      },
      get pipelineGridRowIndices() {
        return derived.pipelineGridRowIndices;
      },
      get pipelineGridColumnIndices() {
        return derived.pipelineGridColumnIndices;
      },
      get pipelineGridIsSingle() {
        return derived.pipelineGridIsSingle;
      },
      get pipelineGridIsMultiplex() {
        return derived.pipelineGridIsMultiplex;
      },
      get pipelineAssignFilteredGraphs() {
        return derived.pipelineAssignFilteredGraphs;
      },
      get pipelineTuningGraph() {
        return derived.pipelineTuningGraph;
      },
      get pipelineTuningUi() {
        return derived.pipelineTuningUi;
      },
      get pipelineTuningPlan() {
        return derived.pipelineTuningPlan;
      },
      get canShowTuningEngineConfig() {
        return derived.canShowTuningEngineConfig;
      },
      get pipelineTuningBaseNodeOverrides() {
        return derived.pipelineTuningBaseNodeOverrides;
      },
      get pipelineTuningCameraNodeOverrides() {
        return derived.pipelineTuningCameraNodeOverrides;
      },
      get pipelineTuningEffectiveNodeOverrides() {
        return derived.pipelineTuningEffectiveNodeOverrides;
      },
      get pipelineTuningNodeDescriptors() {
        return derived.pipelineTuningNodeDescriptors;
      },
      normalizePipelineIdForMetrics,
      activePipelineWireId,
      pipelineMetricsForId,
      findOutputNodeMetrics,
      pipelineDurationMs,
      formatDurationMs,
      outputDurationForPipeline,
      extractGraphOutputPorts,
      ensurePipelineRegistry,
      ensurePipelineGraphAndOutputs,
      ensurePipelineOutputsLoaded,
      outputSelectionForPipeline,
      isMultiplexLayout,
      gridHasUnappliedPipelines,
      isPipelineApplied,
      buildPipelineLayoutPayload,
      applyPipelineGraphForId,
      applyUnappliedPipelines,
      schedulePipelineLayoutApply,
      setOutputSelectionForPipeline,
      refreshPipelineGraphs,
      refreshPipelineOutputs,
      setLivePipelineOutput,
      setFrameSourceForPipelineInstance,
      pipelineLabel,
      listPipelineTemplatesForAssign,
      createPipelineFromTemplateAndAssign,
      openPipelineAssignModal,
      closePipelineAssignModal,
      savePipelineAssignModal,
      dropPipelineEverywhere,
      applyAssignedPipelineIds,
      openPipelineRemoveModal,
      closePipelineRemoveModal,
      confirmPipelineRemove,
      setPipelineGridDimensions,
      gridKey,
      pipelineForCell,
      setPipelineForCell,
      outputKeyForCell,
      setOutputKeyForCell,
      handlePipelineDragStart,
      readDragPayload,
      allowDrop,
      dropOnCell,
      clearCell,
      hydratePipelineUi,
      persistPipelineUi,
      pipelineUiStorageKey,
      pipelineLayoutSignature,
      readPipelineInputDraft,
      readPipelineNodeDraft,
      setPipelineInputDraft,
      clearPipelineInputDraft,
      setPipelineNodeDraft,
      clearPipelineNodeDraft,
      setPipelineInputError,
      setPipelineNodeError,
      setPipelineInputOverride,
      setPipelineNodeOverride,
      updatePipelineInputDraft,
      updatePipelineNodeDraft,
      updatePipelineNodeValue,
      schedulePipelineTuningApply,
      applyPipelineTuningOverrides,
      openPipelineTuningPanel,
      closePipelineTuningPanel,
      stopPipelineTuningPointerTracking,
      startPipelineTuningPointerTracking,
      onPipelineTuningPointerMove,
      onPipelineTuningPointerUp,
      startPipelineTuningDrag,
      startPipelineTuningResize,
      applyPipelineOverridesToGraph,
      nodeValueSignature,
      pipelineOverrideSignature,
      clamp,
      get pipelineGraphs() {
        return pipelineState.pipelineGraphs;
      },
      get pipelineGraphLoading() {
        return pipelineState.pipelineGraphLoading;
      },
      get pipelineGraphError() {
        return pipelineState.pipelineGraphError;
      },
      get selectedPipelineId() {
        return pipelineState.selectedPipelineId;
      },
      get pipelineOutputOptions() {
        return pipelineState.pipelineOutputOptions;
      },
      get selectedPipelineOutput() {
        return pipelineState.selectedPipelineOutput;
      },
      get selectedPipelineGraph() {
        return pipelineState.selectedPipelineGraph;
      },
      get assignedPipelineIds() {
        return pipelineState.assignedPipelineIds;
      },
      get pipelineOutputByPipelineId() {
        return pipelineState.pipelineOutputByPipelineId;
      },
      get pipelineAssignModalOpen() {
        return pipelineState.pipelineAssignModalOpen;
      },
      get pipelineAssignDraft() {
        return pipelineState.pipelineAssignDraft;
      },
      get pipelineAssignQuery() {
        return pipelineState.pipelineAssignQuery;
      },
      get pipelineGridRows() {
        return pipelineState.pipelineGridRows;
      },
      get pipelineGridColumns() {
        return pipelineState.pipelineGridColumns;
      },
      get pipelineGridSlots() {
        return pipelineState.pipelineGridSlots;
      },
      get pipelineGridSlotOutputKeys() {
        return pipelineState.pipelineGridSlotOutputKeys;
      },
      get pipelineUiHydrated() {
        return pipelineState.pipelineUiHydrated;
      },
      get pipelineDragPayload() {
        return pipelineState.pipelineDragPayload;
      },
      get pipelineLayoutTouched() {
        return pipelineState.pipelineLayoutTouched;
      },
      get pipelineRemoveModalOpen() {
        return pipelineState.pipelineRemoveModalOpen;
      },
      get pipelineRemoveCandidateId() {
        return pipelineState.pipelineRemoveCandidateId;
      },
      get pipelineTuningPanelOpen() {
        return pipelineState.pipelineTuningPanelOpen;
      },
      get pipelineTuningPipelineId() {
        return pipelineState.pipelineTuningPipelineId;
      },
      get pipelineTuningEngineConfigOpen() {
        return pipelineState.pipelineTuningEngineConfigOpen;
      },
      get pipelineTuningDragState() {
        return pipelineState.pipelineTuningDragState;
      },
      get pipelineTuningResizeState() {
        return pipelineState.pipelineTuningResizeState;
      },
      get pipelineTuningPosition() {
        return pipelineState.pipelineTuningPosition;
      },
      get pipelineTuningSize() {
        return pipelineState.pipelineTuningSize;
      },
      get pipelineTuningLoading() {
        return pipelineState.pipelineTuningLoading;
      },
      get pipelineTuningError() {
        return pipelineState.pipelineTuningError;
      },
      get pipelineInputOverridesById() {
        return pipelineState.pipelineInputOverridesById;
      },
      get pipelineNodeOverridesById() {
        return pipelineState.pipelineNodeOverridesById;
      },
      get pipelineInputDraftsById() {
        return pipelineState.pipelineInputDraftsById;
      },
      get pipelineNodeDraftsById() {
        return pipelineState.pipelineNodeDraftsById;
      },
      get pipelineInputErrorsById() {
        return pipelineState.pipelineInputErrorsById;
      },
      get pipelineNodeErrorsById() {
        return pipelineState.pipelineNodeErrorsById;
      },
      get pipelineTuningApplyBusy() {
        return pipelineState.pipelineTuningApplyBusy;
      },
      get pipelineTuningApplyQueuedById() {
        return pipelineState.pipelineTuningApplyQueuedById;
      },
      get pipelineTuningLastAppliedSignatureById() {
        return pipelineState.pipelineTuningLastAppliedSignatureById;
      },
      get pipelineTuningLastAppliedNodeOverridesById() {
        return pipelineState.pipelineTuningLastAppliedNodeOverridesById;
      },
      get PIPELINE_LAYOUT_DEBOUNCE_MS() {
        return pipelineState.PIPELINE_LAYOUT_DEBOUNCE_MS;
      },
      get pipelineLayoutApplyTimer() {
        return pipelineState.pipelineLayoutApplyTimer;
      },
      get pipelineTuningApplyRafById() {
        return pipelineState.pipelineTuningApplyRafById;
      }
    };
  }
</script>
