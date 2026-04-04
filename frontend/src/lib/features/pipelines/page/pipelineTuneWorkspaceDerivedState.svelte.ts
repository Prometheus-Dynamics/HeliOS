import { get, type Readable } from 'svelte/store';
import { SvelteMap, SvelteSet } from 'svelte/reactivity';
import { backendFeatures } from '$lib/api/backendFeatures';
import type { StreamInfo } from '$lib/api/client';
import type {
  PipelineDataType,
  PipelineGraphPlan,
  PipelineNodeValue,
  PipelineOverviewPipeline,
  PipelineRegistryEntry
} from '$lib/types/pipeline';
import type { PipelineTuningConstantEntry, PipelineTuningConstantGroup } from '$lib/components/pipelines/types';
import type { PipelineUi } from '$lib/features/pipelines/pipelineUiTypes';
import { fromApiGraphPlan } from '$lib/features/pipelines/graphConverters';
import { extractGraphOutputPorts } from '$lib/features/pipelines/graphOutputPorts';
import {
  registryPortMetadataFor,
  registryPortTypeFor,
  resolveRegistrySnapshotNodeId
} from '$lib/features/devices/camera/page/cameraPipelineTuningController';
import {
  createTuneBindings,
  buildTuneMetricsStreamRefs,
  buildTuneMetricsWantedKey,
  buildTuneMetricsWantedRefs,
  buildTuneStreamsForPipeline,
  findTuneSelectedStream,
  resolveTuneMetadataStream,
  resolveTunePreviewStream
} from './pipelineTunePageSupport';
import {
  buildTuneConstantGroups,
  buildTuneConstantSearchTokens,
  filterTuneConstantGroups
} from './pipelineTuneDerived';
import type { TuneMetricsStreamRef } from './pipelineTuneMetricsRuntime';

