import type { StreamsApi as SharedStreamsApi } from '$lib/api/streamsApi';
import { loadOwnedStreams } from '$lib/api/streamResources';
import type { StreamInfo } from '$lib/api/client';
import type { PipelineGraphPlan, PipelineNodeValue, PipelineOverviewPipeline } from '$lib/types/pipeline';
import { fromApiGraphPlan } from '$lib/features/pipelines/graphConverters';
import { serializeGraphPlan } from '$lib/features/pipelines/graph';
import { nodeOverridesFromDaedalusPatch } from '$lib/features/pipelines/daedalusGraph';
import { extractInputValues, isDaedalusPlan } from './pipelineTuneConstantUtils';
import {
  diffDaedalusNodeValues,
  extractNodeOverridesFromGraph,
  nodeValueSignature,
  normalizePortKey
} from './pipelineTuneState';

type StreamsApi = Pick<typeof SharedStreamsApi, 'setPipelineGraphPatch' | 'setPipelineGraph' | 'setPipelineInputs'>;

type DaedalusPatch = Parameters<typeof nodeOverridesFromDaedalusPatch>[0];

export const createTuneStreamOverrides = (options: {
  StreamsApi: StreamsApi;
  streamGraphForPipeline: (stream: StreamInfo, pipelineId: string) => unknown | null;
  getTuneStreamNodeOverridesById: () => Record<string, Record<string, Record<string, PipelineNodeValue>>>;
  setTuneStreamNodeOverridesById: (next: Record<string, Record<string, Record<string, PipelineNodeValue>>>) => void;
  getTuneStreamLastAppliedNodeOverridesById: () => Record<string, Record<string, Record<string, PipelineNodeValue>>>;
  setTuneStreamLastAppliedNodeOverridesById: (next: Record<string, Record<string, Record<string, PipelineNodeValue>>>) => void;
  getTuneStreamOverridesLoaded: () => Record<string, boolean>;
  setTuneStreamOverridesLoaded: (next: Record<string, boolean>) => void;
  getTuneStreamInputOverridesById: () => Record<string, Record<string, PipelineNodeValue>>;
  setTuneStreamInputOverridesById: (next: Record<string, Record<string, PipelineNodeValue>>) => void;
}) => {
  const fetchTuneStreams = async (): Promise<StreamInfo[]> => {
    return loadOwnedStreams();
  };

  const seedStreamOverrides = (stream: StreamInfo, pipelineId: string, baseGraph: PipelineGraphPlan | null): void => {
    const streamGraph = options.streamGraphForPipeline(stream, pipelineId);
    const manifest = stream.manifest ?? null;
    const binding = Array.isArray(manifest?.pipelines)
      ? manifest.pipelines.find((candidate) => candidate?.pipeline_id === pipelineId)
      : null;
    const streamPatch = binding?.pipeline_patch ?? null;
    if (!baseGraph) return;
    const baseIsDaedalus = isDaedalusPlan(baseGraph);

    if (baseIsDaedalus) {
      if (streamPatch) {
        const nodeOverrideEntries = nodeOverridesFromDaedalusPatch(streamPatch as DaedalusPatch, baseGraph);
        if (Object.keys(nodeOverrideEntries).length > 0) {
          options.setTuneStreamNodeOverridesById({
            ...options.getTuneStreamNodeOverridesById(),
            [stream.id]: nodeOverrideEntries
          });
        }
        options.setTuneStreamLastAppliedNodeOverridesById({
          ...options.getTuneStreamLastAppliedNodeOverridesById(),
          [stream.id]: nodeOverrideEntries
        });
        options.setTuneStreamOverridesLoaded({
          ...options.getTuneStreamOverridesLoaded(),
          [stream.id]: true
        });
        return;
      }
      if (!streamGraph) return;
      const streamPlan = fromApiGraphPlan(streamGraph ?? {});
      const nodeOverrideEntries = diffDaedalusNodeValues(baseGraph, streamPlan);
      if (Object.keys(nodeOverrideEntries).length > 0) {
        options.setTuneStreamNodeOverridesById({
          ...options.getTuneStreamNodeOverridesById(),
          [stream.id]: nodeOverrideEntries
        });
      }
      options.setTuneStreamLastAppliedNodeOverridesById({
        ...options.getTuneStreamLastAppliedNodeOverridesById(),
        [stream.id]: nodeOverrideEntries
      });
      options.setTuneStreamOverridesLoaded({
        ...options.getTuneStreamOverridesLoaded(),
        [stream.id]: true
      });
      return;
    }

    if (!streamGraph) return;
    const baseSerialized = serializeGraphPlan(baseGraph);
    const baseInputs = extractInputValues(baseSerialized);
    const streamInputs = extractInputValues(streamGraph);
    const baseOverrides = extractNodeOverridesFromGraph(baseSerialized);
    const streamOverrides = extractNodeOverridesFromGraph(streamGraph);

    const inputOverrides: Record<string, PipelineNodeValue> = {};
    for (const [key, value] of Object.entries(streamInputs)) {
      const normalizedKey = normalizePortKey(key);
      if (!normalizedKey) continue;
      if (nodeValueSignature(value) === nodeValueSignature(baseInputs[normalizedKey])) continue;
      inputOverrides[normalizedKey] = value;
    }

    const nodeOverrideEntries: Record<string, Record<string, PipelineNodeValue>> = {};
    for (const [nodeId, overrides] of Object.entries(streamOverrides)) {
      const baseNodeOverrides = baseOverrides?.[nodeId] ?? {};
      for (const [portKey, value] of Object.entries(overrides ?? {})) {
        const normalizedPort = normalizePortKey(portKey);
        if (!normalizedPort) continue;
        if (nodeValueSignature(value) === nodeValueSignature(baseNodeOverrides[normalizedPort])) continue;
        nodeOverrideEntries[nodeId] = { ...(nodeOverrideEntries[nodeId] ?? {}), [normalizedPort]: value };
      }
    }

    if (Object.keys(inputOverrides).length > 0) {
      options.setTuneStreamInputOverridesById({
        ...options.getTuneStreamInputOverridesById(),
        [stream.id]: inputOverrides
      });
    }
    if (Object.keys(nodeOverrideEntries).length > 0) {
      options.setTuneStreamNodeOverridesById({
        ...options.getTuneStreamNodeOverridesById(),
        [stream.id]: nodeOverrideEntries
      });
    }
    options.setTuneStreamLastAppliedNodeOverridesById({
      ...options.getTuneStreamLastAppliedNodeOverridesById(),
      [stream.id]: nodeOverrideEntries
    });
    options.setTuneStreamOverridesLoaded({
      ...options.getTuneStreamOverridesLoaded(),
      [stream.id]: true
    });
  };

  return { fetchTuneStreams, seedStreamOverrides };
};

