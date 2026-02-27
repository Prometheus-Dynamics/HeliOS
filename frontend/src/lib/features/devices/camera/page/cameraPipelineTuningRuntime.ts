import { createPipelineTuningController } from './cameraPipelineTuningController';
import type { PipelineDataType } from '$lib/types/pipeline';

type TuningRuntimeArgs = {
  streamId: string;
  stream: () => any;
  pipelineState: any;
  derived: any;
  ensurePipelineRegistry: () => Promise<void>;
  ensurePipelineGraphAndOutputs: (
    pipelineId: string,
    forceRefresh?: boolean
  ) => Promise<{ graphJson: any; filtered: string[]; types: Record<string, PipelineDataType | null | undefined> }>;
  awaitStreamUpdatesSocket: (streamId: string, timeoutMs?: number) => Promise<unknown>;
  pipelineGraphCache: () => any;
  pipelineRegistrySnapshot: () => any;
};

export function createCameraPipelineTuningRuntime(args: TuningRuntimeArgs) {
  const {
    streamId,
    stream,
    pipelineState,
    derived,
    ensurePipelineRegistry,
    ensurePipelineGraphAndOutputs,
    awaitStreamUpdatesSocket,
    pipelineGraphCache,
    pipelineRegistrySnapshot
  } = args;

  return createPipelineTuningController(
    {
      get stream() {
        return stream();
      },
      get streamId() {
        return streamId;
      },
      get pipelineTuningPlan() {
        return derived.pipelineTuningPlan;
      },
      get pipelineTuningGraph() {
        return derived.pipelineTuningGraph;
      },
      get pipelineTuningPanelOpen() {
        return pipelineState.pipelineTuningPanelOpen;
      },
      set pipelineTuningPanelOpen(value) {
        pipelineState.pipelineTuningPanelOpen = value;
      },
      get pipelineTuningPipelineId() {
        return pipelineState.pipelineTuningPipelineId;
      },
      set pipelineTuningPipelineId(value) {
        pipelineState.pipelineTuningPipelineId = value;
      },
      get pipelineTuningEngineConfigOpen() {
        return pipelineState.pipelineTuningEngineConfigOpen;
      },
      set pipelineTuningEngineConfigOpen(value) {
        pipelineState.pipelineTuningEngineConfigOpen = value;
      },
      get pipelineTuningError() {
        return pipelineState.pipelineTuningError;
      },
      set pipelineTuningError(value) {
        pipelineState.pipelineTuningError = value;
      },
      get pipelineTuningLoading() {
        return pipelineState.pipelineTuningLoading;
      },
      set pipelineTuningLoading(value) {
        pipelineState.pipelineTuningLoading = value;
      },
      get pipelineTuningDragState() {
        return pipelineState.pipelineTuningDragState;
      },
      set pipelineTuningDragState(value) {
        pipelineState.pipelineTuningDragState = value;
      },
      get pipelineTuningResizeState() {
        return pipelineState.pipelineTuningResizeState;
      },
      set pipelineTuningResizeState(value) {
        pipelineState.pipelineTuningResizeState = value;
      },
      get pipelineTuningPosition() {
        return pipelineState.pipelineTuningPosition;
      },
      set pipelineTuningPosition(value) {
        pipelineState.pipelineTuningPosition = value;
      },
      get pipelineTuningSize() {
        return pipelineState.pipelineTuningSize;
      },
      set pipelineTuningSize(value) {
        pipelineState.pipelineTuningSize = value;
      },
      get pipelineTuningUiOverride() {
        return pipelineState.pipelineTuningUiOverride;
      },
      set pipelineTuningUiOverride(value) {
        pipelineState.pipelineTuningUiOverride = value;
      },
      get pipelineTuningGraphOverride() {
        return pipelineState.pipelineTuningGraphOverride;
      },
      set pipelineTuningGraphOverride(value) {
        pipelineState.pipelineTuningGraphOverride = value;
      },
      get pipelineTuningLiveGraph() {
        return pipelineState.pipelineTuningLiveGraph;
      },
      set pipelineTuningLiveGraph(value) {
        pipelineState.pipelineTuningLiveGraph = value;
      },
      get pipelineInputOverridesById() {
        return pipelineState.pipelineInputOverridesById;
      },
      set pipelineInputOverridesById(value) {
        pipelineState.pipelineInputOverridesById = value;
      },
      get pipelineNodeOverridesById() {
        return pipelineState.pipelineNodeOverridesById;
      },
      set pipelineNodeOverridesById(value) {
        pipelineState.pipelineNodeOverridesById = value;
      },
      get pipelineInputDraftsById() {
        return pipelineState.pipelineInputDraftsById;
      },
      set pipelineInputDraftsById(value) {
        pipelineState.pipelineInputDraftsById = value;
      },
      get pipelineNodeDraftsById() {
        return pipelineState.pipelineNodeDraftsById;
      },
      set pipelineNodeDraftsById(value) {
        pipelineState.pipelineNodeDraftsById = value;
      },
      get pipelineInputErrorsById() {
        return pipelineState.pipelineInputErrorsById;
      },
      set pipelineInputErrorsById(value) {
        pipelineState.pipelineInputErrorsById = value;
      },
      get pipelineNodeErrorsById() {
        return pipelineState.pipelineNodeErrorsById;
      },
      set pipelineNodeErrorsById(value) {
        pipelineState.pipelineNodeErrorsById = value;
      },
      get pipelineTuningApplyBusy() {
        return pipelineState.pipelineTuningApplyBusy;
      },
      set pipelineTuningApplyBusy(value) {
        pipelineState.pipelineTuningApplyBusy = value;
      },
      get pipelineTuningApplyQueuedById() {
        return pipelineState.pipelineTuningApplyQueuedById;
      },
      set pipelineTuningApplyQueuedById(value) {
        pipelineState.pipelineTuningApplyQueuedById = value;
      },
      get pipelineTuningLastAppliedSignatureById() {
        return pipelineState.pipelineTuningLastAppliedSignatureById;
      },
      set pipelineTuningLastAppliedSignatureById(value) {
        pipelineState.pipelineTuningLastAppliedSignatureById = value;
      },
      get pipelineTuningLastAppliedNodeOverridesById() {
        return pipelineState.pipelineTuningLastAppliedNodeOverridesById;
      },
      set pipelineTuningLastAppliedNodeOverridesById(value) {
        pipelineState.pipelineTuningLastAppliedNodeOverridesById = value;
      },
      get pipelineTuningApplyRafById() {
        return pipelineState.pipelineTuningApplyRafById;
      },
      get pipelineGraphCache() {
        return pipelineGraphCache();
      },
      get pipelineRegistrySnapshot() {
        return pipelineRegistrySnapshot();
      }
    },
    {
      ensurePipelineRegistry,
      ensurePipelineGraphAndOutputs: async (pipelineId, forceRefresh) =>
        ensurePipelineGraphAndOutputs(pipelineId, forceRefresh),
      awaitStreamUpdatesSocket
    }
  );
}
