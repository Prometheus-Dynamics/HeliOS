import { get, type Readable } from 'svelte/store';
import { onDestroy, untrack } from 'svelte';
import type { ControlMeta, StreamInfo } from '$lib/api/client';
import type { StreamsApi as SharedStreamsApi } from '$lib/api/streamsApi';
import type { PipelineGraphPlan, PipelineNodeValue, PipelineOverviewPipeline } from '$lib/types/pipeline';
import { seedControlState } from './pipelineStreamControlUtils';
import { createTuneControlLoader } from './pipelineTuneControlLoader';
import { createTuneWorkspaceMetricsState } from './pipelineTuneWorkspaceMetricsState.svelte';
import {
  runTunePipelineReset,
  runTuneStreamsLoad,
  runTuneScopeTabSync,
  runTuneStreamOverridesSeed,
  runTuneMultiplexHydration,
  runTunePerformanceTabSync,
  runTuneControlsLoad,
  runTuneControlSocketSync
} from './pipelineTuneEffects';
import { resetTuneState } from './pipelineTuneReset';
import { createTuneStreamOverrideRuntime } from './pipelineTuneStreamOverrides';
import { createTuneNodeHandlers } from './pipelineTuneNodeHandlers';

type StreamsApi = Pick<
  typeof SharedStreamsApi,
  | 'resolvedStreams'
  | 'getControls'
  | 'getMetrics'
  | 'setControl'
  | 'setPipelineGraph'
  | 'setPipelineGraphPatch'
  | 'setPipelineInputs'
  | 'setPipelineOutput'
  | 'setPipelineLayout'
  | 'smokePipelineGraph'
>;

type PipelineUpdates = { sendPipeline: (payload: unknown) => boolean | void };

type DerivedState = {
  tunePlan: PipelineGraphPlan | null;
  tuneStreamsForPipeline: StreamInfo[];
  tuneSelectedStream: StreamInfo | null;
  tunePreviewStream: StreamInfo | null;
  tuneMetricsWantedKey: string;
  tuneMetricsWantedRefs: Array<{ id: string; label: string }>;
};

type EditorState = {
  fetchTuneStreams: () => Promise<StreamInfo[]>;
  seedStreamOverrides: (stream: StreamInfo, pipelineId: string, baseGraph: PipelineGraphPlan | null) => void;
  setTuneNodeDraft: (nodeId: string, portKey: string, value: string) => void;
  setTuneNodeError: (nodeId: string, portKey: string, value: string | null) => void;
  setTuneStreamNodeDraft: (streamId: string, nodeId: string, portKey: string, value: string) => void;
  clearTuneStreamNodeDraft: (streamId: string, nodeId: string, portKey: string) => void;
  setTuneStreamNodeError: (streamId: string, nodeId: string, portKey: string, value: string | null) => void;
};

type InteractionState = {
  closeTuneControlSocket: () => void;
  ensureTuneControlSocket: (streamId: string) => void;
};