export type TuneStreamOverrideRuntimeDeps = {
  browser: boolean;
  getStreamUpdatesReadyById: (streamId: string) => boolean;
  getSelectedPipeline: () => PipelineOverviewPipeline | null;
  getTunePlan: () => PipelineGraphPlan | null;
  getTuneStreamsForPipeline: () => StreamInfo[];
  getTuneStreamInputOverridesById: () => Record<string, Record<string, PipelineNodeValue>>;
  getTuneStreamNodeOverridesById: () => Record<string, Record<string, Record<string, PipelineNodeValue>>>;
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
  getStreamUpdatesSocket: (streamId: string) => { ready?: () => boolean; send?: (payload: Record<string, unknown>) => boolean } | null;
  StreamsApi: StreamsApi;
  buildErrorMessage: (params: { error: unknown; fallback: string }) => string;
  toaster: { success: (payload: { title: string }) => void; error: (payload: { title: string; description?: string }) => void };
  reportError: (params: { title: string; error: unknown; fallback: string }) => void;
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
};

export const createTuneStreamOverrideRuntime = (deps: TuneStreamOverrideRuntimeDeps) => {
  const STREAM_OVERRIDE_DEBOUNCE_MS = 400;

  const scheduleTuneStreamAutoApply = (streamId: string): void => {
    if (!deps.browser) return;
    const timers = deps.getTuneStreamAutoApplyTimerById();
    const existing = timers[streamId];
    if (existing) {
      clearTimeout(existing);
    }
    const nextTimer = window.setTimeout(() => {
      const rafs = deps.getTuneStreamApplyRafById();
      const existingRaf = rafs[streamId];
      if (existingRaf) {
        cancelAnimationFrame(existingRaf);
      }
      const handle = requestAnimationFrame(() => {
        const current = deps.getTuneStreamApplyRafById();
        const nextRafs = { ...current };
        delete nextRafs[streamId];
        deps.setTuneStreamApplyRafById(nextRafs);
        void applyTuneStreamOverridesFor(streamId, { quiet: true });
      });
      deps.setTuneStreamApplyRafById({ ...deps.getTuneStreamApplyRafById(), [streamId]: handle });
    }, STREAM_OVERRIDE_DEBOUNCE_MS);
    deps.setTuneStreamAutoApplyTimerById({ ...timers, [streamId]: nextTimer });
  };

  const resolveStream = (streamId: string): StreamInfo | null =>
    deps.getTuneStreamsForPipeline().find((stream) => stream.id === streamId) ?? null;

  const applyTuneStreamOverridesFor = async (
    streamId: string,
    options: { quiet?: boolean } = {}
  ): Promise<void> => {
    if (!deps.browser) return;
    const selectedPipeline = deps.getSelectedPipeline();
    const pipelineId = selectedPipeline?.id ?? null;
    if (!pipelineId) return;
    const stream = resolveStream(streamId);
    if (!stream) return;
    const plan = deps.getTunePlan();
    if (!plan) return;

    const busyMap = deps.getTuneStreamApplyBusyById();
    if (busyMap[streamId]) {
      deps.setTuneStreamApplyQueuedById({ ...deps.getTuneStreamApplyQueuedById(), [streamId]: true });
      return;
    }

    const inputOverrides = deps.getTuneStreamInputOverridesById()[streamId] ?? {};
    const nodeOverrides = deps.getTuneStreamNodeOverridesById()[streamId] ?? {};
    const previousNodeOverrides = deps.getTuneStreamLastAppliedNodeOverridesById()[streamId] ?? {};
    const signature = deps.streamOverrideSignature(pipelineId, streamId, inputOverrides, nodeOverrides);
    const lastApplied = deps.getTuneStreamLastAppliedSignatureById()[streamId] ?? null;
    if (signature === lastApplied) return;

    deps.setTuneStreamApplyBusyById({ ...busyMap, [streamId]: true });
    deps.setTuneStreamApplyErrorById({ ...deps.getTuneStreamApplyErrorById(), [streamId]: null });

    const timers = deps.getTuneStreamAutoApplyTimerById();
    if (timers[streamId]) {
      clearTimeout(timers[streamId]);
      const nextTimers = { ...timers };
      delete nextTimers[streamId];
      deps.setTuneStreamAutoApplyTimerById(nextTimers);
    }

    try {
      const inputPayload = Object.fromEntries(
        Object.entries(inputOverrides).map(([key, value]) => [key, value?.value ?? null])
      );
      const hasInputs = Object.keys(inputPayload).length > 0;
      const hasNodeOverrides = Object.values(nodeOverrides).some((entry) => entry && Object.keys(entry).length > 0);
      const hadNodeOverrides = Object.values(previousNodeOverrides).some((entry) => entry && Object.keys(entry).length > 0);
      const shouldApplyNodeOverrides = hasNodeOverrides || hadNodeOverrides;

      const socket = deps.getStreamUpdatesReadyById(streamId) ? deps.getStreamUpdatesSocket(streamId) : null;

      if (!hasInputs && !shouldApplyNodeOverrides) {
        deps.setTuneStreamLastAppliedSignatureById({
          ...deps.getTuneStreamLastAppliedSignatureById(),
          [streamId]: signature
        });
        return;
      }

      if (shouldApplyNodeOverrides) {
        if (deps.isDaedalusPlan(plan)) {
          const patch = deps.buildDaedalusGraphPatch(plan, nodeOverrides, previousNodeOverrides);
          if (patch.ops.length > 0) {
            const sent = socket?.send?.({ type: 'set_graph_patch', pipeline_id: pipelineId, patch });
            if (!sent) {
              await deps.StreamsApi.setPipelineGraphPatch({
                id: streamId,
                requestBody: { patch, pipeline_id: pipelineId }
              });
            }
          }
        } else {
          const nextPlan = deps.safeClonePlan(plan);
          nextPlan.nodeValueOverrides = deps.mergeNodeOverrides(nextPlan.nodeValueOverrides ?? {}, nodeOverrides);
          nextPlan.pipelineInputValues = { ...(nextPlan.pipelineInputValues ?? {}), ...inputOverrides };
          const graph = deps.serializeGraphPlan(nextPlan);
          const sent = socket?.send?.({ type: 'set_graph', pipeline_id: pipelineId, graph });
          if (!sent) {
            await deps.StreamsApi.setPipelineGraph({ id: streamId, requestBody: { graph, pipeline_id: pipelineId } });
          }
        }
        deps.setTuneStreamLastAppliedNodeOverridesById({
          ...deps.getTuneStreamLastAppliedNodeOverridesById(),
          [streamId]: nodeOverrides
        });
      }

      if (hasInputs) {
        const sentInputs = socket?.send?.({ type: 'set_inputs', pipeline_id: pipelineId, inputs: inputPayload });
        if (!sentInputs) {
          await deps.StreamsApi.setPipelineInputs({
            id: streamId,
            requestBody: { pipeline_id: pipelineId, inputs: inputPayload }
          });
        }
      }

      deps.setTuneStreamLastAppliedSignatureById({
        ...deps.getTuneStreamLastAppliedSignatureById(),
        [streamId]: signature
      });
      if (!options.quiet) {
        deps.toaster.success({ title: 'Stream overrides applied' });
      }
    } catch (error) {
      const message = deps.buildErrorMessage({ error, fallback: 'Unable to apply stream overrides.' });
      deps.setTuneStreamApplyErrorById({ ...deps.getTuneStreamApplyErrorById(), [streamId]: message });
      if (!options.quiet) {
        deps.reportError({
          title: 'Stream update failed',
          error,
          fallback: message
        });
      }
    } finally {
      const nextBusy = { ...deps.getTuneStreamApplyBusyById() };
      delete nextBusy[streamId];
      deps.setTuneStreamApplyBusyById(nextBusy);
      const queued = deps.getTuneStreamApplyQueuedById();
      if (queued[streamId]) {
        const nextQueued = { ...queued };
        delete nextQueued[streamId];
        deps.setTuneStreamApplyQueuedById(nextQueued);
        void applyTuneStreamOverridesFor(streamId, { quiet: true });
      }
    }
  };

  return { scheduleTuneStreamAutoApply, applyTuneStreamOverridesFor };
};
