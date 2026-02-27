<script lang="ts" module>
  import { onDestroy, onMount, untrack } from 'svelte';
  import type { StreamInfo, StreamManifest, StreamMetrics } from '$lib/api/httpClient';
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
    RAW_LOOPBACK_GRAPH: any;
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

    let pipelineRegistrySnapshot = $state<any | null>(null);
    let pipelineOutputOptionsCache = $state<Record<string, string[]>>({});
    let pipelineGraphCache = $state<Record<string, any>>({});
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

    let applyPipelineOverridesToGraphRef = (pipelineId: string, graph: any) => graph;

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
      ensurePipelineRegistry,
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

    onDestroy(() => {
      stopPipelineTuningPointerTracking();
    });

    onMount(() => {
      pipelineState.pipelineAssignDraft = normalizeAssignedPipelineIds(pipelineState.assignedPipelineIds);
      void refresh();
      void refreshPipelineGraphs();
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
      add((manifest as any)?.active_pipeline_id);
      add((manifest as any)?.pipeline_id);
      if (Array.isArray((manifest as any)?.pipelines)) {
        (manifest as any).pipelines.forEach((entry: any) => add(entry?.pipeline_id ?? entry?.pipelineId ?? entry?.id));
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

    const runtime = $derived.by(() => ({
      pipelineState,
      pipelineRegistrySnapshot,
      pipelineOutputOptionsCache,
      pipelineGraphCache,
      pipelineUiHydratedFor,
      telemetrySample: derived.telemetrySample,
      activePipelineIds: derived.activePipelineIds,
      pipelineGridRowIndices: derived.pipelineGridRowIndices,
      pipelineGridColumnIndices: derived.pipelineGridColumnIndices,
      pipelineGridIsSingle: derived.pipelineGridIsSingle,
      pipelineGridIsMultiplex: derived.pipelineGridIsMultiplex,
      pipelineAssignFilteredGraphs: derived.pipelineAssignFilteredGraphs,
      pipelineTuningGraph: derived.pipelineTuningGraph,
      pipelineTuningUi: derived.pipelineTuningUi,
      pipelineTuningPlan: derived.pipelineTuningPlan,
      canShowTuningEngineConfig: derived.canShowTuningEngineConfig,
      pipelineTuningBaseNodeOverrides: derived.pipelineTuningBaseNodeOverrides,
      pipelineTuningCameraNodeOverrides: derived.pipelineTuningCameraNodeOverrides,
      pipelineTuningEffectiveNodeOverrides: derived.pipelineTuningEffectiveNodeOverrides,
      pipelineTuningNodeDescriptors: derived.pipelineTuningNodeDescriptors,
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
      // expose core state for other controllers
      pipelineGraphs: pipelineState.pipelineGraphs,
      pipelineGraphLoading: pipelineState.pipelineGraphLoading,
      pipelineGraphError: pipelineState.pipelineGraphError,
      selectedPipelineId: pipelineState.selectedPipelineId,
      pipelineOutputOptions: pipelineState.pipelineOutputOptions,
      selectedPipelineOutput: pipelineState.selectedPipelineOutput,
      selectedPipelineGraph: pipelineState.selectedPipelineGraph,
      assignedPipelineIds: pipelineState.assignedPipelineIds,
      pipelineOutputByPipelineId: pipelineState.pipelineOutputByPipelineId,
      pipelineAssignModalOpen: pipelineState.pipelineAssignModalOpen,
      pipelineAssignDraft: pipelineState.pipelineAssignDraft,
      pipelineAssignQuery: pipelineState.pipelineAssignQuery,
      pipelineGridRows: pipelineState.pipelineGridRows,
      pipelineGridColumns: pipelineState.pipelineGridColumns,
      pipelineGridSlots: pipelineState.pipelineGridSlots,
      pipelineGridSlotOutputKeys: pipelineState.pipelineGridSlotOutputKeys,
      pipelineUiHydrated: pipelineState.pipelineUiHydrated,
      pipelineDragPayload: pipelineState.pipelineDragPayload,
      pipelineLayoutTouched: pipelineState.pipelineLayoutTouched,
      pipelineRemoveModalOpen: pipelineState.pipelineRemoveModalOpen,
      pipelineRemoveCandidateId: pipelineState.pipelineRemoveCandidateId,
      pipelineTuningPanelOpen: pipelineState.pipelineTuningPanelOpen,
      pipelineTuningPipelineId: pipelineState.pipelineTuningPipelineId,
      pipelineTuningEngineConfigOpen: pipelineState.pipelineTuningEngineConfigOpen,
      pipelineTuningDragState: pipelineState.pipelineTuningDragState,
      pipelineTuningResizeState: pipelineState.pipelineTuningResizeState,
      pipelineTuningPosition: pipelineState.pipelineTuningPosition,
      pipelineTuningSize: pipelineState.pipelineTuningSize,
      pipelineTuningLoading: pipelineState.pipelineTuningLoading,
      pipelineTuningError: pipelineState.pipelineTuningError,
      pipelineInputOverridesById: pipelineState.pipelineInputOverridesById,
      pipelineNodeOverridesById: pipelineState.pipelineNodeOverridesById,
      pipelineInputDraftsById: pipelineState.pipelineInputDraftsById,
      pipelineNodeDraftsById: pipelineState.pipelineNodeDraftsById,
      pipelineInputErrorsById: pipelineState.pipelineInputErrorsById,
      pipelineNodeErrorsById: pipelineState.pipelineNodeErrorsById,
      pipelineTuningApplyBusy: pipelineState.pipelineTuningApplyBusy,
      pipelineTuningApplyQueuedById: pipelineState.pipelineTuningApplyQueuedById,
      pipelineTuningLastAppliedSignatureById: pipelineState.pipelineTuningLastAppliedSignatureById,
      pipelineTuningLastAppliedNodeOverridesById: pipelineState.pipelineTuningLastAppliedNodeOverridesById,
      PIPELINE_LAYOUT_DEBOUNCE_MS: pipelineState.PIPELINE_LAYOUT_DEBOUNCE_MS,
      pipelineLayoutApplyTimer: pipelineState.pipelineLayoutApplyTimer,
      pipelineTuningApplyRafById: pipelineState.pipelineTuningApplyRafById
    }));

    // svelte-ignore state_referenced_locally
    return runtime;
  }
</script>