type Args = {
  browser: boolean;
  activeTab: Readable<'pipeline' | 'tune'>;
  selectedPipeline: Readable<PipelineOverviewPipeline | null>;
  assignBusy: Readable<boolean>;
  pipelineUpdatesReady: Readable<boolean>;
  streamUpdatesReadyById: Readable<Record<string, boolean>>;
  getStreamUpdatesSocket: (streamId: string) => { ready?: () => boolean; send?: (payload: Record<string, unknown>) => boolean } | null;
  pipelineUpdates: PipelineUpdates;
  saveCurrentPipeline: () => Promise<void>;
  handlePlanChange: (plan: PipelineGraphPlan) => void;
  setNodeConstantValue: (nodeId: string, portKey: string, value: PipelineNodeValue) => void;
  StreamsApi: StreamsApi;
  buildErrorMessage: (input: { error: unknown; fallback: string }) => string;
  reportError: (params: { title: string; error: unknown; fallback: string }) => void;
  toaster: { success: (payload: { title: string }) => void; error: (payload: { title: string; description?: string }) => void };
  streamUsesPipeline: (stream: StreamInfo, pipelineId: string) => boolean;
  streamLabel: (stream: StreamInfo) => string;
  extractGraphAlias: (graph: unknown) => string | null;
  asRecord: (value: unknown) => Record<string, unknown> | null;
  asStreamInfo: (value: unknown) => StreamInfo | null;
  refreshPipelineMetrics: () => void;
  derivedState: DerivedState;
  editorState: EditorState;
  interactionState: InteractionState;
  getLastTunePipelineId: () => string | null;
  setLastTunePipelineId: (next: string | null) => void;
  getTuneScopeTab: () => 'global' | string;
  setTuneScopeTab: (next: 'global' | string) => void;
  getTuneStreams: () => StreamInfo[];
  setTuneStreams: (next: StreamInfo[]) => void;
  getTuneStreamsLoaded: () => boolean;
  setTuneStreamsLoaded: (next: boolean) => void;
  getTuneStreamsLoading: () => boolean;
  setTuneStreamsLoading: (next: boolean) => void;
  getTuneStreamsError: () => string | null;
  setTuneStreamsError: (next: string | null) => void;
  getTuneStreamsRetryTimer: () => number | null;
  setTuneStreamsRetryTimer: (next: number | null) => void;
  getTuneAssignBusySeen: () => boolean;
  setTuneAssignBusySeen: (next: boolean) => void;
  getTuneNodeDrafts: () => Record<string, Record<string, string>>;
  setTuneNodeDrafts: (next: Record<string, Record<string, string>>) => void;
  getTuneNodeErrors: () => Record<string, Record<string, string | null>>;
  setTuneNodeErrors: (next: Record<string, Record<string, string | null>>) => void;
  getTuneStreamInputOverridesById: () => Record<string, Record<string, PipelineNodeValue>>;
  setTuneStreamInputOverridesById: (next: Record<string, Record<string, PipelineNodeValue>>) => void;
  getTuneStreamNodeOverridesById: () => Record<string, Record<string, Record<string, PipelineNodeValue>>>;
  setTuneStreamNodeOverridesById: (next: Record<string, Record<string, Record<string, PipelineNodeValue>>>) => void;
  getTuneStreamNodeDraftsById: () => Record<string, Record<string, Record<string, string>>>;
  setTuneStreamNodeDraftsById: (next: Record<string, Record<string, Record<string, string>>>) => void;
  getTuneStreamNodeErrorsById: () => Record<string, Record<string, Record<string, string | null>>>;
  setTuneStreamNodeErrorsById: (next: Record<string, Record<string, Record<string, string | null>>>) => void;
  getTuneStreamOverridesLoaded: () => Record<string, boolean>;
  setTuneStreamOverridesLoaded: (next: Record<string, boolean>) => void;
  getTunePerformanceTab: () => 'metrics' | 'controls' | 'layout' | 'outputs';
  setTunePerformanceTab: (next: 'metrics' | 'controls' | 'layout' | 'outputs') => void;
  getTuneMultiplexDirty: () => boolean;
  setTuneMultiplexDirty: (next: boolean) => void;
  getTuneMultiplexHydratedStreamId: () => string | null;
  setTuneMultiplexHydratedStreamId: (next: string | null) => void;
  getTuneMultiplexHydratedSignature: () => string | null;
  setTuneMultiplexHydratedSignature: (next: string | null) => void;
  getTuneMultiplexLastAppliedSignature: () => string | null;
  setTuneMultiplexLastAppliedSignature: (next: string | null) => void;
  RAW_STREAM_PIPELINE_ID: string;
  RAW_STREAM_PIPELINE_UUID: string;
  getTuneMultiplexError: () => string | null;
  setTuneMultiplexError: (next: string | null) => void;
  getTuneMultiplexBusy: () => boolean;
  setTuneMultiplexBusy: (next: boolean) => void;
  getTuneMultiplexAutoApplyTimer: () => number | null;
  setTuneMultiplexAutoApplyTimer: (next: number | null) => void;
  getTuneMultiplexSlots: () => Record<string, string | null>;
  setTuneMultiplexSlots: (next: Record<string, string | null>) => void;
  getTuneMultiplexSlotOutputs: () => Record<string, string | null>;
  setTuneMultiplexSlotOutputs: (next: Record<string, string | null>) => void;
  setTuneStreamControls: (next: ControlMeta[]) => void;
  getTuneControlState: () => Record<number, number | boolean | null>;
  setTuneControlState: (next: Record<number, number | boolean | null>) => void;
  getTuneControlAppliedState: () => Record<number, number | boolean | null>;
  setTuneControlAppliedState: (next: Record<number, number | boolean | null>) => void;
  getTuneControlBusy: () => Record<number, boolean>;
  setTuneControlBusy: (next: Record<number, boolean>) => void;
  getTuneControlsLoading: () => boolean;
  setTuneControlsLoading: (next: boolean) => void;
  getTuneControlsError: () => string | null;
  setTuneControlsError: (next: string | null) => void;
  getTuneControlsRequestId: () => number;
  setTuneControlsRequestId: (next: number) => void;
  getTuneControlsLoadedStreamId: () => string | null;
  setTuneControlsLoadedStreamId: (next: string | null) => void;
  setTuneControlsQuery: (next: string) => void;
  setTuneShowReadOnlyControls: (next: boolean) => void;
  getTuneMultiplexRows: () => number;
  setTuneMultiplexRows: (next: number) => void;
  getTuneMultiplexColumns: () => number;
  setTuneMultiplexColumns: (next: number) => void;
  getTuneGlobalAutoSaveTimer: () => number | null;
  setTuneGlobalAutoSaveTimer: (next: number | null) => void;
  getTuneStreamLastAppliedNodeOverridesById: () => Record<string, Record<string, Record<string, PipelineNodeValue>>>;
  setTuneStreamLastAppliedNodeOverridesById: (next: Record<string, Record<string, Record<string, PipelineNodeValue>>>) => void;
  getTuneStreamLastAppliedSignatureById: () => Record<string, string>;
  setTuneStreamLastAppliedSignatureById: (next: Record<string, string>) => void;
  getTuneStreamApplyBusyById: () => Record<string, boolean>;
  setTuneStreamApplyBusyById: (next: Record<string, boolean>) => void;
  getTuneStreamApplyQueuedById: () => Record<string, boolean>;
  setTuneStreamApplyQueuedById: (next: Record<string, boolean>) => void;
  getTuneStreamApplyErrorById: () => Record<string, string | null>;
  setTuneStreamApplyErrorById: (next: Record<string, string | null>) => void;
  getTuneStreamApplyRafById: () => Record<string, number>;
  setTuneStreamApplyRafById: (next: Record<string, number>) => void;
  getTuneStreamAutoApplyTimerById: () => Record<string, number>;
  setTuneStreamAutoApplyTimerById: (next: Record<string, number>) => void;
  buildDaedalusGraphPatch: (
    plan: PipelineGraphPlan,
    nodeOverrides: Record<string, Record<string, PipelineNodeValue>>,
    previousOverrides?: Record<string, Record<string, PipelineNodeValue>> | null
  ) => { ops: unknown[] };
  mergeNodeOverrides: (
    base: Record<string, Record<string, PipelineNodeValue>>,
    extra: Record<string, Record<string, PipelineNodeValue>>
  ) => Record<string, Record<string, PipelineNodeValue>>;
  serializeGraphPlan: (plan: PipelineGraphPlan) => unknown;
  safeClonePlan: (plan: PipelineGraphPlan) => PipelineGraphPlan;
  isDaedalusPlan: (plan: PipelineGraphPlan | null | undefined) => boolean;
  streamOverrideSignature: (
    pipelineId: string | null | undefined,
    streamId: string,
    inputOverrides: Record<string, PipelineNodeValue>,
    nodeOverrides: Record<string, Record<string, PipelineNodeValue>>
  ) => string;
  normalizePortKey: (value: string | null | undefined) => string;
  resolveDataTypeKey: (dataType: unknown) => string | null;
  getDataTypeVariants: (dataType: unknown) => string[];
  buildNodeValueFromInput: (
    raw: string,
    typeKey: string,
    variants?: string[]
  ) =>
    | { success: true; value: PipelineNodeValue; error?: string }
    | { success: false; error: string };
};

