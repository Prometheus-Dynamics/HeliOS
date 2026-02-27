import type { StreamInfo } from '$lib/api/httpClient';
import { StreamsApi } from '$lib/api/streamsApi';
import type { PipelineGraphPlan, PipelineNodeValue } from '$lib/types/pipeline';
import { buildDaedalusGraphPatch } from '$lib/features/pipelines/daedalusGraph';
import { reportError } from '$lib/ui/errorPolicy';
import { pipelineOverrideSignature } from './tuningSignatures';

export type PipelineTuningApplyState = {
  get stream(): StreamInfo | null;
  get streamId(): string;
  get pipelineTuningPlan(): PipelineGraphPlan | null;
  get pipelineTuningGraph(): any;
  get pipelineInputOverridesById(): Record<string, Record<string, PipelineNodeValue>>;
  get pipelineNodeOverridesById(): Record<string, Record<string, Record<string, PipelineNodeValue>>>;
  get pipelineTuningLastAppliedNodeOverridesById(): Record<string, Record<string, Record<string, PipelineNodeValue>>>;
  set pipelineTuningLastAppliedNodeOverridesById(value: Record<string, Record<string, Record<string, PipelineNodeValue>>>);
  get pipelineTuningLastAppliedSignatureById(): Record<string, string>;
  set pipelineTuningLastAppliedSignatureById(value: Record<string, string>);
  get pipelineTuningApplyBusy(): boolean;
  set pipelineTuningApplyBusy(value: boolean);
  get pipelineTuningApplyQueuedById(): Record<string, boolean>;
  set pipelineTuningApplyQueuedById(value: Record<string, boolean>);
  get pipelineTuningApplyRafById(): Map<string, number>;
};

export type PipelineTuningApplyDeps = {
  awaitStreamUpdatesSocket: (streamId: string, timeoutMs?: number) => Promise<{ send?: (payload: any) => boolean | void } | null>;
  applyPipelineOverridesToGraph: (
    pipelineId: string,
    graph: any,
    inputOverridesById: Record<string, Record<string, PipelineNodeValue>>,
    nodeOverridesById: Record<string, Record<string, Record<string, PipelineNodeValue>>>
  ) => any;
  isDaedalusPlan: (plan: PipelineGraphPlan | null | undefined) => boolean;
};

