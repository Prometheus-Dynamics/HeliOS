<script lang="ts">
  import { browser } from '$app/environment';
  import { onDestroy, untrack, type Snippet } from 'svelte';
  import { get, type Readable } from 'svelte/store';
  import { toaster } from '$lib';
  import { loadOwnedStreams } from '$lib/api/streamResources';
  import { buildErrorMessage, reportError } from '$lib/ui/errorPolicy';
  import { connectStreamControls, type StreamControlSocket } from '$lib/api/streamControls';
  import type { StreamsApi as SharedStreamsApi } from '$lib/api/streamsApi';
  import { backendFeatures } from '$lib/api/backendFeatures';
  import type { ControlMeta, StreamInfo, StreamPipelineLayout } from '$lib/ts-bindings/http/client';
  import type {
    PipelineDataType,
    PipelineGraphPlan,
    PipelineNodeValue,
    PipelineOverviewPipeline,
    PipelineRegistryEntry
  } from '$lib/types/pipeline';
  import type { PipelineStreamNodeMetrics } from '$lib/types/pipeline';
  import type { PipelineTuningConstantEntry, PipelineTuningConstantGroup } from '$lib/components/pipelines/types';
  import type { PipelineUi } from '$lib/features/pipelines/pipelineUiTypes';
  import { DEFAULT_PIPELINE_UI } from '$lib/features/pipelines/pipelineUiTypes';
  import {
    buildControlValue,
    clampControlValue,
    controlMax,
    controlMin,
    controlStep,
    displayControlValue,
    extractControlValue,
    menuOptions,
    menuValueDisplay,
    seedControlState
  } from './pipelineStreamControlUtils';
  import { buildTuneMultiplexSignature, multiplexKey, normalizeMultiplexSlots } from './pipelineMultiplexUtils';
  import { createTuneControlRuntime } from './pipelineTuneControlRuntime';
  import { createTuneMultiplexRuntime } from './pipelineTuneMultiplexRuntime';
  import { createTuneMultiplexApply } from './pipelineTuneMultiplexApply';
  import { createTuneStreamOverrideRuntime, createTuneStreamOverrides } from './pipelineTuneStreamOverrides';
  import {
    buildPipelineMetricsSummary,
    buildTuneMetricsSnapshot,
    createTuneMetricsRuntime,
    tuneRuntimeFromStats,
    type StreamMetricsSummary,
    type TuneMetricsStreamRef
  } from './pipelineTuneMetricsRuntime';
  import { openStreamMetricsSocket } from '$lib/api/streamMetrics';
  import { canUseWebSockets } from '$lib/api/core/ws';
  import { cancellableWithTimeout } from '$lib/api/requestUtils';
  import { createTuneMultiplexUi } from './pipelineTuneMultiplexUi';
  import { createTuneNodeHandlers } from './pipelineTuneNodeHandlers';
  import { createTuneControlLoader } from './pipelineTuneControlLoader';
  import { resetTuneState } from './pipelineTuneReset';
  import { createPipelineTuneUiActions } from './pipelineTuneUiActions';
  import { createTuneDraftState } from './pipelineTuneDraftState';
  import {
    buildTuneConstantGroups,
    buildTuneConstantSearchTokens,
    buildTuneMultiplexOutputOptionsCache,
    buildTuneMultiplexPalettePipelineIds,
    filterTuneConstantGroups
  } from './pipelineTuneDerived';
  import {
    runTunePipelineReset,
    runTuneStreamsLoad,
    runTuneScopeTabSync,
    runTuneStreamOverridesSeed,
    runTuneMultiplexHydration,
    runTuneMetricsRefresh,
    runTunePerformanceTabSync,
    runTuneControlsLoad,
    runTuneControlSocketSync
  } from './pipelineTuneEffects';
  import {
    mergeNodeOverrides,
    normalizePortKey,
    streamOverrideSignature
  } from './pipelineTuneState';
  import {
    asRecord,
    asStreamInfo,
    buildTuneMetricsStreamRefs,
    buildTuneMetricsWantedKey,
    buildTuneMetricsWantedRefs,
    buildTuneStreamsForPipeline,
    createTuneBindings,
    extractGraphAlias,
    findTuneSelectedStream,
    resolveTuneMetadataStream,
    resolveTunePreviewStream
  } from './pipelineTunePageSupport';
  import { isDaedalusPlan, safeClonePlan } from './pipelineTuneConstantUtils';
  import { fromApiGraphPlan } from '$lib/features/pipelines/graphConverters';
  import { buildDaedalusGraphPatch } from '$lib/features/pipelines/daedalusGraph';
  import { serializeGraphPlan } from '$lib/features/pipelines/graph';
  import { extractGraphOutputPorts } from '$lib/features/pipelines/graphOutputPorts';
  import {
    registryPortMetadataFor,
    registryPortTypeFor,
    resolveRegistrySnapshotNodeId
  } from '$lib/features/devices/camera/page/cameraPipelineTuningController';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';

  type PipelineUpdates = { sendPipeline: (payload: unknown) => boolean | void };
  type PipelinesApi = {
    listRegistry: (options?: { forceRefresh?: boolean; cacheMs?: number }) => Promise<unknown>;
    fetchGraph: (params: { id: string }) => Promise<unknown>;
    updateGraph: (params: { id: string; requestBody: { name?: string; graph: unknown } }) => Promise<unknown>;
  };

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

  type TuneDeps = {
    children?: Snippet<[ { tune: Record<string, unknown> } ]>;
    activeTab: Readable<'pipeline' | 'tune'>;
    pipelineUpdates: PipelineUpdates;
    pipelineUpdatesReady: Readable<boolean>;
    streamUpdatesReadyById: Readable<Record<string, boolean>>;
    getStreamUpdatesSocket: (streamId: string) => { ready?: () => boolean; send?: (payload: Record<string, unknown>) => boolean } | null;
    selectedPipeline: Readable<PipelineOverviewPipeline | null>;
    editingPlan: Readable<PipelineGraphPlan | null>;
    detailContext: Readable<unknown>;
    pipelines: Readable<PipelineOverviewPipeline[]>;
    pipelineLabelById: (pipelineId: string) => string;
    streamUsesPipeline: (stream: StreamInfo, pipelineId: string) => boolean;
    streamLabel: (stream: StreamInfo) => string;
    streamGraphForPipeline: (stream: StreamInfo, pipelineId: string) => unknown | null;
    outputOptionsForPipeline: (pipelineId: string | null) => string[];
    handlePlanChange: (plan: PipelineGraphPlan) => void;
    setNodeConstantValue: (nodeId: string, portKey: string, value: PipelineNodeValue) => void;
    saveCurrentPipeline: () => Promise<void>;
    openAssignModal: () => void;
    assignBusy: Readable<boolean>;
    refreshPipelineMetrics: () => void;
    resolveDataTypeKey: (dataType: PipelineDataType | undefined) => string | null;
    getDataTypeVariants: (dataType: PipelineDataType | undefined) => string[];
    resolveRegistryEntryForNode: (node: PipelineGraphPlan['nodes'][string] | undefined) => PipelineRegistryEntry | null;
    buildNodeValueFromInput: (
      raw: string,
      typeKey: string,
      variants?: string[]
    ) =>
      | { success: true; value: PipelineNodeValue; error?: string }
      | { success: false; error: string };
    extractTuneConstantEntries: (plan: PipelineGraphPlan) => PipelineTuningConstantEntry[];
    RAW_STREAM_PIPELINE_ID: string;
    RAW_STREAM_PIPELINE_UUID: string;
    PipelinesApi: PipelinesApi;
    StreamsApi: StreamsApi;
  };

  const {
    children,
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
  }: TuneDeps = $props();
  untrack(() => detailContext);

  const PIPELINE_UI_METADATA_KEY = 'helios.pipeline.ui';

  let tuneNodeDrafts = $state<Record<string, Record<string, string>>>({});
  let tuneNodeErrors = $state<Record<string, Record<string, string | null>>>({});
  let tuneStreams = $state<StreamInfo[]>([]);
  let tuneStreamsLoading = $state(false);
  let tuneStreamsLoaded = $state(false);
  let tuneStreamsError = $state<string | null>(null);
  let tuneStreamsRetryTimer: number | null = null;
  let tuneAssignBusySeen = $state(false);
  let tuneRegistrySnapshot = $state<unknown | null>(null);
  let tuneRegistrySnapshotLoading = $state(false);
  let tuneScopeTab = $state<'global' | string>('global');
  let tuneUiMode = $state<'pipeline' | 'advanced'>('pipeline');
  let tuneUiEditMode = $state(false);
  let tuneUiActiveTabId = $state<string>('');
  let tuneUiSelectedItemId = $state<string | null>(null);
  let tuneUiSelectedItemAnchor = $state<{ x: number; y: number } | null>(null);
  let tunePipelineUiDraft = $state<PipelineUi>(DEFAULT_PIPELINE_UI);
  let tunePipelineUiSearch = $state('');
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
  let tuneMetricsRefreshPipelineId = $state<string | null>(null);

  $effect(() => {
    if (!browser) return;
    if ($activeTab !== 'tune') return;
    const pipelineId = $selectedPipeline?.id ?? null;
    tunePipelineUiDraft = DEFAULT_PIPELINE_UI;
    if (!pipelineId) return;
    let cancelled = false;
    (async () => {
      try {
        const doc = (await PipelinesApi.fetchGraph({ id: pipelineId })) as
          | { graph?: { metadata?: Record<string, unknown> } }
          | null;
        if (cancelled) return;
        const metadata = (doc?.graph as { metadata?: Record<string, unknown> } | undefined)?.metadata;
        const ui = metadata?.[PIPELINE_UI_METADATA_KEY] as PipelineUi | undefined;
        tunePipelineUiDraft = ui ?? DEFAULT_PIPELINE_UI;
      } catch (error) {
        if (!cancelled) {
          console.warn('Failed to load pipeline UI', error);
          tunePipelineUiDraft = DEFAULT_PIPELINE_UI;
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  });

  $effect(() => {
    if (tuneUiMode !== 'pipeline') {
      tuneUiEditMode = false;
    }
  });

  $effect(() => {
    if (!tuneUiEditMode) {
      tuneUiSelectedItemId = null;
      tuneUiSelectedItemAnchor = null;
    }
  });

  const { saveTunePipelineUi, resetTunePipelineUi } = untrack(() =>
    createPipelineTuneUiActions({
      browser,
      PIPELINE_UI_METADATA_KEY,
      DEFAULT_PIPELINE_UI,
      getSelectedPipelineId: () => $selectedPipeline?.id ?? null,
      getPipelineUiDraft: () => tunePipelineUiDraft,
      setPipelineUiDraft: (next) => {
        tunePipelineUiDraft = next;
      },
      PipelinesApi,
      pipelineLabelById,
      toaster,
      reportError,
      buildErrorMessage
    })
  );

  const {
    readTuneNodeDraft,
    setTuneNodeDraft,
    clearTuneNodeDraft,
    setTuneNodeError,
    readTuneStreamNodeDraft,
    setTuneStreamNodeDraft,
    clearTuneStreamNodeDraft,
    setTuneStreamNodeError
  } = createTuneDraftState({
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
    }
  });

  const { fetchTuneStreams, seedStreamOverrides } = untrack(() =>
    createTuneStreamOverrides({
      StreamsApi,
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
    })
  );

  const tunePlan: PipelineGraphPlan | null = $derived.by(() => {
    const selected = $selectedPipeline?.graph ?? null;
    const draft = $editingPlan ?? null;
    if (!draft) return selected;
    const draftNodeCount = Object.keys((draft as PipelineGraphPlan).nodes ?? {}).length;
    const selectedNodeCount = Object.keys(selected?.nodes ?? {}).length;
    if (draftNodeCount === 0 && selectedNodeCount > 0) return selected;
    return draft;
  });

  type TuneConstantEntry = PipelineTuningConstantEntry;
  const tuneConstants: TuneConstantEntry[] = $derived.by(() =>
    tunePlan ? extractTuneConstantEntries(tunePlan) : []
  );
  type TuneConstantGroup = PipelineTuningConstantGroup;
  const tuneConstantGroups: TuneConstantGroup[] = $derived.by(() => buildTuneConstantGroups(tuneConstants));
  let tuneConstantSearch = $state('');
  const tuneBindings = $state(
    createTuneBindings({
      getTuneUiEditMode: () => tuneUiEditMode,
      setTuneUiEditMode: (value) => {
        tuneUiEditMode = value;
      },
      getTuneScopeTab: () => tuneScopeTab,
      setTuneScopeTab: (value) => {
        tuneScopeTab = value;
      },
      getTunePipelineUiSearch: () => tunePipelineUiSearch,
      setTunePipelineUiSearch: (value) => {
        tunePipelineUiSearch = value;
      },
      getTunePipelineUiDraft: () => tunePipelineUiDraft,
      setTunePipelineUiDraft: (value) => {
        tunePipelineUiDraft = value;
      },
      getTuneUiActiveTabId: () => tuneUiActiveTabId,
      setTuneUiActiveTabId: (value) => {
        tuneUiActiveTabId = value;
      },
      getTuneUiSelectedItemId: () => tuneUiSelectedItemId,
      setTuneUiSelectedItemId: (value) => {
        tuneUiSelectedItemId = value;
      },
      getTuneUiSelectedItemAnchor: () => tuneUiSelectedItemAnchor,
      setTuneUiSelectedItemAnchor: (value) => {
        tuneUiSelectedItemAnchor = value;
      },
      getTuneConstantSearch: () => tuneConstantSearch,
      setTuneConstantSearch: (value) => {
        tuneConstantSearch = value;
      },
      getTunePerformanceTab: () => tunePerformanceTab,
      setTunePerformanceTab: (value) => {
        tunePerformanceTab = value;
      },
      getTuneControlsQuery: () => tuneControlsQuery,
      setTuneControlsQuery: (value) => {
        tuneControlsQuery = value;
      },
      getTuneShowReadOnlyControls: () => tuneShowReadOnlyControls,
      setTuneShowReadOnlyControls: (value) => {
        tuneShowReadOnlyControls = value;
      },
      getTuneControlState: () => tuneControlState,
      setTuneControlState: (value) => {
        tuneControlState = value;
      },
      getTuneControlAppliedState: () => tuneControlAppliedState,
      setTuneControlAppliedState: (value) => {
        tuneControlAppliedState = value;
      },
      getTuneControlBusy: () => tuneControlBusy,
      setTuneControlBusy: (value) => {
        tuneControlBusy = value;
      },
      getTuneMultiplexRows: () => tuneMultiplexRows,
      setTuneMultiplexRows: (value) => {
        tuneMultiplexRows = value;
      },
      getTuneMultiplexColumns: () => tuneMultiplexColumns,
      setTuneMultiplexColumns: (value) => {
        tuneMultiplexColumns = value;
      },
      getTuneSelectedPipelineOutput: () => tuneSelectedPipelineOutput,
      setTuneSelectedPipelineOutput: (value) => {
        tuneSelectedPipelineOutput = value;
      },
      getTunePipelineRemoveModalOpen: () => tunePipelineRemoveModalOpen,
      setTunePipelineRemoveModalOpen: (value) => {
        tunePipelineRemoveModalOpen = value;
      },
      getTunePipelineRemoveCandidateId: () => tunePipelineRemoveCandidateId,
      setTunePipelineRemoveCandidateId: (value) => {
        tunePipelineRemoveCandidateId = value;
      },
      getTunePipelineAssignModalOpen: () => tunePipelineAssignModalOpen,
      setTunePipelineAssignModalOpen: (value) => {
        tunePipelineAssignModalOpen = value;
      },
      getTunePipelineAssignQuery: () => tunePipelineAssignQuery,
      setTunePipelineAssignQuery: (value) => {
        tunePipelineAssignQuery = value;
      },
      getTunePipelineAssignDraft: () => tunePipelineAssignDraft,
      setTunePipelineAssignDraft: (value) => {
        tunePipelineAssignDraft = value;
      }
    })
  );
  const tuneConstantSearchTokens: string[] = $derived.by(() => buildTuneConstantSearchTokens(tuneConstantSearch));
  const tuneFilteredConstantGroups: TuneConstantGroup[] = $derived.by(() =>
    filterTuneConstantGroups(tuneConstantGroups, tuneConstantSearchTokens)
  );
  const tuneStreamsForPipeline: StreamInfo[] = $derived.by(() =>
    buildTuneStreamsForPipeline({
      pipeline: $selectedPipeline ?? null,
      tuneStreams,
      streamUsesPipeline
    })
  );

  const tuneMetricsStreamRefs: TuneMetricsStreamRef[] = $derived.by(() =>
    buildTuneMetricsStreamRefs({
      pipeline: $selectedPipeline ?? null,
      tuneStreams,
      tuneStreamsForPipeline,
      streamLabel
    })
  );

  const tuneMetricsWantedRefs: TuneMetricsStreamRef[] = $derived.by(() =>
    buildTuneMetricsWantedRefs({
      tuneScopeTab,
      tuneMetricsStreamRefs
    })
  );

  const tuneMetricsWantedKey: string = $derived.by(() =>
    buildTuneMetricsWantedKey({
      pipelineId: $selectedPipeline?.id ?? '',
      tuneScopeTab,
      tuneMetricsStreamRefs
    })
  );
  const tuneActiveStreamId = $derived(tuneScopeTab === 'global' ? null : tuneScopeTab);
  const tuneSelectedStream = $derived(findTuneSelectedStream(tuneActiveStreamId, tuneStreamsForPipeline));
  const tunePreviewStream: StreamInfo | null = $derived.by(() =>
    resolveTunePreviewStream(tuneScopeTab, tuneSelectedStream)
  );
  const tuneMetadataStream: StreamInfo | null = $derived.by(() =>
    resolveTuneMetadataStream(tunePreviewStream, tuneStreamsForPipeline)
  );

  $effect(() => {
    if (!browser) return;
    if (!get(backendFeatures).pipelineRegistryPrefetch) return;
    if (tuneRegistrySnapshotLoading || tuneRegistrySnapshot) return;
    tuneRegistrySnapshotLoading = true;
    PipelinesApi.listRegistry({ cacheMs: 5_000 })
      .then((snapshot) => {
        tuneRegistrySnapshot = snapshot ?? null;
      })
      .catch((error) => {
        console.warn('Failed to load pipeline registry snapshot', error);
        tuneRegistrySnapshot = null;
      })
      .finally(() => {
        tuneRegistrySnapshotLoading = false;
      });
  });

  const tuneLivePlan: PipelineGraphPlan | null = $derived.by(() => {
    if (!tuneMetadataStream) return null;
    const pipelineId = $selectedPipeline?.id ?? '';
    if (!pipelineId) return null;
    const graph = streamGraphForPipeline(tuneMetadataStream, pipelineId);
    if (!graph) return null;
    try {
      return fromApiGraphPlan(graph);
    } catch (error) {
      console.warn('Failed to parse stream graph for dynamic metadata', error);
      return null;
    }
  });

  const mergeMetadata = (
    base: PipelineTuningConstantEntry['metadata'],
    live: PipelineTuningConstantEntry['metadata']
  ): PipelineTuningConstantEntry['metadata'] => {
    if (!base && !live) return null;
    if (!base) return live ?? null;
    if (!live) return base ?? null;
    const liveAllowed = Array.isArray(live.allowedValues) && live.allowedValues.length > 0 ? live.allowedValues : null;
    const baseAllowed = Array.isArray(base.allowedValues) && base.allowedValues.length > 0 ? base.allowedValues : null;
    return {
      ...base,
      ...live,
      ...(liveAllowed || baseAllowed ? { allowedValues: liveAllowed ?? baseAllowed ?? undefined } : {})
    };
  };

  const mergeDataType = (
    base: PipelineTuningConstantEntry['dataType'],
    live: PipelineTuningConstantEntry['dataType']
  ): PipelineTuningConstantEntry['dataType'] => {
    if (!base) return live ?? null;
    if (!live) return base ?? null;
    const baseVariants = getDataTypeVariants(base ?? undefined);
    const liveVariants = getDataTypeVariants(live ?? undefined);
    if (liveVariants.length > 0) {
      if (baseVariants.length === 0) return live;
      if (baseVariants.join('|') !== liveVariants.join('|')) return live;
    }
    const baseKey = (resolveDataTypeKey(base ?? undefined) ?? '').toLowerCase();
    const liveKey = (resolveDataTypeKey(live ?? undefined) ?? '').toLowerCase();
    const genericKeys = new SvelteSet(['generic', 'any', 'unknown', 'dynamic']);
    if (genericKeys.has(baseKey) && liveKey && !genericKeys.has(liveKey)) return live;
    return base;
  };

  type TuneDescriptor = {
    nodeId: string;
    nodeLabel: string;
    portKey: string;
    dataType: PipelineTuningConstantEntry['dataType'];
    defaultValue: PipelineNodeValue | null;
    overrideValue: PipelineNodeValue | null;
    metadata: PipelineTuningConstantEntry['metadata'];
  };

  const valueForPort = (
    values: Record<string, PipelineNodeValue> | null | undefined,
    portKey: string
  ): PipelineNodeValue | null => {
    if (!values) return null;
    const normalized = normalizePortKey(portKey);
    const exact = values[portKey];
    if (exact) return exact;
    if (normalized && values[normalized]) return values[normalized] ?? null;
    const fallbackKey = Object.keys(values).find((key) => normalizePortKey(key) === normalized);
    return fallbackKey ? values[fallbackKey] ?? null : null;
  };

  const buildInputDescriptorMap = (plan: PipelineGraphPlan | null): Map<string, TuneDescriptor> => {
    const map = new SvelteMap<string, TuneDescriptor>();
    if (!plan) return map;
    for (const [nodeId, node] of Object.entries(plan.nodes ?? {})) {
      const nodeLabel = node?.metadata?.name ?? nodeId;
      const inputs = node?.inputs ?? {};
      const registryEntry = resolveRegistryEntryForNode(node);
      const registryInputs = registryEntry?.inputs ?? {};
      const portMeta = node?.metadata?.inputPorts ?? {};
      const registryPortMeta = registryEntry?.metadata?.inputPorts ?? {};
      const portKeys = new SvelteSet<string>([
        ...Object.keys(inputs),
        ...Object.keys(registryInputs),
        ...Object.keys(portMeta),
        ...Object.keys(registryPortMeta)
      ]);
      for (const portKey of portKeys) {
        const dataType = (inputs as Record<string, PipelineDataType | undefined>)[portKey];
        const registryType = (registryInputs as Record<string, PipelineDataType | undefined>)[portKey];
        const normalizedPort = normalizePortKey(portKey);
        if (!normalizedPort) continue;
        const nodeMeta =
          portMeta[portKey] ??
          portMeta[portKey.toLowerCase()] ??
          (normalizedPort !== portKey ? portMeta[normalizedPort] ?? portMeta[normalizedPort.toLowerCase()] : null) ??
          null;
        const registryMeta =
          registryPortMeta[portKey] ??
          registryPortMeta[portKey.toLowerCase()] ??
          (normalizedPort !== portKey
            ? registryPortMeta[normalizedPort] ?? registryPortMeta[normalizedPort.toLowerCase()]
            : null) ??
          null;
        const lookupId = node.backendId ?? nodeId;
        const snapshotNodeId = tuneRegistrySnapshot
          ? resolveRegistrySnapshotNodeId(tuneRegistrySnapshot, lookupId)
          : null;
        const snapshotMeta = tuneRegistrySnapshot && snapshotNodeId
          ? registryPortMetadataFor(tuneRegistrySnapshot, snapshotNodeId, portKey)
          : null;
        const snapshotType = tuneRegistrySnapshot && snapshotNodeId
          ? registryPortTypeFor(tuneRegistrySnapshot, snapshotNodeId, portKey)
          : null;
        const baseValue = valueForPort(node?.info?.values ?? null, portKey);
        map.set(`${nodeId}:${normalizedPort}`, {
          nodeId,
          nodeLabel,
          portKey,
          dataType: mergeDataType(
            (dataType ?? null) as PipelineTuningConstantEntry['dataType'],
            mergeDataType(
              (registryType ?? null) as PipelineTuningConstantEntry['dataType'],
              (snapshotType ?? null) as PipelineTuningConstantEntry['dataType']
            )
          ),
          defaultValue: baseValue,
          overrideValue: null,
          metadata: mergeMetadata(
            mergeMetadata(
              (nodeMeta ?? null) as PipelineTuningConstantEntry['metadata'],
              (registryMeta ?? null) as PipelineTuningConstantEntry['metadata']
            ),
            (snapshotMeta ?? null) as PipelineTuningConstantEntry['metadata']
          )
        });
      }
    }
    return map;
  };

  const tuneLiveConstants: TuneConstantEntry[] = $derived.by(() =>
    tuneLivePlan ? extractTuneConstantEntries(tuneLivePlan) : []
  );
  const tuneLiveConstantMap: Map<string, TuneConstantEntry> = $derived.by(() => {
    const map = new SvelteMap<string, TuneConstantEntry>();
    for (const entry of tuneLiveConstants) {
      const nodeId = entry.nodeId ?? '';
      const portKey = normalizePortKey(entry.portKey);
      if (!nodeId || !portKey) continue;
      map.set(`${nodeId}:${portKey}`, entry);
    }
    return map;
  });
  const tunePlanInputMap = $derived.by(() => buildInputDescriptorMap(tunePlan));
  const tuneLiveInputMap = $derived.by(() => buildInputDescriptorMap(tuneLivePlan));
  const tuneConstantMap: Map<string, TuneDescriptor> = $derived.by(() => {
    const map = new SvelteMap<string, TuneDescriptor>();
    for (const entry of tuneConstants) {
      const nodeId = entry.nodeId ?? '';
      const portKey = normalizePortKey(entry.portKey);
      if (!nodeId || !portKey) continue;
      map.set(`${nodeId}:${portKey}`, {
        nodeId,
        nodeLabel: entry.nodeLabel ?? nodeId,
        portKey: entry.portKey,
        dataType: entry.dataType,
        defaultValue: entry.baseValue ?? null,
        overrideValue: entry.overrideValue ?? null,
        metadata: entry.metadata ?? null
      });
    }
    return map;
  });
  const tuneNodeDescriptors = $derived.by(() =>
    (() => {
      const merged = new SvelteMap<string, TuneDescriptor>();
      for (const [key, desc] of tunePlanInputMap) {
        merged.set(key, { ...desc });
      }
      for (const [key, desc] of tuneConstantMap) {
        const existing = merged.get(key);
        merged.set(key, {
          nodeId: desc.nodeId,
          nodeLabel: desc.nodeLabel ?? existing?.nodeLabel ?? desc.nodeId,
          portKey: desc.portKey ?? existing?.portKey ?? '',
          dataType: mergeDataType(existing?.dataType ?? null, desc.dataType ?? null),
          defaultValue: desc.defaultValue ?? existing?.defaultValue ?? null,
          overrideValue: desc.overrideValue ?? existing?.overrideValue ?? null,
          metadata: mergeMetadata(existing?.metadata ?? null, desc.metadata ?? null)
        });
      }
      const output: TuneDescriptor[] = [];
      for (const [key, desc] of merged.entries()) {
        const live =
          tuneLiveConstantMap.get(key) ??
          tuneLiveInputMap.get(key) ??
          null;
        const liveDefaultValue =
          live && typeof live === 'object' && 'baseValue' in live
            ? (live as PipelineTuningConstantEntry).baseValue ?? null
            : (live as TuneDescriptor | null)?.defaultValue ?? null;
        output.push({
          ...desc,
          dataType: mergeDataType(desc.dataType, live?.dataType ?? null),
          metadata: mergeMetadata(desc.metadata, live?.metadata ?? null),
          defaultValue: desc.defaultValue ?? liveDefaultValue,
          overrideValue: desc.overrideValue ?? live?.overrideValue ?? null
        });
      }
      return output;
    })()
  );

  const outputOptionsForTunePipeline = (pipelineId: string | null): string[] => {
    if (!pipelineId) return [];
    if (pipelineId === RAW_STREAM_PIPELINE_ID) return ['frame'];
    const filteredPlanOutputs = outputOptionsForPipeline(pipelineId);
    if (tunePreviewStream) {
      const graph = streamGraphForPipeline(tunePreviewStream, pipelineId);
      const ports = extractGraphOutputPorts(graph);
      if (ports.length) {
        if (filteredPlanOutputs.length) {
          return ports.filter((port) => filteredPlanOutputs.includes(port));
        }
        return [];
      }
    }
    return filteredPlanOutputs;
  };

  const {
    scheduleControlApply,
    closeTuneControlSocket,
    ensureTuneControlSocket,
    applyStreamControl,
    tuneFilteredControls
  } = untrack(() =>
    createTuneControlRuntime({
      getTunePreviewStream: () => tunePreviewStream,
      getTuneControlSocket: () => tuneControlSocket,
      setTuneControlSocket: (socket) => {
        tuneControlSocket = socket;
      },
      getTuneControlSocketStreamId: () => tuneControlSocketStreamId,
      setTuneControlSocketStreamId: (streamId) => {
        tuneControlSocketStreamId = streamId;
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
      connectStreamControls,
      StreamsApi,
      toaster,
      reportError,
      buildErrorMessage,
      clampControlValue,
      buildControlValue
    })
  );

  const tuneMultiplexRowIndices: number[] = $derived.by(() =>
    Array.from({ length: Math.min(Math.max(Math.trunc(tuneMultiplexRows), 1), 6) }, (_, i) => i)
  );
  const tuneMultiplexColumnIndices: number[] = $derived.by(() =>
    Array.from({ length: Math.min(Math.max(Math.trunc(tuneMultiplexColumns), 1), 6) }, (_, i) => i)
  );

  const tuneMultiplexPalettePipelineIds: string[] = $derived.by(() =>
    buildTuneMultiplexPalettePipelineIds({
      tunePreviewStream,
      pipelineLabelById,
      RAW_STREAM_PIPELINE_ID,
      RAW_STREAM_PIPELINE_UUID
    })
  );

  function handleTuneAssign() {
    if (!$selectedPipeline) {
      toaster.info({ title: 'Select a pipeline', description: 'Select a pipeline before assigning.' });
      return;
    }
    openAssignModal();
  }

  const tuneMultiplexOutputOptionsCache: Record<string, string[]> = $derived.by(() => {
    void tunePreviewStream;
    return buildTuneMultiplexOutputOptionsCache({
      tuneMultiplexPalettePipelineIds,
      tuneMultiplexSlots,
      outputOptionsForPipeline: outputOptionsForTunePipeline
    });
  });
  const tuneMultiplexLayoutSignature = $derived.by(() =>
    buildTuneMultiplexSignature(tuneMultiplexRows, tuneMultiplexColumns, tuneMultiplexSlots, tuneMultiplexSlotOutputs)
  );

  const tuneMultiplexGridIsSingle = $derived.by(() => Math.trunc(tuneMultiplexRows) === 1 && Math.trunc(tuneMultiplexColumns) === 1);

  let markTuneMultiplexDirty = () => {};

  const {
    tunePipelineForCell,
    tuneOutputKeyForCell,
    setTuneOutputKeyForCell,
    tuneOutputSelectionForPipeline,
    setTuneOutputSelectionForPipeline,
    setTuneMultiplexGridDimensions,
    clearTuneMultiplexCell,
    tuneAllowDrop,
    startTuneMultiplexDrag,
    dropTuneMultiplexOn
  } = createTuneMultiplexUi({
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
    getTuneMultiplexGridIsSingle: () => tuneMultiplexGridIsSingle,
    setTuneMultiplexDragPipelineId: (next) => {
      tuneMultiplexDragPipelineId = next;
    },
    setTuneMultiplexDragSource: (next) => {
      tuneMultiplexDragSource = next;
    },
    getTuneMultiplexDragPipelineId: () => tuneMultiplexDragPipelineId,
    getTuneMultiplexDragSource: () => tuneMultiplexDragSource,
    outputOptionsForPipeline: outputOptionsForTunePipeline,
    markTuneMultiplexDirty: () => markTuneMultiplexDirty()
  });

  let applyTuneMultiplex: (options?: { quiet?: boolean }) => Promise<void> = async () => {};

  const {
    buildTuneMultiplexStateSignature,
    scheduleTuneMultiplexAutoApply,
    markTuneMultiplexDirty: markTuneMultiplexDirtyRuntime
  } = createTuneMultiplexRuntime({
    browser,
    getTunePreviewStream: () => tunePreviewStream,
    getTuneMultiplexAutoApplyTimer: () => tuneMultiplexAutoApplyTimer,
    setTuneMultiplexAutoApplyTimer: (timer) => {
      tuneMultiplexAutoApplyTimer = timer;
    },
    getTuneMultiplexBusy: () => tuneMultiplexBusy,
    setTuneMultiplexDirty: (dirty) => {
      tuneMultiplexDirty = dirty;
    },
    getTuneMultiplexRows: () => tuneMultiplexRows,
    getTuneMultiplexColumns: () => tuneMultiplexColumns,
    getTuneMultiplexSlots: () => tuneMultiplexSlots,
    getTuneMultiplexSlotOutputs: () => tuneMultiplexSlotOutputs,
    getTuneMultiplexLastAppliedSignature: () => tuneMultiplexLastAppliedSignature,
    buildTuneMultiplexSignature,
    applyTuneMultiplex: (options) => applyTuneMultiplex(options)
  });
  markTuneMultiplexDirty = markTuneMultiplexDirtyRuntime;

  const {
    buildTuneMultiplexLayout,
    resolveTunePipelineGraph,
    resolveTunePipelineOutput,
    boundTunePipelines,
    setTuneLivePipelineOutput,
    applyTuneMultiplex: applyTuneMultiplexImpl
  } = untrack(() =>
    createTuneMultiplexApply({
      browser,
      RAW_STREAM_PIPELINE_ID,
      RAW_STREAM_PIPELINE_UUID,
      getTunePreviewStream: () => tunePreviewStream,
      getTuneMultiplexRows: () => tuneMultiplexRows,
      getTuneMultiplexColumns: () => tuneMultiplexColumns,
      getTuneMultiplexSlots: () => tuneMultiplexSlots,
      setTuneMultiplexSlots: (next) => {
        tuneMultiplexSlots = next;
      },
      getTuneMultiplexSlotOutputs: () => tuneMultiplexSlotOutputs,
      setTuneMultiplexSlotOutputs: (next) => {
        tuneMultiplexSlotOutputs = next;
      },
      getTuneMultiplexGridIsSingle: () => tuneMultiplexGridIsSingle,
      getTuneMultiplexAutoApplyTimer: () => tuneMultiplexAutoApplyTimer,
      setTuneMultiplexAutoApplyTimer: (timer) => {
        tuneMultiplexAutoApplyTimer = timer;
      },
      getTuneMultiplexBusy: () => tuneMultiplexBusy,
      setTuneMultiplexBusy: (busy) => {
        tuneMultiplexBusy = busy;
      },
      setTuneMultiplexError: (message) => {
        tuneMultiplexError = message;
      },
      setTuneMultiplexDirty: (dirty) => {
        tuneMultiplexDirty = dirty;
      },
      setTuneMultiplexLastAppliedSignature: (signature) => {
        tuneMultiplexLastAppliedSignature = signature;
      },
      setTuneMultiplexHydratedSignature: (signature) => {
        tuneMultiplexHydratedSignature = signature;
      },
      setTuneMultiplexHydratedStreamId: (streamId) => {
        tuneMultiplexHydratedStreamId = streamId;
      },
      outputOptionsForPipeline: outputOptionsForTunePipeline,
      resolvePipelineLabel: pipelineLabelById,
      pipelines: { get: () => get(pipelines) },
      serializeGraphPlan,
      StreamsApi,
      toaster,
      reportError,
      buildErrorMessage,
      fetchTuneStreams,
      buildTuneMultiplexStateSignature
    })
  );

  applyTuneMultiplex = applyTuneMultiplexImpl;

  $effect(() => {
    const key = multiplexKey(0, 0);
    const next = tuneMultiplexSlotOutputs[key] ?? null;
    if (tuneSelectedPipelineOutput !== next) {
      tuneSelectedPipelineOutput = next;
    }
  });

  const TUNE_METRICS_POLL_MS = 3_000;
  const TUNE_METRICS_WS_INTERVAL_MS = 800;
  const TUNE_METRICS_SNAPSHOT_TIMEOUT_MS = 2500;
  let tuneMetricsSnapshots = $state<PipelineStreamNodeMetrics[]>([]);
  let tuneMetricsStatus = $state<'idle' | 'connecting' | 'connected' | 'error'>('idle');
  let tuneMetricsError = $state<string | null>(null);
  let tuneMetricsUpdatedAt = $state<number | null>(null);
  let tuneMetricsPollTimer: number | null = null;
  let tuneMetricsRequestId = $state(0);
  let tuneMetricsInFlight = false;

  type MetricsSource = {
    metrics: PipelineStreamNodeMetrics[];
    status: 'idle' | 'connecting' | 'connected' | 'error';
    error: string | null;
    updatedAt: number | null;
  };
  const metricsSource: MetricsSource = $derived.by(() => {
    return {
      metrics: tuneMetricsSnapshots,
      status: tuneMetricsStatus,
      error: tuneMetricsError,
      updatedAt: tuneMetricsUpdatedAt
    };
  });

  const pipelineMetricsSummary: StreamMetricsSummary[] = $derived.by(() =>
    buildPipelineMetricsSummary(metricsSource.metrics ?? [])
  );

  const metricsUpdatedLabel: string = $derived.by(() => {
    const ts = metricsSource.updatedAt ?? null;
    if (!ts || !Number.isFinite(ts)) return 'Not updated yet';
    const date = new Date(ts);
    return Number.isFinite(date.valueOf()) ? date.toLocaleTimeString() : 'Unknown';
  });

  const metricsStatusLabel: string = $derived.by(() => {
    const status = metricsSource.status ?? 'idle';
    if (status === 'connected') return 'Live';
    if (status === 'connecting') return 'Connecting…';
    if (status === 'error') return 'Offline';
    return 'Idle';
  });

  const { fetchTuneMetricsSnapshots } = untrack(() =>
    createTuneMetricsRuntime({
      StreamsApi,
      buildErrorMessage,
      getSelectedPipelineId: () => $selectedPipeline?.id ?? null,
      getTuneMetricsSnapshots: () => tuneMetricsSnapshots,
      setTuneMetricsSnapshots: (next) => {
        tuneMetricsSnapshots = next;
      },
      getTuneMetricsStatus: () => tuneMetricsStatus,
      setTuneMetricsStatus: (next) => {
        tuneMetricsStatus = next;
      },
      getTuneMetricsError: () => tuneMetricsError,
      setTuneMetricsError: (next) => {
        tuneMetricsError = next;
      },
      getTuneMetricsUpdatedAt: () => tuneMetricsUpdatedAt,
      setTuneMetricsUpdatedAt: (next) => {
        tuneMetricsUpdatedAt = next;
      },
      getTuneMetricsRequestId: () => tuneMetricsRequestId,
      setTuneMetricsRequestId: (next) => {
        tuneMetricsRequestId = next;
      },
      getTuneMetricsInFlight: () => tuneMetricsInFlight,
      setTuneMetricsInFlight: (next) => {
        tuneMetricsInFlight = next;
      }
    })
  );

  $effect(() => {
    runTunePipelineReset({
      currentPipelineId: $selectedPipeline?.id ?? null,
      lastPipelineId: lastTunePipelineId,
      setLastPipelineId: (next) => {
        lastTunePipelineId = next;
      },
      reset: () =>
        resetTuneState({
          setTuneNodeDrafts: (next) => {
            tuneNodeDrafts = next;
          },
          setTuneNodeErrors: (next) => {
            tuneNodeErrors = next;
          },
          setTuneStreamInputOverridesById: (next) => {
            tuneStreamInputOverridesById = next;
          },
          setTuneStreamNodeOverridesById: (next) => {
            tuneStreamNodeOverridesById = next;
          },
          setTuneStreamNodeDraftsById: (next) => {
            tuneStreamNodeDraftsById = next;
          },
          setTuneStreamNodeErrorsById: (next) => {
            tuneStreamNodeErrorsById = next;
          },
          setTuneStreamOverridesLoaded: (next) => {
            tuneStreamOverridesLoaded = next;
          },
          setTuneScopeTab: (next) => {
            tuneScopeTab = next;
          },
          setTuneStreamApplyBusyById: (next) => {
            tuneStreamApplyBusyById = next;
          },
          setTuneStreamApplyErrorById: (next) => {
            tuneStreamApplyErrorById = next;
          },
          setTuneStreamApplyQueuedById: (next) => {
            tuneStreamApplyQueuedById = next;
          },
          setTuneStreamLastAppliedSignatureById: (next) => {
            tuneStreamLastAppliedSignatureById = next;
          },
          setTuneStreamLastAppliedNodeOverridesById: (next) => {
            tuneStreamLastAppliedNodeOverridesById = next;
          },
          getTuneStreamAutoApplyTimerById: () => tuneStreamAutoApplyTimerById,
          setTuneStreamAutoApplyTimerById: (next) => {
            tuneStreamAutoApplyTimerById = next;
          },
          getTuneStreamApplyRafById: () => tuneStreamApplyRafById,
          setTuneStreamApplyRafById: (next) => {
            tuneStreamApplyRafById = next;
          },
          getTuneGlobalAutoSaveTimer: () => tuneGlobalAutoSaveTimer,
          setTuneGlobalAutoSaveTimer: (next) => {
            tuneGlobalAutoSaveTimer = next;
          },
          setTuneMultiplexError: (next) => {
            tuneMultiplexError = next;
          },
          setTuneStreamControls: (next: ControlMeta[]) => {
            tuneStreamControls = next;
          },
          setTuneMetricsSnapshots: (next: PipelineStreamNodeMetrics[]) => {
            tuneMetricsSnapshots = next;
          },
          setTuneMetricsStatus: (next) => {
            tuneMetricsStatus = next;
          },
          setTuneMetricsError: (next) => {
            tuneMetricsError = next;
          },
          setTuneMetricsUpdatedAt: (next) => {
            tuneMetricsUpdatedAt = next;
          },
          getTuneMetricsPollTimer: () => tuneMetricsPollTimer,
          setTuneMetricsPollTimer: (next) => {
            tuneMetricsPollTimer = next;
          },
          setTuneControlState: (next) => {
            tuneControlState = next;
          },
          setTuneControlAppliedState: (next) => {
            tuneControlAppliedState = next;
          },
          setTuneControlBusy: (next) => {
            tuneControlBusy = next;
          },
          setTuneControlsQuery: (next) => {
            tuneControlsQuery = next;
          },
          setTuneShowReadOnlyControls: (next) => {
            tuneShowReadOnlyControls = next;
          },
          setTuneControlsLoading: (next) => {
            tuneControlsLoading = next;
          },
          setTuneControlsError: (next) => {
            tuneControlsError = next;
          },
          setTuneControlsLoadedStreamId: (next) => {
            tuneControlsLoadedStreamId = next;
          }
        })
    });
  });

  $effect(() => {
    runTuneStreamsLoad({
      activeTab: $activeTab,
      tuneStreamsLoaded,
      tuneStreamsLoading,
      setTuneStreamsLoading: (next) => {
        tuneStreamsLoading = next;
      },
      setTuneStreamsLoaded: (next) => {
        tuneStreamsLoaded = next;
      },
      setTuneStreamsError: (next) => {
        tuneStreamsError = next;
      },
      setTuneStreams: (next) => {
        tuneStreams = next;
      },
      fetchTuneStreams,
      buildErrorMessage
    });
  });

  $effect(() => {
    const busy = $assignBusy;
    if (busy) {
      tuneAssignBusySeen = true;
      return;
    }
    if (!tuneAssignBusySeen) return;
    tuneAssignBusySeen = false;

    if ($activeTab !== 'tune') {
      tuneStreamsLoaded = false;
      return;
    }

    let cancelled = false;
    tuneStreamsLoading = true;
    tuneStreamsError = null;
    fetchTuneStreams()
      .then((streams) => {
        if (cancelled) return;
        tuneStreams = Array.isArray(streams) ? streams : [];
        tuneStreamsLoaded = true;
      })
      .catch((error) => {
        if (cancelled) return;
        console.error('Failed to refresh streams after pipeline assignment', error);
        tuneStreamsError = buildErrorMessage({ error, fallback: 'Unable to refresh attached streams.' });
      })
      .finally(() => {
        if (cancelled) return;
        tuneStreamsLoading = false;
      });
    return () => {
      cancelled = true;
    };
  });

  $effect(() => {
    if (!browser) return;
    if ($activeTab !== 'tune') return;
    if (!tuneStreamsError) return;
    if (tuneStreamsLoading) return;
    if (tuneStreams.length > 0) return;
    if (tuneStreamsRetryTimer) return;

    // `runTuneStreamsLoad` marks streams as "loaded" even on errors; schedule a single backoff retry.
    tuneStreamsRetryTimer = window.setTimeout(() => {
      tuneStreamsRetryTimer = null;
      if ($activeTab !== 'tune') return;
      tuneStreamsLoaded = false;
    }, 2000);

    return () => {
      if (tuneStreamsRetryTimer) {
        clearTimeout(tuneStreamsRetryTimer);
        tuneStreamsRetryTimer = null;
      }
    };
  });

  $effect(() => {
    if (!browser) return;

    if ($activeTab !== 'tune' || tunePerformanceTab !== 'metrics') {
      tuneMetricsSnapshots = [];
      tuneMetricsStatus = 'idle';
      tuneMetricsError = null;
      tuneMetricsUpdatedAt = null;
      if (tuneMetricsPollTimer) {
        clearInterval(tuneMetricsPollTimer);
        tuneMetricsPollTimer = null;
      }
      return;
    }

    let isMounted = true;
    let reconnectTimer: number | null = null;
    let connectTimeoutTimer: number | null = null;
    let reconnectAttempts = 0;
    let sawAnyMetrics = false;
    let pollInFlight = false;
    const cleanups: Record<string, () => void> = {};

    const stopTimers = () => {
      if (reconnectTimer) {
        clearTimeout(reconnectTimer);
        reconnectTimer = null;
      }
      if (connectTimeoutTimer) {
        clearTimeout(connectTimeoutTimer);
        connectTimeoutTimer = null;
      }
      if (tuneMetricsPollTimer) {
        clearInterval(tuneMetricsPollTimer);
        tuneMetricsPollTimer = null;
      }
      reconnectAttempts = 0;
    };

    const stopSockets = () => {
      Object.values(cleanups).forEach((fn) => fn());
      for (const key of Object.keys(cleanups)) delete cleanups[key];
    };

    const upsertSnapshot = (snapshot: PipelineStreamNodeMetrics) => {
      const rest = tuneMetricsSnapshots.filter((entry) => entry.streamId !== snapshot.streamId);
      tuneMetricsSnapshots = [snapshot, ...rest];
    };

    const fetchSnapshotFor = async (ref: TuneMetricsStreamRef) => {
      try {
        const metrics = await cancellableWithTimeout(
          () => StreamsApi.getMetrics({ id: ref.id }),
          TUNE_METRICS_SNAPSHOT_TIMEOUT_MS
        );
        if (!isMounted) return;
        upsertSnapshot(
          buildTuneMetricsSnapshot(ref, (metrics ?? {}) as Record<string, unknown>, {
            pipelineId: $selectedPipeline?.id ?? null
          })
        );
        // Snapshot fetch is enough to render useful data even if WS is blocked.
        sawAnyMetrics = true;
        if (tuneMetricsStatus === 'connecting') tuneMetricsStatus = 'connected';
        tuneMetricsUpdatedAt = Date.now();
      } catch (error) {
        if (!isMounted) return;
        const message = buildErrorMessage({ error, fallback: `Unable to load metrics for ${ref.label}.` });
        tuneMetricsError = tuneMetricsError ?? message;
      }
    };

    const scheduleReconnect = (wanted: TuneMetricsStreamRef[], connect: () => void) => {
      if (reconnectTimer) return;
      const delay = Math.min(15_000, 1_500 * 2 ** Math.min(reconnectAttempts, 4));
      reconnectAttempts += 1;
      reconnectTimer = window.setTimeout(() => {
        reconnectTimer = null;
        if (!isMounted) return;
        stopSockets();
        // Reseed placeholders so Global stays populated after reconnect.
        tuneMetricsSnapshots = wanted.map((ref) => buildTuneMetricsSnapshot(ref, null, { pipelineId: $selectedPipeline?.id ?? null }));
        tuneMetricsStatus = 'connecting';
        tuneMetricsError = tuneMetricsError ?? null;
        tuneMetricsUpdatedAt = null;
        connect();
      }, delay);
    };

    const openSocketFor = (ref: TuneMetricsStreamRef, wanted: TuneMetricsStreamRef[], connect: () => void) =>
      openStreamMetricsSocket(
        ref.id,
        {
          onMetrics: (event) => {
            if (!isMounted || event.stream_id !== ref.id) return;
            sawAnyMetrics = true;
            reconnectAttempts = 0;
            upsertSnapshot(
              buildTuneMetricsSnapshot(ref, (event.metrics ?? {}) as Record<string, unknown>, {
                pipelineId: $selectedPipeline?.id ?? null
              })
            );
            tuneMetricsStatus = 'connected';
            tuneMetricsError = null;
            tuneMetricsUpdatedAt = typeof event.timestamp_ms === 'number' ? event.timestamp_ms : Date.now();
            if (tuneMetricsPollTimer) {
              clearInterval(tuneMetricsPollTimer);
              tuneMetricsPollTimer = null;
            }
          },
          onError: (err) => {
            if (!isMounted) return;
            tuneMetricsError = err?.error ?? 'Metrics stream error';
            if (!sawAnyMetrics) tuneMetricsStatus = 'error';
            tuneMetricsUpdatedAt = tuneMetricsUpdatedAt ?? Date.now();
            scheduleReconnect(wanted, connect);
          },
          onClose: (info) => {
            if (!isMounted || info?.expected) return;
            if (!sawAnyMetrics) {
              tuneMetricsStatus = 'error';
              tuneMetricsError = tuneMetricsError ?? 'Metrics stream closed';
              tuneMetricsUpdatedAt = tuneMetricsUpdatedAt ?? Date.now();
            }
            scheduleReconnect(wanted, connect);
          }
        },
        { intervalMs: TUNE_METRICS_WS_INTERVAL_MS }
      );

    const startMetrics = (wanted: TuneMetricsStreamRef[]) => {
      // Seed placeholders so Global lists streams immediately.
      tuneMetricsSnapshots = wanted.map((ref) => buildTuneMetricsSnapshot(ref, null, { pipelineId: $selectedPipeline?.id ?? null }));
      tuneMetricsStatus = 'connecting';
      tuneMetricsError = null;
      tuneMetricsUpdatedAt = null;
      sawAnyMetrics = false;
      reconnectAttempts = 0;

      const connect = () => {
        for (const ref of wanted) void fetchSnapshotFor(ref);

        if (!canUseWebSockets()) {
          void fetchTuneMetricsSnapshots(wanted);
          if (tuneMetricsPollTimer) clearInterval(tuneMetricsPollTimer);
          tuneMetricsPollTimer = window.setInterval(() => void fetchTuneMetricsSnapshots(wanted), TUNE_METRICS_POLL_MS);
          return;
        }

        for (const ref of wanted) {
          cleanups[ref.id] = openSocketFor(ref, wanted, connect);
        }

        // Keep an HTTP poll running until WS actually streams frames. This avoids "connected but stale"
        // when a WS proxy or firewall breaks upgrades silently.
        if (!tuneMetricsPollTimer) {
          tuneMetricsPollTimer = window.setInterval(() => {
            if (!isMounted) return;
            if (pollInFlight) return;
            pollInFlight = true;
            Promise.allSettled(wanted.map((ref) => fetchSnapshotFor(ref))).finally(() => {
              pollInFlight = false;
            });
          }, TUNE_METRICS_POLL_MS);
        }

        // Avoid hanging on "Connecting..." forever if the socket opens but no payloads arrive.
        if (connectTimeoutTimer) clearTimeout(connectTimeoutTimer);
        connectTimeoutTimer = window.setTimeout(() => {
          connectTimeoutTimer = null;
          if (!isMounted || sawAnyMetrics || tuneMetricsStatus !== 'connecting') return;
          tuneMetricsStatus = 'error';
          tuneMetricsError = tuneMetricsError ?? 'Metrics stream is not responding.';
          tuneMetricsUpdatedAt = tuneMetricsUpdatedAt ?? Date.now();
          scheduleReconnect(wanted, connect);
        }, Math.max(1500, TUNE_METRICS_SNAPSHOT_TIMEOUT_MS));
      };

      connect();
    };

    // Only track changes in the *key*, not the array identity, to avoid reconnect loops when
    // upstream stores refresh selectedPipeline/streams with the same ids.
    const wantedKey = tuneMetricsWantedKey;
    const wanted = untrack(() => tuneMetricsWantedRefs);
    void wantedKey;

    if (wanted.length > 0) {
      startMetrics(wanted);
      return () => {
        isMounted = false;
        stopTimers();
        stopSockets();
      };
    }

    // If we can't resolve attached streams from the existing Tune state (e.g. stream list not loaded yet),
    // try a one-off discovery pass before giving up.
    const pipeline = $selectedPipeline;
    const pipelineId = pipeline?.id ?? '';
    if (tuneScopeTab === 'global' && pipelineId) {
      tuneMetricsSnapshots = [];
      tuneMetricsStatus = 'connecting';
      tuneMetricsError = null;
      tuneMetricsUpdatedAt = null;

      void (async () => {
        try {
          const listResult = await cancellableWithTimeout(
            () => loadOwnedStreams({ preferCached: false }),
            TUNE_METRICS_SNAPSHOT_TIMEOUT_MS
          );
          if (!isMounted) return;
          const streams = listResult;

          const aliases = new SvelteSet<string>();
          const name = typeof pipeline?.name === 'string' ? pipeline.name.trim().toLowerCase() : '';
          const alias = typeof pipeline?.alias === 'string' ? pipeline.alias.trim().toLowerCase() : '';
          if (name) aliases.add(name);
          if (alias) aliases.add(alias);

          const aliasMatches = (graph: unknown): boolean => {
            if (!aliases.size) return false;
            const value = extractGraphAlias(graph);
            if (!value) return false;
            return aliases.has(value.toLowerCase());
          };

          const matching = streams.filter((stream) => {
            const streamInfo = asStreamInfo(stream);
            if (!streamInfo) return false;
            const id = streamInfo.id;
            if (!id.trim()) return false;
            const manifest = asRecord(streamInfo.manifest);
            if (manifest?.internal === true) return false;
            if (streamUsesPipeline(streamInfo, pipelineId)) return true;
            if (aliasMatches(manifest?.pipeline_graph)) return true;
            if (Array.isArray(manifest?.pipelines)) {
              return manifest.pipelines.some((binding) => aliasMatches(asRecord(binding)?.pipeline_graph));
            }
            return false;
          });

          const refs: TuneMetricsStreamRef[] = [];
          const seen = new SvelteSet<string>();
          for (const stream of matching) {
            const streamInfo = asStreamInfo(stream);
            if (!streamInfo) continue;
            const id = streamInfo.id.trim();
            if (!id || seen.has(id)) continue;
            const label = streamLabel(streamInfo) || id;
            refs.push({ id, label });
            seen.add(id);
          }

          if (refs.length === 0) {
            tuneMetricsSnapshots = [];
            tuneMetricsStatus = 'idle';
            tuneMetricsError = null;
            tuneMetricsUpdatedAt = null;
            return;
          }

          startMetrics(refs);
        } catch (error) {
          if (!isMounted) return;
          tuneMetricsSnapshots = [];
          tuneMetricsStatus = 'error';
          tuneMetricsError = buildErrorMessage({ error, fallback: 'Unable to discover attached streams for metrics.' });
          tuneMetricsUpdatedAt = Date.now();
        }
      })();

      return () => {
        isMounted = false;
        stopTimers();
        stopSockets();
      };
    }

    // Nothing to connect to.
    tuneMetricsSnapshots = [];
    tuneMetricsStatus = 'idle';
    tuneMetricsError = null;
    tuneMetricsUpdatedAt = null;
    stopTimers();
    stopSockets();
    return () => {
      isMounted = false;
      stopTimers();
      stopSockets();
    };
  });

  $effect(() => {
    runTuneScopeTabSync({
      activeTab: $activeTab,
      tuneScopeTab,
      tuneStreamsForPipeline,
      setTuneScopeTab: (next) => {
        tuneScopeTab = next;
      }
    });
  });

  $effect(() => {
    runTuneStreamOverridesSeed({
      activeTab: $activeTab,
      tuneSelectedStream,
      tunePlan,
      selectedPipelineId: $selectedPipeline?.id ?? null,
      tuneStreamOverridesLoaded,
      seedStreamOverrides
    });
  });

  $effect(() => {
    runTuneMultiplexHydration({
      activeTab: $activeTab,
      tunePreviewStream,
      tuneMultiplexDirty,
      tuneMultiplexHydratedStreamId,
      tuneMultiplexHydratedSignature,
      tuneMultiplexLastAppliedSignature,
      RAW_STREAM_PIPELINE_UUID,
      RAW_STREAM_PIPELINE_ID,
      setTuneMultiplexError: (next) => {
        tuneMultiplexError = next;
      },
      setTuneMultiplexBusy: (next) => {
        tuneMultiplexBusy = next;
      },
      tuneMultiplexAutoApplyTimer,
      clearMultiplexAutoApplyTimer: () => {
        if (tuneMultiplexAutoApplyTimer) {
          clearTimeout(tuneMultiplexAutoApplyTimer);
          tuneMultiplexAutoApplyTimer = null;
        }
      },
      setTuneMultiplexDirty: (next) => {
        tuneMultiplexDirty = next;
      },
      setTuneMultiplexHydratedStreamId: (next) => {
        tuneMultiplexHydratedStreamId = next;
      },
      setTuneMultiplexHydratedSignature: (next) => {
        tuneMultiplexHydratedSignature = next;
      },
      setTuneMultiplexLastAppliedSignature: (next) => {
        tuneMultiplexLastAppliedSignature = next;
      },
      setTunePerformanceTab: (next) => {
        tunePerformanceTab = next;
      },
      setTuneMultiplexSlots: (next) => {
        tuneMultiplexSlots = next;
      },
      setTuneMultiplexSlotOutputs: (next) => {
        tuneMultiplexSlotOutputs = next;
      },
      setTuneStreamControls: (next: ControlMeta[]) => {
        tuneStreamControls = next;
      },
      setTuneControlState: (next) => {
        tuneControlState = next;
      },
      setTuneControlAppliedState: (next) => {
        tuneControlAppliedState = next;
      },
      setTuneControlBusy: (next) => {
        tuneControlBusy = next;
      },
      setTuneControlsLoading: (next) => {
        tuneControlsLoading = next;
      },
      setTuneControlsError: (next) => {
        tuneControlsError = next;
      },
      setTuneControlsLoadedStreamId: (next) => {
        tuneControlsLoadedStreamId = next;
      },
      setTuneMultiplexRows: (next) => {
        tuneMultiplexRows = next;
      },
      setTuneMultiplexColumns: (next) => {
        tuneMultiplexColumns = next;
      }
    });
  });

  $effect(() => {
    runTuneMetricsRefresh({
      activeTab: $activeTab,
      selectedPipelineId: $selectedPipeline?.id ?? null,
      tuneMetricsRefreshPipelineId,
      setTuneMetricsRefreshPipelineId: (next) => {
        tuneMetricsRefreshPipelineId = next;
      },
      refreshPipelineMetrics
    });
  });

  $effect(() => {
    runTunePerformanceTabSync({
      activeTab: $activeTab,
      tuneScopeTab,
      tunePerformanceTab,
      setTunePerformanceTab: (next) => {
        tunePerformanceTab = next;
      }
    });
  });

  const { loadTuneControls } = untrack(() =>
    createTuneControlLoader({
      StreamsApi,
      buildErrorMessage,
      seedControlState,
      setTuneStreamControls: (next) => {
        tuneStreamControls = next;
      },
      setTuneControlState: (next) => {
        tuneControlState = next;
      },
      setTuneControlAppliedState: (next) => {
        tuneControlAppliedState = next;
      },
      setTuneControlsLoadedStreamId: (streamId) => {
        tuneControlsLoadedStreamId = streamId;
      },
      setTuneControlsLoading: (loading) => {
        tuneControlsLoading = loading;
      },
      setTuneControlsError: (message) => {
        tuneControlsError = message;
      },
      getTuneControlsRequestId: () => tuneControlsRequestId,
      setTuneControlsRequestId: (next) => {
        tuneControlsRequestId = next;
      }
    })
  );

  $effect(() => {
    runTuneControlsLoad({
      activeTab: $activeTab,
      tunePerformanceTab,
      tunePreviewStream,
      tuneControlsLoadedStreamId,
      tuneControlsLoading,
      loadTuneControls
    });
  });

  function scheduleTuneGlobalAutoSave(): void {
    if (!browser) return;
    if (!$selectedPipeline) return;
    if ($pipelineUpdatesReady) return;
    if (tuneGlobalAutoSaveTimer) {
      clearTimeout(tuneGlobalAutoSaveTimer);
    }
    tuneGlobalAutoSaveTimer = window.setTimeout(() => {
      tuneGlobalAutoSaveTimer = null;
      void saveCurrentPipeline();
    }, 500);
  }

  const { scheduleTuneStreamAutoApply, applyTuneStreamOverridesFor } = untrack(() =>
    createTuneStreamOverrideRuntime({
      browser,
      getStreamUpdatesReadyById: (streamId) => Boolean($streamUpdatesReadyById[streamId]),
      getSelectedPipeline: () => $selectedPipeline ?? null,
    getTunePlan: () => tunePlan,
    getTuneStreamsForPipeline: () => tuneStreamsForPipeline,
    getTuneStreamInputOverridesById: () => tuneStreamInputOverridesById,
    getTuneStreamNodeOverridesById: () => tuneStreamNodeOverridesById,
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
      getStreamUpdatesSocket,
      StreamsApi,
      buildErrorMessage,
      reportError,
      toaster,
      buildDaedalusGraphPatch,
      mergeNodeOverrides,
      serializeGraphPlan,
      safeClonePlan,
      isDaedalusPlan,
      streamOverrideSignature
    })
  );

  const { updateGlobalNodeValue, updateStreamNodeValue } = untrack(() =>
    createTuneNodeHandlers({
      getTunePlan: () => tunePlan,
      isDaedalusPlan,
      safeClonePlan,
      handlePlanChange,
      pipelineUpdates,
      setNodeConstantValue,
      scheduleTuneGlobalAutoSave,
      normalizePortKey,
      resolveDataTypeKey,
      getDataTypeVariants,
      buildNodeValueFromInput,
      setTuneNodeDraft,
      setTuneNodeError,
      setTuneStreamNodeDraft,
      clearTuneStreamNodeDraft,
      setTuneStreamNodeError,
      getTuneStreamNodeOverridesById: () => tuneStreamNodeOverridesById,
      setTuneStreamNodeOverridesById: (next) => {
        tuneStreamNodeOverridesById = next;
      },
      scheduleTuneStreamAutoApply
    })
  );

  $effect(() => {
    runTuneControlSocketSync({
      activeTab: $activeTab,
      tunePerformanceTab,
      tunePreviewStream,
      closeTuneControlSocket,
      ensureTuneControlSocket
    });
  });

  onDestroy(() => {
    closeTuneControlSocket();
    if (tuneMultiplexAutoApplyTimer) {
      clearTimeout(tuneMultiplexAutoApplyTimer);
    }
    if (tuneGlobalAutoSaveTimer) {
      clearTimeout(tuneGlobalAutoSaveTimer);
    }
    if (tuneMetricsPollTimer) {
      clearInterval(tuneMetricsPollTimer);
    }
  });

  const ctx = $derived.by(() => ({
    CONTROL_APPLY_DEBOUNCE_MS,
    PIPELINE_UI_METADATA_KEY,
    RAW_STREAM_PIPELINE_ID,
    RAW_STREAM_PIPELINE_UUID,
    TUNE_METRICS_POLL_MS,
    applyStreamControl,
    applyTuneMultiplex,
    applyTuneStreamOverridesFor,
    asRecord,
    boundTunePipelines,
    buildControlValue,
    buildPipelineMetricsSummary,
    buildTuneMetricsSnapshot,
    buildTuneMultiplexLayout,
    buildTuneMultiplexSignature,
    buildTuneMultiplexStateSignature,
    clampControlValue,
    clearTuneMultiplexCell,
    clearTuneNodeDraft,
    clearTuneStreamNodeDraft,
    closeTuneControlSocket,
    controlMax,
    controlMin,
    controlStep,
    displayControlValue,
    extractControlValue,
    dropTuneMultiplexOn,
    ensureTuneControlSocket,
    fetchTuneMetricsSnapshots,
    fetchTuneStreams,
    handleTuneAssign,
    lastTunePipelineId,
    markTuneMultiplexDirty,
    menuOptions,
    menuValueDisplay,
    metricsSource,
    metricsStatusLabel,
    metricsUpdatedLabel,
    pipelineMetricsSummary,
    multiplexKey,
    normalizeMultiplexSlots,
    readTuneNodeDraft,
    readTuneStreamNodeDraft,
    resetTunePipelineUi,
    resolveTunePipelineGraph,
    resolveTunePipelineOutput,
    scheduleControlApply,
    scheduleTuneGlobalAutoSave,
    scheduleTuneMultiplexAutoApply,
    scheduleTuneStreamAutoApply,
    seedControlState,
    seedStreamOverrides,
    saveTunePipelineUi,
    setTuneLivePipelineOutput,
    setTuneMultiplexGridDimensions,
    setTuneNodeDraft,
    setTuneNodeError,
    setTuneOutputKeyForCell,
    setTuneOutputSelectionForPipeline,
    setTuneStreamNodeDraft,
    setTuneStreamNodeError,
    startTuneMultiplexDrag,
    tuneActiveStreamId,
    tuneAllowDrop,
    tuneConstantGroups,
    tuneConstantSearch,
    tuneConstantSearchTokens,
    tuneConstants,
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
    tuneBindings,
    tuneFilteredConstantGroups,
    tuneFilteredControls,
    tuneGlobalAutoSaveTimer,
    tuneMetricsError,
    tuneMetricsInFlight,
    tuneMetricsPollTimer,
    tuneMetricsRefreshPipelineId,
    tuneMetricsRequestId,
    tuneMetricsSnapshots,
    tuneMetricsStatus,
    tuneMetricsUpdatedAt,
    tuneMultiplexAutoApplyTimer,
    tuneMultiplexBusy,
    tuneMultiplexColumnIndices,
    tuneMultiplexColumns,
    tuneMultiplexDirty,
    tuneMultiplexDragPipelineId,
    tuneMultiplexDragSource,
    tuneMultiplexError,
    tuneMultiplexGridIsSingle,
    tuneMultiplexHydratedSignature,
    tuneMultiplexHydratedStreamId,
    tuneMultiplexLayoutSignature,
    tuneMultiplexLastAppliedSignature,
    tuneMultiplexOutputOptionsCache,
    tuneMultiplexPalettePipelineIds,
    tuneMultiplexRowIndices,
    tuneMultiplexRows,
    tuneMultiplexSlotOutputs,
    tuneMultiplexSlots,
    tuneNodeDescriptors,
    tuneNodeDrafts,
    tuneNodeErrors,
    tuneOutputKeyForCell,
    tuneOutputSelectionForPipeline,
    tunePerformanceTab,
    tunePipelineAssignDraft,
    tunePipelineAssignFilteredGraphs,
    tunePipelineAssignModalOpen,
    tunePipelineAssignQuery,
    tunePipelineForCell,
    tunePipelineGraphs,
    tunePipelineRemoveCandidateId,
    tunePipelineRemoveModalOpen,
    tunePipelineUiDraft,
    tunePipelineUiSearch,
    tunePlan,
    tunePreviewStream,
    tuneRuntimeFromStats,
    tuneScopeTab,
    tuneSelectedPipelineOutput,
    tuneSelectedStream,
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
    tuneStreamsForPipeline,
    tuneStreamsLoaded,
    tuneStreamsLoading,
    tuneUiActiveTabId,
    tuneUiEditMode,
    tuneUiMode,
    tuneUiSelectedItemAnchor,
    tuneUiSelectedItemId,
    updateGlobalNodeValue,
    updateStreamNodeValue
  }));
</script>

{@render children?.({ tune: ctx })}
