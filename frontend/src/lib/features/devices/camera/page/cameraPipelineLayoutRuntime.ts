import { PipelinesApi } from '$lib/api/pipelinesApi';
import { StreamsApi } from '$lib/api/streamsApi';
import { reportError } from '$lib/ui/errorPolicy';
import { createPipelineLayoutController } from './cameraPipelineLayoutController';
import { PIPELINE_UI_STORAGE_PREFIX } from './cameraPipelineTuningController';

type LayoutRuntimeArgs = {
  streamId: string;
  stream: () => any;
  manifestState: () => any;
  pipelineState: any;
  pipelineRegistrySnapshot: () => any;
  setPipelineRegistrySnapshot: (value: any) => void;
  pipelineGraphCache: () => any;
  setPipelineGraphCache: (value: any) => void;
  pipelineOutputOptionsCache: () => any;
  setPipelineOutputOptionsCache: (value: any) => void;
  refresh: () => Promise<void>;
  scheduleStreamPresetApply: () => void;
  onExternalLayoutApplied?: () => void;
  applyPipelineOverridesToGraph: (pipelineId: string, graph: any) => any;
  apiPath: (path: string) => string;
  apiBase?: string;
  layoutDebounceMs: number;
};

export function createCameraPipelineLayoutRuntime(args: LayoutRuntimeArgs) {
  const {
    streamId,
    stream,
    manifestState,
    pipelineState,
    pipelineRegistrySnapshot,
    setPipelineRegistrySnapshot,
    pipelineGraphCache,
    setPipelineGraphCache,
    pipelineOutputOptionsCache,
    setPipelineOutputOptionsCache,
    refresh,
    scheduleStreamPresetApply,
    onExternalLayoutApplied,
    applyPipelineOverridesToGraph,
    apiPath,
    apiBase,
    layoutDebounceMs
  } = args;

  return createPipelineLayoutController(
    {
      get stream() {
        return stream();
      },
      get streamId() {
        return streamId;
      },
      get manifestState() {
        return manifestState();
      },
      get pipelineRegistrySnapshot() {
        return pipelineRegistrySnapshot();
      },
      set pipelineRegistrySnapshot(value) {
        setPipelineRegistrySnapshot(value);
      },
      get pipelineGraphCache() {
        return pipelineGraphCache();
      },
      set pipelineGraphCache(value) {
        setPipelineGraphCache(value);
      },
      get pipelineOutputOptionsCache() {
        return pipelineOutputOptionsCache();
      },
      set pipelineOutputOptionsCache(value) {
        setPipelineOutputOptionsCache(value);
      },
      get pipelineOutputByPipelineId() {
        return pipelineState.pipelineOutputByPipelineId;
      },
      set pipelineOutputByPipelineId(value) {
        pipelineState.pipelineOutputByPipelineId = value;
      },
      get pipelineGraphLoading() {
        return pipelineState.pipelineGraphLoading;
      },
      set pipelineGraphLoading(value) {
        pipelineState.pipelineGraphLoading = value;
      },
      get pipelineGraphError() {
        return pipelineState.pipelineGraphError;
      },
      set pipelineGraphError(value) {
        pipelineState.pipelineGraphError = value;
      },
      get pipelineGraphs() {
        return pipelineState.pipelineGraphs;
      },
      set pipelineGraphs(value) {
        pipelineState.pipelineGraphs = value;
      },
      get pipelineOutputOptions() {
        return pipelineState.pipelineOutputOptions;
      },
      set pipelineOutputOptions(value) {
        pipelineState.pipelineOutputOptions = value;
      },
      get selectedPipelineOutput() {
        return pipelineState.selectedPipelineOutput;
      },
      set selectedPipelineOutput(value) {
        pipelineState.selectedPipelineOutput = value;
      },
      get selectedPipelineGraph() {
        return pipelineState.selectedPipelineGraph;
      },
      set selectedPipelineGraph(value) {
        pipelineState.selectedPipelineGraph = value;
      },
      get pipelineLayoutApplyTimer() {
        return pipelineState.pipelineLayoutApplyTimer;
      },
      set pipelineLayoutApplyTimer(value) {
        pipelineState.pipelineLayoutApplyTimer = value;
      },
      get pipelineLayoutTouched() {
        return pipelineState.pipelineLayoutTouched;
      },
      set pipelineLayoutTouched(value) {
        pipelineState.pipelineLayoutTouched = value;
      },
      get pipelineAssignmentsTouched() {
        return pipelineState.pipelineAssignmentsTouched;
      },
      set pipelineAssignmentsTouched(value) {
        pipelineState.pipelineAssignmentsTouched = value;
      },
      get pipelineGridRows() {
        return pipelineState.pipelineGridRows;
      },
      set pipelineGridRows(value) {
        pipelineState.pipelineGridRows = value;
      },
      get pipelineGridColumns() {
        return pipelineState.pipelineGridColumns;
      },
      set pipelineGridColumns(value) {
        pipelineState.pipelineGridColumns = value;
      },
      get pipelineGridSlots() {
        return pipelineState.pipelineGridSlots;
      },
      set pipelineGridSlots(value) {
        pipelineState.pipelineGridSlots = value;
      },
      get pipelineGridSlotOutputKeys() {
        return pipelineState.pipelineGridSlotOutputKeys;
      },
      set pipelineGridSlotOutputKeys(value) {
        pipelineState.pipelineGridSlotOutputKeys = value;
      },
      get assignedPipelineIds() {
        return pipelineState.assignedPipelineIds;
      },
      set assignedPipelineIds(value) {
        pipelineState.assignedPipelineIds = value;
      },
      get pipelineAssignDraft() {
        return pipelineState.pipelineAssignDraft;
      },
      set pipelineAssignDraft(value) {
        pipelineState.pipelineAssignDraft = value;
      },
      get pipelineAssignQuery() {
        return pipelineState.pipelineAssignQuery;
      },
      set pipelineAssignQuery(value) {
        pipelineState.pipelineAssignQuery = value;
      },
      get pipelineAssignModalOpen() {
        return pipelineState.pipelineAssignModalOpen;
      },
      set pipelineAssignModalOpen(value) {
        pipelineState.pipelineAssignModalOpen = value;
      },
      get pipelineRemoveModalOpen() {
        return pipelineState.pipelineRemoveModalOpen;
      },
      set pipelineRemoveModalOpen(value) {
        pipelineState.pipelineRemoveModalOpen = value;
      },
      get pipelineRemoveCandidateId() {
        return pipelineState.pipelineRemoveCandidateId;
      },
      set pipelineRemoveCandidateId(value) {
        pipelineState.pipelineRemoveCandidateId = value;
      },
      get pipelineDragPayload() {
        return pipelineState.pipelineDragPayload;
      },
      set pipelineDragPayload(value) {
        pipelineState.pipelineDragPayload = value;
      },
      get selectedPipelineId() {
        return pipelineState.selectedPipelineId;
      },
      set selectedPipelineId(value) {
        pipelineState.selectedPipelineId = value;
      },
      get pipelineUiHydrated() {
        return pipelineState.pipelineUiHydrated;
      }
    },
    {
      pipelinesApi: PipelinesApi,
      streamsApi: StreamsApi,
      reportError,
      refresh,
      scheduleStreamPresetApply,
      onExternalLayoutApplied,
      applyPipelineOverridesToGraph,
      apiPath,
      apiBase,
      layoutDebounceMs,
      storagePrefix: PIPELINE_UI_STORAGE_PREFIX
    }
  );
}
