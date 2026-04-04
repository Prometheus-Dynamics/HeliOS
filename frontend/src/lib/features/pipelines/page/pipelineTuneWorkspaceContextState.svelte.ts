  import { browser } from '$app/environment';
  import { untrack } from 'svelte';
  import { get, type Readable } from 'svelte/store';
  import { toaster } from '$lib';
  import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';
  import type { StreamControlSocket } from '$lib/api/streamControls';
  import type { ControlMeta, StreamInfo, StreamPipelineLayout } from '$lib/api/client';
  import type { PipelineGraphPlan, PipelineNodeValue, PipelineOverviewPipeline } from '$lib/types/pipeline';
  import type { PipelineUi } from '$lib/features/pipelines/pipelineUiTypes';
  import { DEFAULT_PIPELINE_UI } from '$lib/features/pipelines/pipelineUiTypes';
  import {
    buildControlValue, clampControlValue, controlMax, controlMin, controlStep,
    displayControlValue, extractControlValue, menuOptions, menuValueDisplay, seedControlState
  } from './pipelineStreamControlUtils';
  import { buildTuneMultiplexSignature, multiplexKey, normalizeMultiplexSlots } from './pipelineMultiplexUtils';
  import { createTuneStreamOverrides } from './pipelineTuneStreamOverrides';
  import { buildPipelineMetricsSummary, buildTuneMetricsSnapshot, tuneRuntimeFromStats } from './pipelineTuneMetricsRuntime';
  import { createTuneWorkspaceDerivedState } from './pipelineTuneWorkspaceDerivedState.svelte';
  import { createTuneWorkspaceInteractionState } from './pipelineTuneWorkspaceInteractionState.svelte';
  import { createTuneWorkspaceEditorState } from './pipelineTuneWorkspaceEditorState.svelte';
  import { createTuneWorkspaceOrchestrationState } from './pipelineTuneWorkspaceOrchestrationState.svelte';
  import { mergeNodeOverrides, normalizePortKey, streamOverrideSignature } from './pipelineTuneHelpers';
  import { asRecord, asStreamInfo, extractGraphAlias } from './pipelineTunePageSupport';
  import { isDaedalusPlan, safeClonePlan } from './pipelineTuneConstantUtils';
  import type { PipelineUpdates, PipelinesApi, StreamsApi, TuneDeps } from './pipelineTuneWorkspaceTypes';
  import { buildDaedalusGraphPatch } from '$lib/features/pipelines/daedalusGraph';
  import { serializeGraphPlan } from '$lib/features/pipelines/graph';
  import { SvelteMap } from 'svelte/reactivity';

  export function createPipelineTuneWorkspaceContext(getDeps: () => TuneDeps) {
  const {
    activeTab,
    pipelineUpdates,
    pipelineUpdatesReady,
    streamUpdatesReadyById,
    getStreamUpdatesSocket,
    selectedPipeline,
    editingPlan,
    detailContext,
    pipelines,
    pipelineLabelById,
    streamUsesPipeline,
    streamLabel,
    streamGraphForPipeline,
    outputOptionsForPipeline,
    handlePlanChange,
    setNodeConstantValue,
    saveCurrentPipeline,
    openAssignModal,
    assignBusy,
    refreshPipelineMetrics,
    resolveDataTypeKey,
    getDataTypeVariants,
    resolveRegistryEntryForNode,
    buildNodeValueFromInput,
    extractTuneConstantEntries,
    RAW_STREAM_PIPELINE_ID,
    RAW_STREAM_PIPELINE_UUID,
    PipelinesApi,
    StreamsApi
  }: TuneDeps = untrack(getDeps);
  untrack(() => detailContext);

  const PIPELINE_UI_METADATA_KEY = 'helios.pipeline.ui';

  let tuneNodeDrafts = $state<Record<string, Record<string, string>>>({}),
    tuneNodeErrors = $state<Record<string, Record<string, string | null>>>({});
  let tuneStreams = $state<StreamInfo[]>([]),
    tuneStreamsLoading = $state(false),
    tuneStreamsLoaded = $state(false),
    tuneStreamsError = $state<string | null>(null);
  let tuneStreamsRetryTimer: number | null = null;
  let tuneAssignBusySeen = $state(false), tuneScopeTab = $state<'global' | string>('global');
  let tuneUiMode = $state<'pipeline' | 'advanced'>('pipeline'), tuneUiEditMode = $state(false), tuneUiActiveTabId = $state<string>('');
  let tuneUiSelectedItemId = $state<string | null>(null), tuneUiSelectedItemAnchor = $state<{ x: number; y: number } | null>(null);
  let tunePipelineUiDraft = $state<PipelineUi>(DEFAULT_PIPELINE_UI), tunePipelineUiSearch = $state('');
  let tuneStreamInputOverridesById = $state<Record<string, Record<string, PipelineNodeValue>>>({});
  let tuneStreamNodeOverridesById = $state<Record<string, Record<string, Record<string, PipelineNodeValue>>>>({});
  let tuneStreamOverridesLoaded = $state<Record<string, boolean>>({});
  let tuneStreamNodeDraftsById = $state<Record<string, Record<string, Record<string, string>>>>({});
  let tuneStreamNodeErrorsById = $state<Record<string, Record<string, Record<string, string | null>>>>({});
  let tuneStreamApplyBusyById = $state<Record<string, boolean>>({});
  let tuneStreamApplyErrorById = $state<Record<string, string | null>>({});
  let tuneStreamApplyQueuedById = $state<Record<string, boolean>>({});
  let tuneStreamAutoApplyTimerById = $state<Record<string, number>>({});
  let tuneStreamApplyRafById = $state<Record<string, number>>({});
  let tuneStreamLastAppliedSignatureById = $state<Record<string, string>>({});
  let tuneStreamLastAppliedNodeOverridesById = $state<Record<string, Record<string, Record<string, PipelineNodeValue>>>>({});
  let tunePerformanceTab = $state<'metrics' | 'controls' | 'layout' | 'outputs'>('metrics');
  let tuneMultiplexRows = $state(1);
  let tuneMultiplexColumns = $state(1);
  let tuneMultiplexSlots = $state<Record<string, string | null>>({});
  let tuneMultiplexSlotOutputs = $state<Record<string, string | null>>({});
  let tuneMultiplexBusy = $state(false);
  let tuneMultiplexError = $state<string | null>(null);
  let tuneMultiplexDirty = $state(false);
  let tuneMultiplexAutoApplyTimer = $state<number | null>(null);
  let tuneMultiplexHydratedStreamId = $state<string | null>(null);
  let tuneMultiplexHydratedSignature = $state<string | null>(null);
  let tuneMultiplexLastAppliedSignature = $state<string | null>(null);
  let tuneMultiplexDragPipelineId = $state<string | null>(null);
  let tuneMultiplexDragSource = $state<{ row: number; column: number } | null>(null);
  let tuneStreamControls = $state<ControlMeta[]>([]);
  let tuneControlState = $state<Record<number, number | boolean | null>>({});
  let tuneControlAppliedState = $state<Record<number, number | boolean | null>>({});
  let tuneControlBusy = $state<Record<number, boolean>>({});
  let tuneControlSocket = $state<StreamControlSocket | null>(null);
  let tuneControlSocketStreamId = $state<string | null>(null);
  const CONTROL_APPLY_DEBOUNCE_MS = 150;
  const tuneControlApplyTimers = new SvelteMap<number, number>();
  const tuneControlApplySeqById = new SvelteMap<number, number>();
  let tuneControlsQuery = $state('');
  let tuneShowReadOnlyControls = $state(false);
  let tuneControlsLoading = $state(false);
  let tuneControlsError = $state<string | null>(null);
  let tuneControlsRequestId = $state(0);
  let tuneControlsLoadedStreamId = $state<string | null>(null);

  let tuneSelectedPipelineOutput = $state<string | null>(null);
  let tunePipelineRemoveModalOpen = $state(false);
  let tunePipelineRemoveCandidateId = $state<string | null>(null);
  let tunePipelineAssignModalOpen = $state(false);
  let tunePipelineAssignDraft = $state<string[]>([]);
  let tunePipelineAssignQuery = $state('');
  const tunePipelineAssignFilteredGraphs: unknown[] = $derived.by(() => []);
  const tunePipelineGraphs: unknown[] = [];

  let tuneGlobalAutoSaveTimer = $state<number | null>(null);
  let lastTunePipelineId = $state<string | null>(null);

  const editorState = createTuneWorkspaceEditorState({
    browser,
    activeTab,
    selectedPipeline,
    PipelinesApi,
    StreamsApi,
    PIPELINE_UI_METADATA_KEY,
    DEFAULT_PIPELINE_UI,
    pipelineLabelById,
    toaster,
    reportError,
    buildErrorMessage,
    getTuneUiMode: () => tuneUiMode,
    getTuneUiEditMode: () => tuneUiEditMode,
    setTuneUiEditMode: (next) => {
      tuneUiEditMode = next;
    },
    setTuneUiSelectedItemId: (next) => {
      tuneUiSelectedItemId = next;
    },
    setTuneUiSelectedItemAnchor: (next) => {
      tuneUiSelectedItemAnchor = next;
    },
    getTunePipelineUiDraft: () => tunePipelineUiDraft,
    setTunePipelineUiDraft: (next) => {
      tunePipelineUiDraft = next;
    },
    getTuneNodeDrafts: () => tuneNodeDrafts,
    setTuneNodeDrafts: (next) => {
      tuneNodeDrafts = next;
    },
    getTuneNodeErrors: () => tuneNodeErrors,
    setTuneNodeErrors: (next) => {
      tuneNodeErrors = next;
    },
    getTuneStreamNodeDraftsById: () => tuneStreamNodeDraftsById,
    setTuneStreamNodeDraftsById: (next) => {
      tuneStreamNodeDraftsById = next;
    },
    getTuneStreamNodeErrorsById: () => tuneStreamNodeErrorsById,
    setTuneStreamNodeErrorsById: (next) => {
      tuneStreamNodeErrorsById = next;
    },
    streamGraphForPipeline,
    getTuneStreamNodeOverridesById: () => tuneStreamNodeOverridesById,
    setTuneStreamNodeOverridesById: (next) => {
      tuneStreamNodeOverridesById = next;
    },
    getTuneStreamLastAppliedNodeOverridesById: () => tuneStreamLastAppliedNodeOverridesById,
    setTuneStreamLastAppliedNodeOverridesById: (next) => {
      tuneStreamLastAppliedNodeOverridesById = next;
    },
    getTuneStreamOverridesLoaded: () => tuneStreamOverridesLoaded,
    setTuneStreamOverridesLoaded: (next) => {
      tuneStreamOverridesLoaded = next;
    },
    getTuneStreamInputOverridesById: () => tuneStreamInputOverridesById,
    setTuneStreamInputOverridesById: (next) => {
      tuneStreamInputOverridesById = next;
    }
  });

  const derivedState = createTuneWorkspaceDerivedState({
    browser,
    activeTab,
    selectedPipeline,
    editingPlan,
    getTuneStreams: () => tuneStreams,
    getTuneScopeTab: () => tuneScopeTab,
    setTuneScopeTab: (next) => {
      tuneScopeTab = next;
    },
    getTuneUiEditMode: () => tuneUiEditMode,
    setTuneUiEditMode: (next) => {
      tuneUiEditMode = next;
    },
    getTunePipelineUiSearch: () => tunePipelineUiSearch,
    setTunePipelineUiSearch: (next) => {
      tunePipelineUiSearch = next;
    },
    getTunePipelineUiDraft: () => tunePipelineUiDraft,
    setTunePipelineUiDraft: (next) => {
      tunePipelineUiDraft = next;
    },
    getTuneUiActiveTabId: () => tuneUiActiveTabId,
    setTuneUiActiveTabId: (next) => {
      tuneUiActiveTabId = next;
    },
    getTuneUiSelectedItemId: () => tuneUiSelectedItemId,
    setTuneUiSelectedItemId: (next) => {
      tuneUiSelectedItemId = next;
    },
    getTuneUiSelectedItemAnchor: () => tuneUiSelectedItemAnchor,
    setTuneUiSelectedItemAnchor: (next) => {
      tuneUiSelectedItemAnchor = next;
    },
    getTunePerformanceTab: () => tunePerformanceTab,
    setTunePerformanceTab: (next) => {
      tunePerformanceTab = next;
    },
    getTuneControlsQuery: () => tuneControlsQuery,
    setTuneControlsQuery: (next) => {
      tuneControlsQuery = next;
    },
    getTuneShowReadOnlyControls: () => tuneShowReadOnlyControls,
    setTuneShowReadOnlyControls: (next) => {
      tuneShowReadOnlyControls = next;
    },
    getTuneControlState: () => tuneControlState,
    setTuneControlState: (next) => {
      tuneControlState = next;
    },
    getTuneControlAppliedState: () => tuneControlAppliedState,
    setTuneControlAppliedState: (next) => {
      tuneControlAppliedState = next;
    },
    getTuneControlBusy: () => tuneControlBusy,
    setTuneControlBusy: (next) => {
      tuneControlBusy = next;
    },
    getTuneMultiplexRows: () => tuneMultiplexRows,
    setTuneMultiplexRows: (next) => {
      tuneMultiplexRows = next;
    },
    getTuneMultiplexColumns: () => tuneMultiplexColumns,
    setTuneMultiplexColumns: (next) => {
      tuneMultiplexColumns = next;
    },
    getTuneSelectedPipelineOutput: () => tuneSelectedPipelineOutput,
    setTuneSelectedPipelineOutput: (next) => {
      tuneSelectedPipelineOutput = next;
    },
    getTunePipelineRemoveModalOpen: () => tunePipelineRemoveModalOpen,
    setTunePipelineRemoveModalOpen: (next) => {
      tunePipelineRemoveModalOpen = next;
    },
    getTunePipelineRemoveCandidateId: () => tunePipelineRemoveCandidateId,
    setTunePipelineRemoveCandidateId: (next) => {
      tunePipelineRemoveCandidateId = next;
    },
    getTunePipelineAssignModalOpen: () => tunePipelineAssignModalOpen,
    setTunePipelineAssignModalOpen: (next) => {
      tunePipelineAssignModalOpen = next;
    },
    getTunePipelineAssignQuery: () => tunePipelineAssignQuery,
    setTunePipelineAssignQuery: (next) => {
      tunePipelineAssignQuery = next;
    },
    getTunePipelineAssignDraft: () => tunePipelineAssignDraft,
    setTunePipelineAssignDraft: (next) => {
      tunePipelineAssignDraft = next;
    },
    streamUsesPipeline,
    streamLabel,
    streamGraphForPipeline,
    outputOptionsForPipeline,
    resolveDataTypeKey,
    getDataTypeVariants,
    resolveRegistryEntryForNode,
    extractTuneConstantEntries,
    normalizePortKey,
    RAW_STREAM_PIPELINE_ID,
    PipelinesApi
  });

  function handleTuneAssign() {
    if (!get(selectedPipeline)) {
      toaster.info({ title: 'Select a pipeline', description: 'Select a pipeline before assigning.' });
      return;
    }
    openAssignModal();
  }

  const interactionState = createTuneWorkspaceInteractionState({
    browser,
    pipelines,
    getTunePreviewStream: () => derivedState.tunePreviewStream,
    getTuneControlSocket: () => tuneControlSocket,
    setTuneControlSocket: (next) => {
      tuneControlSocket = next;
    },
    getTuneControlSocketStreamId: () => tuneControlSocketStreamId,
    setTuneControlSocketStreamId: (next) => {
      tuneControlSocketStreamId = next;
    },
    tuneControlApplyTimers,
    tuneControlApplySeqById,
    getTuneControlState: () => tuneControlState,
    setTuneControlState: (next) => {
      tuneControlState = next;
    },
    getTuneControlAppliedState: () => tuneControlAppliedState,
    setTuneControlAppliedState: (next) => {
      tuneControlAppliedState = next;
    },
    getTuneControlBusy: () => tuneControlBusy,
    setTuneControlBusy: (next) => {
      tuneControlBusy = next;
    },
    getTuneStreamControls: () => tuneStreamControls,
    getTuneControlsQuery: () => tuneControlsQuery,
    getTuneShowReadOnlyControls: () => tuneShowReadOnlyControls,
    controlApplyDebounceMs: CONTROL_APPLY_DEBOUNCE_MS,
    StreamsApi,
    toaster,
    reportError,
    buildErrorMessage,
    clampControlValue,
    buildControlValue,
    getTuneMultiplexRows: () => tuneMultiplexRows,
    setTuneMultiplexRows: (next) => {
      tuneMultiplexRows = next;
    },
    getTuneMultiplexColumns: () => tuneMultiplexColumns,
    setTuneMultiplexColumns: (next) => {
      tuneMultiplexColumns = next;
    },
    getTuneMultiplexSlots: () => tuneMultiplexSlots,
    setTuneMultiplexSlots: (next) => {
      tuneMultiplexSlots = next;
    },
    getTuneMultiplexSlotOutputs: () => tuneMultiplexSlotOutputs,
    setTuneMultiplexSlotOutputs: (next) => {
      tuneMultiplexSlotOutputs = next;
    },
    getTuneMultiplexDragPipelineId: () => tuneMultiplexDragPipelineId,
    setTuneMultiplexDragPipelineId: (next) => {
      tuneMultiplexDragPipelineId = next;
    },
    getTuneMultiplexDragSource: () => tuneMultiplexDragSource,
    setTuneMultiplexDragSource: (next) => {
      tuneMultiplexDragSource = next;
    },
    getTuneSelectedPipelineOutput: () => tuneSelectedPipelineOutput,
    setTuneSelectedPipelineOutput: (next) => {
      tuneSelectedPipelineOutput = next;
    },
    getTuneMultiplexAutoApplyTimer: () => tuneMultiplexAutoApplyTimer,
    setTuneMultiplexAutoApplyTimer: (next) => {
      tuneMultiplexAutoApplyTimer = next;
    },
    getTuneMultiplexBusy: () => tuneMultiplexBusy,
    setTuneMultiplexBusy: (next) => {
      tuneMultiplexBusy = next;
    },
    setTuneMultiplexError: (next) => {
      tuneMultiplexError = next;
    },
    setTuneMultiplexDirty: (next) => {
      tuneMultiplexDirty = next;
    },
    getTuneMultiplexLastAppliedSignature: () => tuneMultiplexLastAppliedSignature,
    setTuneMultiplexLastAppliedSignature: (next) => {
      tuneMultiplexLastAppliedSignature = next;
    },
    setTuneMultiplexHydratedSignature: (next) => {
      tuneMultiplexHydratedSignature = next;
    },
    setTuneMultiplexHydratedStreamId: (next) => {
      tuneMultiplexHydratedStreamId = next;
    },
    resolvePipelineLabel: pipelineLabelById,
    RAW_STREAM_PIPELINE_ID,
    RAW_STREAM_PIPELINE_UUID,
    outputOptionsForPipeline: derivedState.outputOptionsForTunePipeline,
    serializeGraphPlan,
    fetchTuneStreams: editorState.fetchTuneStreams,
    buildTuneMultiplexSignature
  });

  const orchestrationState = createTuneWorkspaceOrchestrationState({
    browser,
    activeTab,
    selectedPipeline,
    assignBusy,
    pipelineUpdatesReady,
    streamUpdatesReadyById,
    getStreamUpdatesSocket,
    pipelineUpdates,
    saveCurrentPipeline,
    handlePlanChange,
    setNodeConstantValue,
    StreamsApi,
    buildErrorMessage,
    reportError,
    toaster,
    streamUsesPipeline,
    streamLabel,
    extractGraphAlias,
    asRecord,
    asStreamInfo,
    refreshPipelineMetrics,
    derivedState: {
      tunePlan: derivedState.tunePlan,
      tuneStreamsForPipeline: derivedState.tuneStreamsForPipeline,
      tuneSelectedStream: derivedState.tuneSelectedStream,
      tunePreviewStream: derivedState.tunePreviewStream,
      tuneMetricsWantedKey: derivedState.tuneMetricsWantedKey,
      tuneMetricsWantedRefs: derivedState.tuneMetricsWantedRefs
    },
    editorState: {
      fetchTuneStreams: editorState.fetchTuneStreams,
      seedStreamOverrides: editorState.seedStreamOverrides,
      setTuneNodeDraft: editorState.setTuneNodeDraft,
      setTuneNodeError: editorState.setTuneNodeError,
      setTuneStreamNodeDraft: editorState.setTuneStreamNodeDraft,
      clearTuneStreamNodeDraft: editorState.clearTuneStreamNodeDraft,
      setTuneStreamNodeError: editorState.setTuneStreamNodeError
    },
    interactionState: {
      closeTuneControlSocket: interactionState.closeTuneControlSocket,
      ensureTuneControlSocket: interactionState.ensureTuneControlSocket
    },
    getLastTunePipelineId: () => lastTunePipelineId,
    setLastTunePipelineId: (next) => {
      lastTunePipelineId = next;
    },
    getTuneScopeTab: () => tuneScopeTab,
    setTuneScopeTab: (next) => {
      tuneScopeTab = next;
    },
    getTuneStreams: () => tuneStreams,
    setTuneStreams: (next) => {
      tuneStreams = next;
    },
    getTuneStreamsLoaded: () => tuneStreamsLoaded,
    setTuneStreamsLoaded: (next) => {
      tuneStreamsLoaded = next;
    },
    getTuneStreamsLoading: () => tuneStreamsLoading,
    setTuneStreamsLoading: (next) => {
      tuneStreamsLoading = next;
    },
    getTuneStreamsError: () => tuneStreamsError,
    setTuneStreamsError: (next) => {
      tuneStreamsError = next;
    },
    getTuneStreamsRetryTimer: () => tuneStreamsRetryTimer,
    setTuneStreamsRetryTimer: (next) => {
      tuneStreamsRetryTimer = next;
    },
    getTuneAssignBusySeen: () => tuneAssignBusySeen,
    setTuneAssignBusySeen: (next) => {
      tuneAssignBusySeen = next;
    },
    getTuneNodeDrafts: () => tuneNodeDrafts,
    setTuneNodeDrafts: (next) => {
      tuneNodeDrafts = next;
    },
    getTuneNodeErrors: () => tuneNodeErrors,
    setTuneNodeErrors: (next) => {
      tuneNodeErrors = next;
    },
    getTuneStreamInputOverridesById: () => tuneStreamInputOverridesById,
    setTuneStreamInputOverridesById: (next) => {
      tuneStreamInputOverridesById = next;
    },
    getTuneStreamNodeOverridesById: () => tuneStreamNodeOverridesById,
    setTuneStreamNodeOverridesById: (next) => {
      tuneStreamNodeOverridesById = next;
    },
    getTuneStreamNodeDraftsById: () => tuneStreamNodeDraftsById,
    setTuneStreamNodeDraftsById: (next) => {
      tuneStreamNodeDraftsById = next;
    },
    getTuneStreamNodeErrorsById: () => tuneStreamNodeErrorsById,
    setTuneStreamNodeErrorsById: (next) => {
      tuneStreamNodeErrorsById = next;
    },
    getTuneStreamOverridesLoaded: () => tuneStreamOverridesLoaded,
    setTuneStreamOverridesLoaded: (next) => {
      tuneStreamOverridesLoaded = next;
    },
    getTunePerformanceTab: () => tunePerformanceTab,
    setTunePerformanceTab: (next) => {
      tunePerformanceTab = next;
    },
    getTuneMultiplexDirty: () => tuneMultiplexDirty,
    setTuneMultiplexDirty: (next) => {
      tuneMultiplexDirty = next;
    },
    getTuneMultiplexHydratedStreamId: () => tuneMultiplexHydratedStreamId,
    setTuneMultiplexHydratedStreamId: (next) => {
      tuneMultiplexHydratedStreamId = next;
    },
    getTuneMultiplexHydratedSignature: () => tuneMultiplexHydratedSignature,
    setTuneMultiplexHydratedSignature: (next) => {
      tuneMultiplexHydratedSignature = next;
    },
    getTuneMultiplexLastAppliedSignature: () => tuneMultiplexLastAppliedSignature,
    setTuneMultiplexLastAppliedSignature: (next) => {
      tuneMultiplexLastAppliedSignature = next;
    },
    RAW_STREAM_PIPELINE_ID,
    RAW_STREAM_PIPELINE_UUID,
    getTuneMultiplexError: () => tuneMultiplexError,
    setTuneMultiplexError: (next) => {
      tuneMultiplexError = next;
    },
    getTuneMultiplexBusy: () => tuneMultiplexBusy,
    setTuneMultiplexBusy: (next) => {
      tuneMultiplexBusy = next;
    },
    getTuneMultiplexAutoApplyTimer: () => tuneMultiplexAutoApplyTimer,
    setTuneMultiplexAutoApplyTimer: (next) => {
      tuneMultiplexAutoApplyTimer = next;
    },
    getTuneMultiplexSlots: () => tuneMultiplexSlots,
    setTuneMultiplexSlots: (next) => {
      tuneMultiplexSlots = next;
    },
    getTuneMultiplexSlotOutputs: () => tuneMultiplexSlotOutputs,
    setTuneMultiplexSlotOutputs: (next) => {
      tuneMultiplexSlotOutputs = next;
    },
    setTuneStreamControls: (next) => {
      tuneStreamControls = next;
    },
    getTuneControlState: () => tuneControlState,
    setTuneControlState: (next) => {
      tuneControlState = next;
    },
    getTuneControlAppliedState: () => tuneControlAppliedState,
    setTuneControlAppliedState: (next) => {
      tuneControlAppliedState = next;
    },
    getTuneControlBusy: () => tuneControlBusy,
    setTuneControlBusy: (next) => {
      tuneControlBusy = next;
    },
    getTuneControlsLoading: () => tuneControlsLoading,
    setTuneControlsLoading: (next) => {
      tuneControlsLoading = next;
    },
    getTuneControlsError: () => tuneControlsError,
    setTuneControlsError: (next) => {
      tuneControlsError = next;
    },
    getTuneControlsRequestId: () => tuneControlsRequestId,
    setTuneControlsRequestId: (next) => {
      tuneControlsRequestId = next;
    },
    getTuneControlsLoadedStreamId: () => tuneControlsLoadedStreamId,
    setTuneControlsLoadedStreamId: (next) => {
      tuneControlsLoadedStreamId = next;
    },
    setTuneControlsQuery: (next) => {
      tuneControlsQuery = next;
    },
    setTuneShowReadOnlyControls: (next) => {
      tuneShowReadOnlyControls = next;
    },
    getTuneMultiplexRows: () => tuneMultiplexRows,
    setTuneMultiplexRows: (next) => {
      tuneMultiplexRows = next;
    },
    getTuneMultiplexColumns: () => tuneMultiplexColumns,
    setTuneMultiplexColumns: (next) => {
      tuneMultiplexColumns = next;
    },
    getTuneGlobalAutoSaveTimer: () => tuneGlobalAutoSaveTimer,
    setTuneGlobalAutoSaveTimer: (next) => {
      tuneGlobalAutoSaveTimer = next;
    },
    getTuneStreamLastAppliedNodeOverridesById: () => tuneStreamLastAppliedNodeOverridesById,
    setTuneStreamLastAppliedNodeOverridesById: (next) => {
      tuneStreamLastAppliedNodeOverridesById = next;
    },
    getTuneStreamLastAppliedSignatureById: () => tuneStreamLastAppliedSignatureById,
    setTuneStreamLastAppliedSignatureById: (next) => {
      tuneStreamLastAppliedSignatureById = next;
    },
    getTuneStreamApplyBusyById: () => tuneStreamApplyBusyById,
    setTuneStreamApplyBusyById: (next) => {
      tuneStreamApplyBusyById = next;
    },
    getTuneStreamApplyQueuedById: () => tuneStreamApplyQueuedById,
    setTuneStreamApplyQueuedById: (next) => {
      tuneStreamApplyQueuedById = next;
    },
    getTuneStreamApplyErrorById: () => tuneStreamApplyErrorById,
    setTuneStreamApplyErrorById: (next) => {
      tuneStreamApplyErrorById = next;
    },
    getTuneStreamApplyRafById: () => tuneStreamApplyRafById,
    setTuneStreamApplyRafById: (next) => {
      tuneStreamApplyRafById = next;
    },
    getTuneStreamAutoApplyTimerById: () => tuneStreamAutoApplyTimerById,
    setTuneStreamAutoApplyTimerById: (next) => {
      tuneStreamAutoApplyTimerById = next;
    },
    buildDaedalusGraphPatch,
    mergeNodeOverrides,
    serializeGraphPlan,
    safeClonePlan,
    isDaedalusPlan,
    streamOverrideSignature,
    normalizePortKey,
    resolveDataTypeKey,
    getDataTypeVariants,
    buildNodeValueFromInput
  });

  const ctx = $derived.by(() => ({
    CONTROL_APPLY_DEBOUNCE_MS,
    PIPELINE_UI_METADATA_KEY,
    RAW_STREAM_PIPELINE_ID,
    RAW_STREAM_PIPELINE_UUID,
    TUNE_METRICS_POLL_MS: orchestrationState.metricsState.TUNE_METRICS_POLL_MS,
    applyStreamControl: interactionState.applyStreamControl,
    applyTuneMultiplex: interactionState.applyTuneMultiplex,
    applyTuneStreamOverridesFor: orchestrationState.applyTuneStreamOverridesFor,
    asRecord,
    boundTunePipelines: interactionState.boundTunePipelines,
    buildControlValue,
    buildPipelineMetricsSummary,
    buildTuneMetricsSnapshot,
    buildTuneMultiplexLayout: interactionState.buildTuneMultiplexLayout,
    buildTuneMultiplexSignature,
    buildTuneMultiplexStateSignature: interactionState.buildTuneMultiplexStateSignature,
    clampControlValue,
    clearTuneMultiplexCell: interactionState.clearTuneMultiplexCell,
    clearTuneNodeDraft: editorState.clearTuneNodeDraft,
    clearTuneStreamNodeDraft: editorState.clearTuneStreamNodeDraft,
    closeTuneControlSocket: interactionState.closeTuneControlSocket,
    controlMax,
    controlMin,
    controlStep,
    displayControlValue,
    extractControlValue,
    dropTuneMultiplexOn: interactionState.dropTuneMultiplexOn,
    ensureTuneControlSocket: interactionState.ensureTuneControlSocket,
    fetchTuneMetricsSnapshots: orchestrationState.metricsState.fetchTuneMetricsSnapshots,
    fetchTuneStreams: editorState.fetchTuneStreams,
    handleTuneAssign,
    lastTunePipelineId,
    markTuneMultiplexDirty: interactionState.markTuneMultiplexDirty,
    menuOptions,
    menuValueDisplay,
    metricsSource: orchestrationState.metricsState.metricsSource,
    metricsStatusLabel: orchestrationState.metricsState.metricsStatusLabel,
    metricsUpdatedLabel: orchestrationState.metricsState.metricsUpdatedLabel,
    pipelineMetricsSummary: orchestrationState.metricsState.pipelineMetricsSummary,
    multiplexKey,
    normalizeMultiplexSlots,
    readTuneNodeDraft: editorState.readTuneNodeDraft,
    readTuneStreamNodeDraft: editorState.readTuneStreamNodeDraft,
    resetTunePipelineUi: editorState.resetTunePipelineUi,
    resolveTunePipelineGraph: interactionState.resolveTunePipelineGraph,
    resolveTunePipelineOutput: interactionState.resolveTunePipelineOutput,
    scheduleControlApply: interactionState.scheduleControlApply,
    scheduleTuneGlobalAutoSave: orchestrationState.scheduleTuneGlobalAutoSave,
    scheduleTuneMultiplexAutoApply: interactionState.scheduleTuneMultiplexAutoApply,
    scheduleTuneStreamAutoApply: orchestrationState.scheduleTuneStreamAutoApply,
    seedControlState,
    seedStreamOverrides: editorState.seedStreamOverrides,
    saveTunePipelineUi: editorState.saveTunePipelineUi,
    setTuneLivePipelineOutput: interactionState.setTuneLivePipelineOutput,
    setTuneMultiplexGridDimensions: interactionState.setTuneMultiplexGridDimensions,
    setTuneNodeDraft: editorState.setTuneNodeDraft,
    setTuneNodeError: editorState.setTuneNodeError,
    setTuneOutputKeyForCell: interactionState.setTuneOutputKeyForCell,
    setTuneOutputSelectionForPipeline: interactionState.setTuneOutputSelectionForPipeline,
    setTuneStreamNodeDraft: editorState.setTuneStreamNodeDraft,
    setTuneStreamNodeError: editorState.setTuneStreamNodeError,
    startTuneMultiplexDrag: interactionState.startTuneMultiplexDrag,
    tuneActiveStreamId: derivedState.tuneActiveStreamId,
    tuneAllowDrop: interactionState.tuneAllowDrop,
    tuneConstantGroups: derivedState.tuneConstantGroups,
    tuneConstantSearch: derivedState.tuneConstantSearch,
    tuneConstantSearchTokens: derivedState.tuneConstantSearchTokens,
    tuneConstants: derivedState.tuneConstants,
    tuneControlAppliedState,
    tuneControlApplySeqById,
    tuneControlApplyTimers,
    tuneControlBusy,
    tuneControlSocket,
    tuneControlSocketStreamId,
    tuneControlState,
    tuneControlsError,
    tuneControlsLoadedStreamId,
    tuneControlsLoading,
    tuneControlsQuery,
    tuneControlsRequestId,
    tuneBindings: derivedState.tuneBindings,
    tuneFilteredConstantGroups: derivedState.tuneFilteredConstantGroups,
    tuneFilteredControls: interactionState.tuneFilteredControls,
    tuneGlobalAutoSaveTimer,
    tuneMetricsError: orchestrationState.metricsState.tuneMetricsError,
    tuneMetricsInFlight: orchestrationState.metricsState.tuneMetricsInFlight,
    tuneMetricsPollTimer: orchestrationState.metricsState.tuneMetricsPollTimer,
    tuneMetricsRefreshPipelineId: orchestrationState.metricsState.tuneMetricsRefreshPipelineId,
    tuneMetricsRequestId: orchestrationState.metricsState.tuneMetricsRequestId,
    tuneMetricsSnapshots: orchestrationState.metricsState.tuneMetricsSnapshots,
    tuneMetricsStatus: orchestrationState.metricsState.tuneMetricsStatus,
    tuneMetricsUpdatedAt: orchestrationState.metricsState.tuneMetricsUpdatedAt,
    tuneMultiplexAutoApplyTimer,
    tuneMultiplexBusy,
    tuneMultiplexColumnIndices: interactionState.tuneMultiplexColumnIndices,
    tuneMultiplexColumns,
    tuneMultiplexDirty,
    tuneMultiplexDragPipelineId,
    tuneMultiplexDragSource,
    tuneMultiplexError,
    tuneMultiplexGridIsSingle: interactionState.tuneMultiplexGridIsSingle,
    tuneMultiplexHydratedSignature,
    tuneMultiplexHydratedStreamId,
    tuneMultiplexLayoutSignature: interactionState.tuneMultiplexLayoutSignature,
    tuneMultiplexLastAppliedSignature,
    tuneMultiplexOutputOptionsCache: interactionState.tuneMultiplexOutputOptionsCache,
    tuneMultiplexPalettePipelineIds: interactionState.tuneMultiplexPalettePipelineIds,
    tuneMultiplexRowIndices: interactionState.tuneMultiplexRowIndices,
    tuneMultiplexRows,
    tuneMultiplexSlotOutputs,
    tuneMultiplexSlots,
    tuneNodeDescriptors: derivedState.tuneNodeDescriptors,
    tuneNodeDrafts,
    tuneNodeErrors,
    tuneOutputKeyForCell: interactionState.tuneOutputKeyForCell,
    tuneOutputSelectionForPipeline: interactionState.tuneOutputSelectionForPipeline,
    tunePerformanceTab,
    tunePipelineAssignDraft,
    tunePipelineAssignFilteredGraphs,
    tunePipelineAssignModalOpen,
    tunePipelineAssignQuery,
    tunePipelineForCell: interactionState.tunePipelineForCell,
    tunePipelineGraphs,
    tunePipelineRemoveCandidateId,
    tunePipelineRemoveModalOpen,
    tunePipelineUiDraft,
    tunePipelineUiSearch,
    tunePlan: derivedState.tunePlan,
    tunePreviewStream: derivedState.tunePreviewStream,
    tuneRuntimeFromStats,
    tuneScopeTab,
    tuneSelectedPipelineOutput,
    tuneSelectedStream: derivedState.tuneSelectedStream,
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
    tuneStreamsForPipeline: derivedState.tuneStreamsForPipeline,
    tuneStreamsLoaded,
    tuneStreamsLoading,
    tuneUiActiveTabId,
    tuneUiEditMode,
    tuneUiMode,
    tuneUiSelectedItemAnchor,
    tuneUiSelectedItemId,
    updateGlobalNodeValue: orchestrationState.updateGlobalNodeValue,
    updateStreamNodeValue: orchestrationState.updateStreamNodeValue
  }));

  return () => ctx;
}

export type PipelineTuneWorkspaceContextGetter = ReturnType<typeof createPipelineTuneWorkspaceContext>;
export type PipelineTuneWorkspaceContext = ReturnType<PipelineTuneWorkspaceContextGetter>;