type PipelinesApi = {
  listRegistry: (options?: { forceRefresh?: boolean; cacheMs?: number }) => Promise<unknown>;
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

type Args = {
  browser: boolean;
  activeTab: Readable<'pipeline' | 'tune'>;
  selectedPipeline: Readable<PipelineOverviewPipeline | null>;
  editingPlan: Readable<PipelineGraphPlan | null>;
  getTuneStreams: () => StreamInfo[];
  getTuneScopeTab: () => 'global' | string;
  setTuneScopeTab: (next: 'global' | string) => void;
  getTuneUiEditMode: () => boolean;
  setTuneUiEditMode: (next: boolean) => void;
  getTunePipelineUiSearch: () => string;
  setTunePipelineUiSearch: (next: string) => void;
  getTunePipelineUiDraft: () => PipelineUi;
  setTunePipelineUiDraft: (next: PipelineUi) => void;
  getTuneUiActiveTabId: () => string;
  setTuneUiActiveTabId: (next: string) => void;
  getTuneUiSelectedItemId: () => string | null;
  setTuneUiSelectedItemId: (next: string | null) => void;
  getTuneUiSelectedItemAnchor: () => { x: number; y: number } | null;
  setTuneUiSelectedItemAnchor: (next: { x: number; y: number } | null) => void;
  getTunePerformanceTab: () => 'metrics' | 'controls' | 'layout' | 'outputs';
  setTunePerformanceTab: (next: 'metrics' | 'controls' | 'layout' | 'outputs') => void;
  getTuneControlsQuery: () => string;
  setTuneControlsQuery: (next: string) => void;
  getTuneShowReadOnlyControls: () => boolean;
  setTuneShowReadOnlyControls: (next: boolean) => void;
  getTuneControlState: () => Record<number, number | boolean | null>;
  setTuneControlState: (next: Record<number, number | boolean | null>) => void;
  getTuneControlAppliedState: () => Record<number, number | boolean | null>;
  setTuneControlAppliedState: (next: Record<number, number | boolean | null>) => void;
  getTuneControlBusy: () => Record<number, boolean>;
  setTuneControlBusy: (next: Record<number, boolean>) => void;
  getTuneMultiplexRows: () => number;
  setTuneMultiplexRows: (next: number) => void;
  getTuneMultiplexColumns: () => number;
  setTuneMultiplexColumns: (next: number) => void;
  getTuneSelectedPipelineOutput: () => string | null;
  setTuneSelectedPipelineOutput: (next: string | null) => void;
  getTunePipelineRemoveModalOpen: () => boolean;
  setTunePipelineRemoveModalOpen: (next: boolean) => void;
  getTunePipelineRemoveCandidateId: () => string | null;
  setTunePipelineRemoveCandidateId: (next: string | null) => void;
  getTunePipelineAssignModalOpen: () => boolean;
  setTunePipelineAssignModalOpen: (next: boolean) => void;
  getTunePipelineAssignQuery: () => string;
  setTunePipelineAssignQuery: (next: string) => void;
  getTunePipelineAssignDraft: () => string[];
  setTunePipelineAssignDraft: (next: string[]) => void;
  streamUsesPipeline: (stream: StreamInfo, pipelineId: string) => boolean;
  streamLabel: (stream: StreamInfo) => string;
  streamGraphForPipeline: (stream: StreamInfo, pipelineId: string) => unknown | null;
  outputOptionsForPipeline: (pipelineId: string | null) => string[];
  resolveDataTypeKey: (dataType: PipelineDataType | undefined) => string | null;
  getDataTypeVariants: (dataType: PipelineDataType | undefined) => string[];
  resolveRegistryEntryForNode: (node: PipelineGraphPlan['nodes'][string] | undefined) => PipelineRegistryEntry | null;
  extractTuneConstantEntries: (plan: PipelineGraphPlan) => PipelineTuningConstantEntry[];
  normalizePortKey: (value: string | null | undefined) => string;
  RAW_STREAM_PIPELINE_ID: string;
  PipelinesApi: PipelinesApi;
};

export function createTuneWorkspaceDerivedState(args: Args) {
  const {
    browser,
    activeTab,
    selectedPipeline,
    editingPlan,
    getTuneStreams,
    getTuneScopeTab,
    setTuneScopeTab,
    getTuneUiEditMode,
    setTuneUiEditMode,
    getTunePipelineUiSearch,
    setTunePipelineUiSearch,
    getTunePipelineUiDraft,
    setTunePipelineUiDraft,
    getTuneUiActiveTabId,
    setTuneUiActiveTabId,
    getTuneUiSelectedItemId,
    setTuneUiSelectedItemId,
    getTuneUiSelectedItemAnchor,
    setTuneUiSelectedItemAnchor,
    getTunePerformanceTab,
    setTunePerformanceTab,
    getTuneControlsQuery,
    setTuneControlsQuery,
    getTuneShowReadOnlyControls,
    setTuneShowReadOnlyControls,
    getTuneControlState,
    setTuneControlState,
    getTuneControlAppliedState,
    setTuneControlAppliedState,
    getTuneControlBusy,
    setTuneControlBusy,
    getTuneMultiplexRows,
    setTuneMultiplexRows,
    getTuneMultiplexColumns,
    setTuneMultiplexColumns,
    getTuneSelectedPipelineOutput,
    setTuneSelectedPipelineOutput,
    getTunePipelineRemoveModalOpen,
    setTunePipelineRemoveModalOpen,
    getTunePipelineRemoveCandidateId,
    setTunePipelineRemoveCandidateId,
    getTunePipelineAssignModalOpen,
    setTunePipelineAssignModalOpen,
    getTunePipelineAssignQuery,
    setTunePipelineAssignQuery,
    getTunePipelineAssignDraft,
    setTunePipelineAssignDraft,
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
  } = args;

  let tuneConstantSearch = $state('');
  let tuneRegistrySnapshot = $state<unknown | null>(null);
  let tuneRegistrySnapshotLoading = $state(false);

  const tunePlan: PipelineGraphPlan | null = $derived.by(() => {
    const selected = get(selectedPipeline)?.graph ?? null;
    const draft = get(editingPlan) ?? null;
    if (!draft) return selected;
    const draftNodeCount = Object.keys((draft as PipelineGraphPlan).nodes ?? {}).length;
    const selectedNodeCount = Object.keys(selected?.nodes ?? {}).length;
    if (draftNodeCount === 0 && selectedNodeCount > 0) return selected;
    return draft;
  });

  const tuneConstants: PipelineTuningConstantEntry[] = $derived.by(() =>
    tunePlan ? extractTuneConstantEntries(tunePlan) : []
  );

  const tuneConstantGroups: PipelineTuningConstantGroup[] = $derived.by(() =>
    buildTuneConstantGroups(tuneConstants)
  );

  const tuneBindings = $state(
    createTuneBindings({
      getTuneUiEditMode,
      setTuneUiEditMode,
      getTuneScopeTab: () => getTuneScopeTab(),
      setTuneScopeTab: (value) => {
        setTuneScopeTab(value as 'global' | string);
      },
      getTunePipelineUiSearch,
      setTunePipelineUiSearch,
      getTunePipelineUiDraft,
      setTunePipelineUiDraft,
      getTuneUiActiveTabId,
      setTuneUiActiveTabId,
      getTuneUiSelectedItemId,
      setTuneUiSelectedItemId,
      getTuneUiSelectedItemAnchor,
      setTuneUiSelectedItemAnchor,
      getTuneConstantSearch: () => tuneConstantSearch,
      setTuneConstantSearch: (value) => {
        tuneConstantSearch = value;
      },
      getTunePerformanceTab,
      setTunePerformanceTab,
      getTuneControlsQuery,
      setTuneControlsQuery,
      getTuneShowReadOnlyControls,
      setTuneShowReadOnlyControls,
      getTuneControlState,
      setTuneControlState,
      getTuneControlAppliedState,
      setTuneControlAppliedState,
      getTuneControlBusy,
      setTuneControlBusy,
      getTuneMultiplexRows,
      setTuneMultiplexRows,
      getTuneMultiplexColumns,
      setTuneMultiplexColumns,
      getTuneSelectedPipelineOutput,
      setTuneSelectedPipelineOutput,
      getTunePipelineRemoveModalOpen,
      setTunePipelineRemoveModalOpen,
      getTunePipelineRemoveCandidateId,
      setTunePipelineRemoveCandidateId,
      getTunePipelineAssignModalOpen,
      setTunePipelineAssignModalOpen,
      getTunePipelineAssignQuery,
      setTunePipelineAssignQuery,
      getTunePipelineAssignDraft,
      setTunePipelineAssignDraft
    })
  );

  const tuneConstantSearchTokens: string[] = $derived.by(() =>
    buildTuneConstantSearchTokens(tuneConstantSearch)
  );

  const tuneFilteredConstantGroups: PipelineTuningConstantGroup[] = $derived.by(() =>
    filterTuneConstantGroups(tuneConstantGroups, tuneConstantSearchTokens)
  );

  const tuneStreamsForPipeline: StreamInfo[] = $derived.by(() =>
    buildTuneStreamsForPipeline({
      pipeline: get(selectedPipeline) ?? null,
      tuneStreams: getTuneStreams(),
      streamUsesPipeline
    })
  );

  const tuneMetricsStreamRefs: TuneMetricsStreamRef[] = $derived.by(() =>
    buildTuneMetricsStreamRefs({
      pipeline: get(selectedPipeline) ?? null,
      tuneStreams: getTuneStreams(),
      tuneStreamsForPipeline,
      streamLabel
    })
  );

  const tuneMetricsWantedRefs: TuneMetricsStreamRef[] = $derived.by(() =>
    buildTuneMetricsWantedRefs({
      tuneScopeTab: getTuneScopeTab(),
      tuneMetricsStreamRefs
    })
  );

  const tuneMetricsWantedKey: string = $derived.by(() =>
    buildTuneMetricsWantedKey({
      pipelineId: get(selectedPipeline)?.id ?? '',
      tuneScopeTab: getTuneScopeTab(),
      tuneMetricsStreamRefs
    })
  );

  const tuneActiveStreamId = $derived(getTuneScopeTab() === 'global' ? null : getTuneScopeTab());
  const tuneSelectedStream = $derived(findTuneSelectedStream(tuneActiveStreamId, tuneStreamsForPipeline));
  const tunePreviewStream: StreamInfo | null = $derived.by(() =>
    resolveTunePreviewStream(getTuneScopeTab(), tuneSelectedStream)
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
    const pipelineId = get(selectedPipeline)?.id ?? '';
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
          defaultValue: valueForPort(node?.info?.values ?? null, portKey),
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

  const tuneLiveConstants: PipelineTuningConstantEntry[] = $derived.by(() =>
    tuneLivePlan ? extractTuneConstantEntries(tuneLivePlan) : []
  );

  const tuneLiveConstantMap: Map<string, PipelineTuningConstantEntry> = $derived.by(() => {
    const map = new SvelteMap<string, PipelineTuningConstantEntry>();
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
          live && typeof live === "object" && 'baseValue' in live
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

  return {
    get tuneActiveStreamId() {
      return tuneActiveStreamId;
    },
    tuneBindings,
    get tuneConstantGroups() {
      return tuneConstantGroups;
    },
    get tuneConstantSearch() {
      return tuneConstantSearch;
    },
    get tuneConstantSearchTokens() {
      return tuneConstantSearchTokens;
    },
    get tuneConstants() {
      return tuneConstants;
    },
    get tuneFilteredConstantGroups() {
      return tuneFilteredConstantGroups;
    },
    get tuneMetadataStream() {
      return tuneMetadataStream;
    },
    get tuneMetricsStreamRefs() {
      return tuneMetricsStreamRefs;
    },
    get tuneMetricsWantedKey() {
      return tuneMetricsWantedKey;
    },
    get tuneMetricsWantedRefs() {
      return tuneMetricsWantedRefs;
    },
    get tuneNodeDescriptors() {
      return tuneNodeDescriptors;
    },
    outputOptionsForTunePipeline,
    get tunePlan() {
      return tunePlan;
    },
    get tunePreviewStream() {
      return tunePreviewStream;
    },
    get tuneSelectedStream() {
      return tuneSelectedStream;
    },
    get tuneStreamsForPipeline() {
      return tuneStreamsForPipeline;
    }
  };
}
