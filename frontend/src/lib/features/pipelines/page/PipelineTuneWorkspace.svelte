<script lang="ts">
  import type { Snippet } from 'svelte';
  import { createPipelineTuneWorkspaceContext } from './pipelineTuneWorkspaceContext.svelte';
  import type { StreamsApi as SharedStreamsApi } from '$lib/api/streamsApi';
  import type { ControlMeta, StreamInfo, StreamPipelineLayout } from '$lib/api/client';
  import type {
    PipelineDataType,
    PipelineGraphPlan,
    PipelineNodeValue,
    PipelineOverviewPipeline,
    PipelineRegistryEntry
  } from '$lib/types/pipeline';
  import type { PipelineTuningConstantEntry } from '$lib/components/pipelines/types';

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
    activeTab: import('svelte/store').Readable<'pipeline' | 'tune'>;
    pipelineUpdates: PipelineUpdates;
    pipelineUpdatesReady: import('svelte/store').Readable<boolean>;
    streamUpdatesReadyById: import('svelte/store').Readable<Record<string, boolean>>;
    getStreamUpdatesSocket: (streamId: string) => { ready?: () => boolean; send?: (payload: Record<string, unknown>) => boolean } | null;
    selectedPipeline: import('svelte/store').Readable<PipelineOverviewPipeline | null>;
    editingPlan: import('svelte/store').Readable<PipelineGraphPlan | null>;
    detailContext: import('svelte/store').Readable<unknown>;
    pipelines: import('svelte/store').Readable<PipelineOverviewPipeline[]>;
    pipelineLabelById: (pipelineId: string) => string;
    streamUsesPipeline: (stream: StreamInfo, pipelineId: string) => boolean;
    streamLabel: (stream: StreamInfo) => string;
    streamGraphForPipeline: (stream: StreamInfo, pipelineId: string) => unknown | null;
    outputOptionsForPipeline: (pipelineId: string | null) => string[];
    handlePlanChange: (plan: PipelineGraphPlan) => void;
    setNodeConstantValue: (nodeId: string, portKey: string, value: PipelineNodeValue) => void;
    saveCurrentPipeline: () => Promise<void>;
    openAssignModal: () => void;
    assignBusy: import('svelte/store').Readable<boolean>;
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

  let { children, ...deps }: TuneDeps = $props();
  const ctx = createPipelineTuneWorkspaceContext(deps as TuneDeps);
</script>

{@render children?.({ tune: ctx })}