export function createPipelineTuningApply(state: PipelineTuningApplyState, deps: PipelineTuningApplyDeps) {
  function schedulePipelineTuningApply(pipelineId: string): void {
    const effectiveStreamId = state.stream?.id ?? state.streamId;
    if (!effectiveStreamId || !pipelineId) return;
    if (state.pipelineTuningApplyRafById.has(pipelineId)) return;
    const handle = requestAnimationFrame(() => {
      state.pipelineTuningApplyRafById.delete(pipelineId);
      void applyPipelineTuningOverrides(pipelineId, effectiveStreamId);
    });
    state.pipelineTuningApplyRafById.set(pipelineId, handle);
  }

  async function applyPipelineTuningOverrides(pipelineId: string, streamId: string): Promise<void> {
    if (!state.pipelineTuningPlan) return;
    if (state.pipelineTuningApplyBusy) {
      state.pipelineTuningApplyQueuedById = { ...state.pipelineTuningApplyQueuedById, [pipelineId]: true };
      return;
    }
    const inputOverrides = state.pipelineInputOverridesById?.[pipelineId] ?? {};
    const nodeOverrides = state.pipelineNodeOverridesById?.[pipelineId] ?? {};
    const previousNodeOverrides = state.pipelineTuningLastAppliedNodeOverridesById?.[pipelineId] ?? {};
    const signature = pipelineOverrideSignature(streamId, pipelineId, inputOverrides, nodeOverrides);
    if (state.pipelineTuningLastAppliedSignatureById?.[pipelineId] === signature) {
      return;
    }
    state.pipelineTuningApplyBusy = true;
    try {
      const inputPayload = Object.fromEntries(Object.entries(inputOverrides).map(([key, value]) => [key, value.value ?? null]));
      const hasInputs = Object.keys(inputPayload).length > 0;
      const hasNodeOverrides = Object.values(nodeOverrides).some((entry) => entry && Object.keys(entry).length > 0);
      const hadNodeOverrides = Object.values(previousNodeOverrides).some((entry) => entry && Object.keys(entry).length > 0);
      const shouldApplyNodeOverrides = hasNodeOverrides || hadNodeOverrides;

      const socket = await deps.awaitStreamUpdatesSocket(streamId);

      if (hasInputs && !shouldApplyNodeOverrides) {
        const sent = socket?.send?.({
          type: 'set_inputs',
          pipeline_id: pipelineId,
          inputs: inputPayload
        });
        if (!sent) {
          await StreamsApi.setPipelineInputs({
            id: streamId,
            requestBody: { pipeline_id: pipelineId, inputs: inputPayload }
          });
        }
        state.pipelineTuningLastAppliedSignatureById = { ...state.pipelineTuningLastAppliedSignatureById, [pipelineId]: signature };
        return;
      }
      if (!hasInputs && !shouldApplyNodeOverrides) {
        state.pipelineTuningLastAppliedSignatureById = { ...state.pipelineTuningLastAppliedSignatureById, [pipelineId]: signature };
        return;
      }

      let graphApplied = false;
      if (shouldApplyNodeOverrides && deps.isDaedalusPlan(state.pipelineTuningPlan)) {
        const patch = buildDaedalusGraphPatch(state.pipelineTuningPlan, nodeOverrides, previousNodeOverrides);
        if (patch.ops.length === 0) {
          const graph = deps.applyPipelineOverridesToGraph(
            pipelineId,
            state.pipelineTuningGraph,
            state.pipelineInputOverridesById,
            state.pipelineNodeOverridesById
          );
          if (!graph) return;
          const sentGraph = socket?.send?.({
            type: 'set_graph',
            pipeline_id: pipelineId,
            graph
          });
          if (!sentGraph) {
            await StreamsApi.setPipelineGraph({
              id: streamId,
              requestBody: { graph, pipeline_id: pipelineId }
            });
          }
        } else {
          const sentPatch = socket?.send?.({
            type: 'set_graph_patch',
            pipeline_id: pipelineId,
            patch
          });
          if (!sentPatch) {
            await StreamsApi.setPipelineGraphPatch({
              id: streamId,
              requestBody: { patch, pipeline_id: pipelineId }
            });
          }
        }
        graphApplied = true;
        state.pipelineTuningLastAppliedNodeOverridesById = {
          ...state.pipelineTuningLastAppliedNodeOverridesById,
          [pipelineId]: nodeOverrides
        };
      } else if (shouldApplyNodeOverrides) {
        const graph = deps.applyPipelineOverridesToGraph(
          pipelineId,
          state.pipelineTuningGraph,
          state.pipelineInputOverridesById,
          state.pipelineNodeOverridesById
        );
        if (!graph) return;
        const sentGraph = socket?.send?.({
          type: 'set_graph',
          pipeline_id: pipelineId,
          graph
        });
        if (!sentGraph) {
          await StreamsApi.setPipelineGraph({
            id: streamId,
            requestBody: { graph, pipeline_id: pipelineId }
          });
        }
        graphApplied = true;
        state.pipelineTuningLastAppliedNodeOverridesById = {
          ...state.pipelineTuningLastAppliedNodeOverridesById,
          [pipelineId]: nodeOverrides
        };
      }
      if (hasInputs) {
        const sentInputs = socket?.send?.({
          type: 'set_inputs',
          pipeline_id: pipelineId,
          inputs: inputPayload
        });
        if (!sentInputs) {
          await StreamsApi.setPipelineInputs({
            id: streamId,
            requestBody: { pipeline_id: pipelineId, inputs: inputPayload }
          });
        }
      }
      if (graphApplied || !shouldApplyNodeOverrides) {
        state.pipelineTuningLastAppliedSignatureById = { ...state.pipelineTuningLastAppliedSignatureById, [pipelineId]: signature };
      }
    } catch (err) {
      console.warn('Failed to apply pipeline overrides', err);
      reportError({
        title: 'Pipeline tuning apply failed',
        error: err,
        fallback: `Unable to apply tuning for pipeline ${pipelineId}.`
      });
    } finally {
      state.pipelineTuningApplyBusy = false;
      if (state.pipelineTuningApplyQueuedById?.[pipelineId]) {
        const next = { ...state.pipelineTuningApplyQueuedById };
        delete next[pipelineId];
        state.pipelineTuningApplyQueuedById = next;
        schedulePipelineTuningApply(pipelineId);
      }
    }
  }

  return {
    schedulePipelineTuningApply,
    applyPipelineTuningOverrides
  };
}