export function createTuneWorkspaceOrchestrationState(args: Args) {
  const metricsState = createTuneWorkspaceMetricsState({
    activeTab: args.activeTab,
    selectedPipeline: args.selectedPipeline,
    getTunePerformanceTab: () => args.getTunePerformanceTab(),
    getTuneScopeTab: () => args.getTuneScopeTab(),
    getTuneMetricsWantedKey: () => args.derivedState.tuneMetricsWantedKey,
    getTuneMetricsWantedRefs: () => args.derivedState.tuneMetricsWantedRefs,
    StreamsApi: args.StreamsApi,
    buildErrorMessage: args.buildErrorMessage,
    streamUsesPipeline: args.streamUsesPipeline,
    streamLabel: args.streamLabel,
    extractGraphAlias: args.extractGraphAlias,
    asRecord: args.asRecord,
    asStreamInfo: args.asStreamInfo,
    refreshPipelineMetrics: args.refreshPipelineMetrics
  });

  $effect(() => {
    runTunePipelineReset({
      currentPipelineId: get(args.selectedPipeline)?.id ?? null,
      lastPipelineId: args.getLastTunePipelineId(),
      setLastPipelineId: args.setLastTunePipelineId,
      reset: () =>
        resetTuneState({
          setTuneNodeDrafts: args.setTuneNodeDrafts,
          setTuneNodeErrors: args.setTuneNodeErrors,
          setTuneStreamInputOverridesById: args.setTuneStreamInputOverridesById,
          setTuneStreamNodeOverridesById: args.setTuneStreamNodeOverridesById,
          setTuneStreamNodeDraftsById: args.setTuneStreamNodeDraftsById,
          setTuneStreamNodeErrorsById: args.setTuneStreamNodeErrorsById,
          setTuneStreamOverridesLoaded: args.setTuneStreamOverridesLoaded,
          setTuneScopeTab: args.setTuneScopeTab,
          setTuneStreamApplyBusyById: args.setTuneStreamApplyBusyById,
          setTuneStreamApplyErrorById: args.setTuneStreamApplyErrorById,
          setTuneStreamApplyQueuedById: args.setTuneStreamApplyQueuedById,
          setTuneStreamLastAppliedSignatureById: args.setTuneStreamLastAppliedSignatureById,
          setTuneStreamLastAppliedNodeOverridesById: args.setTuneStreamLastAppliedNodeOverridesById,
          getTuneStreamAutoApplyTimerById: args.getTuneStreamAutoApplyTimerById,
          setTuneStreamAutoApplyTimerById: args.setTuneStreamAutoApplyTimerById,
          getTuneStreamApplyRafById: args.getTuneStreamApplyRafById,
          setTuneStreamApplyRafById: args.setTuneStreamApplyRafById,
          getTuneGlobalAutoSaveTimer: args.getTuneGlobalAutoSaveTimer,
          setTuneGlobalAutoSaveTimer: args.setTuneGlobalAutoSaveTimer,
          setTuneMultiplexError: args.setTuneMultiplexError,
          setTuneStreamControls: args.setTuneStreamControls,
          setTuneMetricsSnapshots: metricsState.setTuneMetricsSnapshots,
          setTuneMetricsStatus: metricsState.setTuneMetricsStatus,
          setTuneMetricsError: metricsState.setTuneMetricsError,
          setTuneMetricsUpdatedAt: metricsState.setTuneMetricsUpdatedAt,
          getTuneMetricsPollTimer: metricsState.getTuneMetricsPollTimer,
          setTuneMetricsPollTimer: metricsState.setTuneMetricsPollTimer,
          setTuneControlState: args.setTuneControlState,
          setTuneControlAppliedState: args.setTuneControlAppliedState,
          setTuneControlBusy: args.setTuneControlBusy,
          setTuneControlsQuery: args.setTuneControlsQuery,
          setTuneShowReadOnlyControls: args.setTuneShowReadOnlyControls,
          setTuneControlsLoading: args.setTuneControlsLoading,
          setTuneControlsError: args.setTuneControlsError,
          setTuneControlsLoadedStreamId: args.setTuneControlsLoadedStreamId
        })
    });
  });

  $effect(() => {
    runTuneStreamsLoad({
      activeTab: get(args.activeTab),
      tuneStreamsLoaded: args.getTuneStreamsLoaded(),
      tuneStreamsLoading: args.getTuneStreamsLoading(),
      setTuneStreamsLoaded: args.setTuneStreamsLoaded,
      setTuneStreamsLoading: args.setTuneStreamsLoading,
      setTuneStreamsError: args.setTuneStreamsError,
      setTuneStreams: args.setTuneStreams,
      fetchTuneStreams: args.editorState.fetchTuneStreams,
      buildErrorMessage: args.buildErrorMessage
    });
  });

  $effect(() => {
    const busy = get(args.assignBusy);
    if (busy) {
      args.setTuneAssignBusySeen(true);
      return;
    }
    if (!args.getTuneAssignBusySeen()) return;
    args.setTuneAssignBusySeen(false);

    if (get(args.activeTab) !== 'tune') {
      args.setTuneStreamsLoaded(false);
      return;
    }

    let cancelled = false;
    args.setTuneStreamsLoading(true);
    args.setTuneStreamsError(null);
    args.editorState.fetchTuneStreams()
      .then((streams) => {
        if (cancelled) return;
        args.setTuneStreams(Array.isArray(streams) ? streams : []);
        args.setTuneStreamsLoaded(true);
      })
      .catch((error) => {
        if (cancelled) return;
        console.error('Failed to refresh streams after pipeline assignment', error);
        args.setTuneStreamsError(args.buildErrorMessage({ error, fallback: 'Unable to refresh attached streams.' }));
      })
      .finally(() => {
        if (!cancelled) {
          args.setTuneStreamsLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  });

  $effect(() => {
    if (!args.browser) return;
    if (get(args.activeTab) !== 'tune') return;
    if (!args.getTuneStreamsError()) return;
    if (args.getTuneStreamsLoading()) return;
    if (args.getTuneStreams().length > 0) return;
    if (args.getTuneStreamsRetryTimer()) return;

    args.setTuneStreamsRetryTimer(
      window.setTimeout(() => {
        args.setTuneStreamsRetryTimer(null);
        if (get(args.activeTab) === 'tune') {
          args.setTuneStreamsLoaded(false);
        }
      }, 2000)
    );

    return () => {
      const timer = args.getTuneStreamsRetryTimer();
      if (timer) {
        clearTimeout(timer);
        args.setTuneStreamsRetryTimer(null);
      }
    };
  });

  $effect(() => {
    runTuneScopeTabSync({
      activeTab: get(args.activeTab),
      tuneScopeTab: args.getTuneScopeTab(),
      tuneStreamsForPipeline: args.derivedState.tuneStreamsForPipeline,
      setTuneScopeTab: args.setTuneScopeTab
    });
  });

  $effect(() => {
    runTuneStreamOverridesSeed({
      activeTab: get(args.activeTab),
      tuneSelectedStream: args.derivedState.tuneSelectedStream,
      tunePlan: args.derivedState.tunePlan,
      selectedPipelineId: get(args.selectedPipeline)?.id ?? null,
      tuneStreamOverridesLoaded: args.getTuneStreamOverridesLoaded(),
      seedStreamOverrides: args.editorState.seedStreamOverrides
    });
  });

  $effect(() => {
    runTuneMultiplexHydration({
      activeTab: get(args.activeTab),
      tunePreviewStream: args.derivedState.tunePreviewStream,
      tuneMultiplexDirty: args.getTuneMultiplexDirty(),
      tuneMultiplexHydratedStreamId: args.getTuneMultiplexHydratedStreamId(),
      tuneMultiplexHydratedSignature: args.getTuneMultiplexHydratedSignature(),
      tuneMultiplexLastAppliedSignature: args.getTuneMultiplexLastAppliedSignature(),
      RAW_STREAM_PIPELINE_UUID: args.RAW_STREAM_PIPELINE_UUID,
      RAW_STREAM_PIPELINE_ID: args.RAW_STREAM_PIPELINE_ID,
      setTuneMultiplexError: args.setTuneMultiplexError,
      setTuneMultiplexBusy: args.setTuneMultiplexBusy,
      tuneMultiplexAutoApplyTimer: args.getTuneMultiplexAutoApplyTimer(),
      clearMultiplexAutoApplyTimer: () => {
        const timer = args.getTuneMultiplexAutoApplyTimer();
        if (timer) {
          clearTimeout(timer);
          args.setTuneMultiplexAutoApplyTimer(null);
        }
      },
      setTuneMultiplexDirty: args.setTuneMultiplexDirty,
      setTuneMultiplexHydratedStreamId: args.setTuneMultiplexHydratedStreamId,
      setTuneMultiplexHydratedSignature: args.setTuneMultiplexHydratedSignature,
      setTuneMultiplexLastAppliedSignature: args.setTuneMultiplexLastAppliedSignature,
      setTunePerformanceTab: args.setTunePerformanceTab,
      setTuneMultiplexSlots: args.setTuneMultiplexSlots,
      setTuneMultiplexSlotOutputs: args.setTuneMultiplexSlotOutputs,
      setTuneStreamControls: args.setTuneStreamControls,
      setTuneControlState: args.setTuneControlState,
      setTuneControlAppliedState: args.setTuneControlAppliedState,
      setTuneControlBusy: args.setTuneControlBusy,
      setTuneControlsLoading: args.setTuneControlsLoading,
      setTuneControlsError: args.setTuneControlsError,
      setTuneControlsLoadedStreamId: args.setTuneControlsLoadedStreamId,
      setTuneMultiplexRows: args.setTuneMultiplexRows,
      setTuneMultiplexColumns: args.setTuneMultiplexColumns
    });
  });

  $effect(() => {
    runTunePerformanceTabSync({
      activeTab: get(args.activeTab),
      tuneScopeTab: args.getTuneScopeTab(),
      tunePerformanceTab: args.getTunePerformanceTab(),
      setTunePerformanceTab: args.setTunePerformanceTab
    });
  });

  const { loadTuneControls } = untrack(() =>
    createTuneControlLoader({
      StreamsApi: args.StreamsApi,
      buildErrorMessage: args.buildErrorMessage,
      seedControlState,
      setTuneStreamControls: args.setTuneStreamControls,
      setTuneControlState: args.setTuneControlState,
      setTuneControlAppliedState: args.setTuneControlAppliedState,
      setTuneControlsLoadedStreamId: args.setTuneControlsLoadedStreamId,
      setTuneControlsLoading: args.setTuneControlsLoading,
      setTuneControlsError: args.setTuneControlsError,
      getTuneControlsRequestId: args.getTuneControlsRequestId,
      setTuneControlsRequestId: args.setTuneControlsRequestId
    })
  );

  $effect(() => {
    runTuneControlsLoad({
      activeTab: get(args.activeTab),
      tunePerformanceTab: args.getTunePerformanceTab(),
      tunePreviewStream: args.derivedState.tunePreviewStream,
      tuneControlsLoadedStreamId: args.getTuneControlsLoadedStreamId(),
      tuneControlsLoading: args.getTuneControlsLoading(),
      loadTuneControls
    });
  });

  const scheduleTuneGlobalAutoSave = (): void => {
    if (!args.browser) return;
    if (!get(args.selectedPipeline)) return;
    if (get(args.pipelineUpdatesReady)) return;
    const timer = args.getTuneGlobalAutoSaveTimer();
    if (timer) {
      clearTimeout(timer);
    }
    args.setTuneGlobalAutoSaveTimer(
      window.setTimeout(() => {
        args.setTuneGlobalAutoSaveTimer(null);
        void args.saveCurrentPipeline();
      }, 500)
    );
  };

  const { scheduleTuneStreamAutoApply, applyTuneStreamOverridesFor } = untrack(() =>
    createTuneStreamOverrideRuntime({
      browser: args.browser,
      getStreamUpdatesReadyById: (streamId) => Boolean(get(args.streamUpdatesReadyById)[streamId]),
      getSelectedPipeline: () => get(args.selectedPipeline) ?? null,
      getTunePlan: () => args.derivedState.tunePlan,
      getTuneStreamsForPipeline: () => args.derivedState.tuneStreamsForPipeline,
      getTuneStreamInputOverridesById: args.getTuneStreamInputOverridesById,
      getTuneStreamNodeOverridesById: args.getTuneStreamNodeOverridesById,
      getTuneStreamLastAppliedNodeOverridesById: args.getTuneStreamLastAppliedNodeOverridesById,
      setTuneStreamLastAppliedNodeOverridesById: args.setTuneStreamLastAppliedNodeOverridesById,
      getTuneStreamLastAppliedSignatureById: args.getTuneStreamLastAppliedSignatureById,
      setTuneStreamLastAppliedSignatureById: args.setTuneStreamLastAppliedSignatureById,
      getTuneStreamApplyBusyById: args.getTuneStreamApplyBusyById,
      setTuneStreamApplyBusyById: args.setTuneStreamApplyBusyById,
      getTuneStreamApplyQueuedById: args.getTuneStreamApplyQueuedById,
      setTuneStreamApplyQueuedById: args.setTuneStreamApplyQueuedById,
      getTuneStreamApplyErrorById: args.getTuneStreamApplyErrorById,
      setTuneStreamApplyErrorById: args.setTuneStreamApplyErrorById,
      getTuneStreamApplyRafById: args.getTuneStreamApplyRafById,
      setTuneStreamApplyRafById: args.setTuneStreamApplyRafById,
      getTuneStreamAutoApplyTimerById: args.getTuneStreamAutoApplyTimerById,
      setTuneStreamAutoApplyTimerById: args.setTuneStreamAutoApplyTimerById,
      getStreamUpdatesSocket: args.getStreamUpdatesSocket,
      StreamsApi: args.StreamsApi,
      buildErrorMessage: args.buildErrorMessage,
      reportError: args.reportError,
      toaster: args.toaster,
      buildDaedalusGraphPatch: args.buildDaedalusGraphPatch,
      mergeNodeOverrides: args.mergeNodeOverrides,
      serializeGraphPlan: args.serializeGraphPlan,
      safeClonePlan: args.safeClonePlan,
      isDaedalusPlan: args.isDaedalusPlan,
      streamOverrideSignature: args.streamOverrideSignature
    })
  );

  const { updateGlobalNodeValue, updateStreamNodeValue } = untrack(() =>
    createTuneNodeHandlers({
      getTunePlan: () => args.derivedState.tunePlan,
      isDaedalusPlan: args.isDaedalusPlan,
      safeClonePlan: args.safeClonePlan,
      handlePlanChange: args.handlePlanChange,
      pipelineUpdates: args.pipelineUpdates,
      setNodeConstantValue: args.setNodeConstantValue,
      scheduleTuneGlobalAutoSave,
      normalizePortKey: args.normalizePortKey,
      resolveDataTypeKey: args.resolveDataTypeKey,
      getDataTypeVariants: args.getDataTypeVariants,
      buildNodeValueFromInput: args.buildNodeValueFromInput,
      setTuneNodeDraft: args.editorState.setTuneNodeDraft,
      setTuneNodeError: args.editorState.setTuneNodeError,
      setTuneStreamNodeDraft: args.editorState.setTuneStreamNodeDraft,
      clearTuneStreamNodeDraft: args.editorState.clearTuneStreamNodeDraft,
      setTuneStreamNodeError: args.editorState.setTuneStreamNodeError,
      getTuneStreamNodeOverridesById: args.getTuneStreamNodeOverridesById,
      setTuneStreamNodeOverridesById: args.setTuneStreamNodeOverridesById,
      scheduleTuneStreamAutoApply
    })
  );

  $effect(() => {
    runTuneControlSocketSync({
      activeTab: get(args.activeTab),
      tunePerformanceTab: args.getTunePerformanceTab(),
      tunePreviewStream: args.derivedState.tunePreviewStream,
      closeTuneControlSocket: args.interactionState.closeTuneControlSocket,
      ensureTuneControlSocket: args.interactionState.ensureTuneControlSocket
    });
  });

  onDestroy(() => {
    args.interactionState.closeTuneControlSocket();
    const multiplexTimer = args.getTuneMultiplexAutoApplyTimer();
    if (multiplexTimer) {
      clearTimeout(multiplexTimer);
    }
    const saveTimer = args.getTuneGlobalAutoSaveTimer();
    if (saveTimer) {
      clearTimeout(saveTimer);
    }
    const metricsTimer = metricsState.getTuneMetricsPollTimer();
    if (metricsTimer) {
      clearInterval(metricsTimer);
    }
  });

  return {
    applyTuneStreamOverridesFor,
    metricsState,
    scheduleTuneGlobalAutoSave,
    scheduleTuneStreamAutoApply,
    updateGlobalNodeValue,
    updateStreamNodeValue
  };
}
