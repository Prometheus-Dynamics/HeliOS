import type { StreamInfo } from '$lib/api/client';
import type {
  PipelineDataType,
  PipelineGraphPlan,
  PipelineNodeValue,
  PipelineRegistryEntry
} from '$lib/types/pipeline';
import type { PipelineUi } from '$lib/features/pipelines/pipelineUiTypes';
import { createPipelineTuningApply } from './tuningApply';
import { createPipelineTuningDrafts } from './tuningDrafts';
import { clamp, createPipelineTuningPointerHandlers } from './tuningPointers';
import { nodeValueSignature, pipelineOverrideSignature } from './tuningSignatures';
import { PIPELINE_UI_METADATA_KEY } from './cameraPageStateTypes';
import { parseMetadataValue } from './cameraStateUtils';
import {
  asRecord,
  asTrimmedString,
  applyPipelineOverridesToGraph,
  isDaedalusPlan,
  normalizeNodePortKey,
  type PipelineNodeValueDescriptor,
  type PipelineTuningDragState,
  type PipelineTuningPosition,
  type PipelineTuningResizeState,
  type PipelineTuningSize,
  type StreamUpdatesSocket
} from './cameraPipelineTuningSupport';
import {
  PIPELINE_OUTPUT_CELL_KEY,
  PIPELINE_UI_STORAGE_PREFIX,
  RAW_LOOPBACK_GRAPH,
  RAW_PIPELINE_ID,
  RAW_PIPELINE_UUID,
  normalizeAssignedPipelineIds,
  normalizePipelineOutputMap,
  setRawPipelineUuid
} from './cameraPipelineShared';

export { clamp, nodeValueSignature, pipelineOverrideSignature };
export {
  PIPELINE_OUTPUT_CELL_KEY,
  PIPELINE_UI_STORAGE_PREFIX,
  RAW_LOOPBACK_GRAPH,
  RAW_PIPELINE_ID,
  RAW_PIPELINE_UUID,
  normalizeAssignedPipelineIds,
  normalizePipelineOutputMap,
  setRawPipelineUuid
};
export {
  applyDaedalusNodeOverrides,
  extractNodeOverrides,
  extractNodeValueDescriptors,
  findPortMetadata,
  isDaedalusPlan,
  mergeNodeOverrides,
  normalizeNodePortKey,
  normalizePortMetadataMap,
  numberFromRegistryValue,
  pipelineDataTypeFromTypeExpr,
  registryPortMetadataFor,
  registryPortTypeFor,
  resolveRegistrySnapshotNodeId,
  safeCloneGraph
} from './cameraPipelineTuningSupport';
export type { PipelineNodeValueDescriptor } from './cameraPipelineTuningSupport';

type PipelineTuningState = {
  get stream(): StreamInfo | null;
  get streamId(): string;
  get pipelineTuningPlan(): PipelineGraphPlan | null;
  get pipelineTuningGraph(): unknown;
  get pipelineTuningPanelOpen(): boolean;
  set pipelineTuningPanelOpen(value: boolean);
  get pipelineTuningPipelineId(): string | null;
  set pipelineTuningPipelineId(value: string | null);
  get pipelineTuningEngineConfigOpen(): boolean;
  set pipelineTuningEngineConfigOpen(value: boolean);
  get pipelineTuningError(): string | null;
  set pipelineTuningError(value: string | null);
  get pipelineTuningLoading(): boolean;
  set pipelineTuningLoading(value: boolean);
  get pipelineTuningDragState(): PipelineTuningDragState | null;
  set pipelineTuningDragState(value: PipelineTuningDragState | null);
  get pipelineTuningResizeState(): PipelineTuningResizeState | null;
  set pipelineTuningResizeState(value: PipelineTuningResizeState | null);
  get pipelineTuningPosition(): PipelineTuningPosition;
  set pipelineTuningPosition(value: PipelineTuningPosition);
  get pipelineTuningSize(): PipelineTuningSize;
  set pipelineTuningSize(value: PipelineTuningSize);
  get pipelineTuningUiOverride(): PipelineUi | null;
  set pipelineTuningUiOverride(value: PipelineUi | null);
  get pipelineTuningGraphOverride(): unknown;
  set pipelineTuningGraphOverride(value: unknown);
  get pipelineTuningLiveGraph(): unknown;
  set pipelineTuningLiveGraph(value: unknown);
  get pipelineInputOverridesById(): Record<string, Record<string, PipelineNodeValue>>;
  set pipelineInputOverridesById(value: Record<string, Record<string, PipelineNodeValue>>);
  get pipelineNodeOverridesById(): Record<string, Record<string, Record<string, PipelineNodeValue>>>;
  set pipelineNodeOverridesById(value: Record<string, Record<string, Record<string, PipelineNodeValue>>>);
  get pipelineInputDraftsById(): Record<string, Record<string, string>>;
  set pipelineInputDraftsById(value: Record<string, Record<string, string>>);
  get pipelineNodeDraftsById(): Record<string, Record<string, Record<string, string>>>;
  set pipelineNodeDraftsById(value: Record<string, Record<string, Record<string, string>>>);
  get pipelineInputErrorsById(): Record<string, Record<string, string>>;
  set pipelineInputErrorsById(value: Record<string, Record<string, string>>);
  get pipelineNodeErrorsById(): Record<string, Record<string, Record<string, string>>>;
  set pipelineNodeErrorsById(value: Record<string, Record<string, Record<string, string>>>);
  get pipelineTuningApplyBusy(): boolean;
  set pipelineTuningApplyBusy(value: boolean);
  get pipelineTuningApplyQueuedById(): Record<string, boolean>;
  set pipelineTuningApplyQueuedById(value: Record<string, boolean>);
  get pipelineTuningLastAppliedSignatureById(): Record<string, string>;
  set pipelineTuningLastAppliedSignatureById(value: Record<string, string>);
  get pipelineTuningLastAppliedNodeOverridesById(): Record<string, Record<string, Record<string, PipelineNodeValue>>>;
  set pipelineTuningLastAppliedNodeOverridesById(value: Record<string, Record<string, Record<string, PipelineNodeValue>>>);
  get pipelineTuningApplyRafById(): Map<string, number>;
  get pipelineGraphCache(): Record<string, unknown>;
  get pipelineRegistrySnapshot(): unknown;
};

type PipelineTuningDeps = {
  ensurePipelineRegistry: () => Promise<void>;
  ensurePipelineGraphAndOutputs: (
    pipelineId: string,
    forceRefresh?: boolean
  ) => Promise<{ graphJson: unknown; filtered: string[]; types: Record<string, PipelineDataType | null | undefined> }>;
  awaitStreamUpdatesSocket: (streamId: string, timeoutMs?: number) => Promise<StreamUpdatesSocket>;
};

export function createPipelineTuningController(state: PipelineTuningState, deps: PipelineTuningDeps) {
  const tuningApply = createPipelineTuningApply(state, {
    awaitStreamUpdatesSocket: deps.awaitStreamUpdatesSocket,
    applyPipelineOverridesToGraph,
    isDaedalusPlan
  });
  const tuningDrafts = createPipelineTuningDrafts(state, {
    normalizeNodePortKey,
    schedulePipelineTuningApply: tuningApply.schedulePipelineTuningApply
  });
  const pointerHandlers = createPipelineTuningPointerHandlers(state);

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
    updatePipelineNodeValue
  } = tuningDrafts;

  const { schedulePipelineTuningApply, applyPipelineTuningOverrides } = tuningApply;

  const {
    stopPipelineTuningPointerTracking,
    startPipelineTuningPointerTracking,
    onPipelineTuningPointerMove,
    onPipelineTuningPointerUp,
    startPipelineTuningDrag,
    startPipelineTuningResize
  } = pointerHandlers;

  const extractPipelineUi = (graph: unknown): PipelineUi | null => {
    const parseMaybeNestedJson = (value: string): unknown => {
      const parsed = parseMetadataValue(value);
      if (typeof parsed !== 'string') return parsed;
      const trimmed = parsed.trim();
      if (!trimmed) return parsed;
      if ((trimmed.startsWith('{') && trimmed.endsWith('}')) || (trimmed.startsWith('[') && trimmed.endsWith(']'))) {
        return parseMetadataValue(trimmed);
      }
      return parsed;
    };

    const graphRecord = asRecord(graph);
    if (!graphRecord) return null;
    const metadata =
      graphRecord.metadata ??
      asRecord(graphRecord.daedalus)?.metadata ??
      asRecord(graphRecord.graph)?.metadata ??
      asRecord(graphRecord.pipeline_graph)?.metadata ??
      null;
    const metadataRecord = asRecord(metadata);
    if (!metadataRecord) return null;
    const raw =
      metadataRecord[PIPELINE_UI_METADATA_KEY] ??
      metadataRecord['helios.pipeline_ui'] ??
      metadataRecord['pipeline.ui'] ??
      null;
    if (!raw) return null;
    if (typeof raw === 'string') {
      return (parseMaybeNestedJson(raw) as PipelineUi | null) ?? null;
    }
    if (raw && typeof raw === 'object' && 'value' in (raw as Record<string, unknown>)) {
      const value = (raw as { value?: unknown }).value;
      if (typeof value === 'string') {
        return (parseMaybeNestedJson(value) as PipelineUi | null) ?? null;
      }
      return (value as PipelineUi | null) ?? null;
    }
    return raw as PipelineUi;
  };

  const extractStreamGraphForPipeline = (pipelineId: string): unknown | null => {
    const stream = state.stream;
    const manifest = stream?.manifest ?? null;
    if (!manifest) return null;
    const normalized = String(pipelineId ?? '').trim();
    if (!normalized.length) return null;
    const manifestRecord = asRecord(manifest);
    const readGraph = (value: unknown) => {
      const record = asRecord(value);
      return record?.pipeline_graph ?? null;
    };
    const activeId = asTrimmedString(manifestRecord?.active_pipeline_id);
    if (activeId && activeId === normalized) return null;
    if (Array.isArray(manifestRecord?.pipelines)) {
      for (const entry of manifestRecord.pipelines) {
        const entryRecord = asRecord(entry);
        if (!entryRecord) continue;
        const entryId = typeof entryRecord.pipeline_id === 'string' ? entryRecord.pipeline_id.trim() : '';
        if (entryId && entryId === normalized) {
          return readGraph(entry);
        }
      }
    }
    return null;
  };

  async function openPipelineTuningPanel(pipelineId: string): Promise<void> {
    state.pipelineTuningPipelineId = pipelineId;
    state.pipelineTuningPanelOpen = true;
    state.pipelineTuningEngineConfigOpen = false;
    state.pipelineTuningError = null;
    state.pipelineTuningLoading = false;
    state.pipelineTuningUiOverride = null;
    state.pipelineTuningGraphOverride = null;
    state.pipelineTuningLiveGraph = null;
    if (typeof window !== 'undefined') {
      const margin = 12;
      const minWidth = 460;
      const minHeight = 420;
      const maxWidth = Math.max(minWidth, window.innerWidth - margin * 2);
      const maxHeight = Math.max(minHeight, window.innerHeight - margin * 2);
      const nextWidth = clamp(state.pipelineTuningSize.width, minWidth, maxWidth);
      const nextHeight = clamp(state.pipelineTuningSize.height, minHeight, maxHeight);
      const maxX = Math.max(margin, window.innerWidth - nextWidth - margin);
      const maxY = Math.max(margin, window.innerHeight - nextHeight - margin);
      state.pipelineTuningSize = { width: nextWidth, height: nextHeight };
      state.pipelineTuningPosition = {
        x: clamp(state.pipelineTuningPosition.x, margin, maxX),
        y: clamp(state.pipelineTuningPosition.y, margin, maxY)
      };
    }
    if (pipelineId === RAW_PIPELINE_ID) return;
    state.pipelineTuningLoading = true;
    try {
      const [, graphResult] = await Promise.all([
        deps.ensurePipelineRegistry(),
        deps.ensurePipelineGraphAndOutputs(pipelineId, true)
      ]);
      const graphJson = graphResult?.graphJson ?? state.pipelineGraphCache?.[pipelineId] ?? null;
      const uiOverride = extractPipelineUi(graphJson);
      state.pipelineTuningUiOverride = uiOverride;
      state.pipelineTuningGraphOverride = graphJson ?? null;
      state.pipelineTuningLiveGraph = extractStreamGraphForPipeline(pipelineId);
    } catch (err) {
      console.warn('Failed to load pipeline graph for tuning panel', err);
      state.pipelineTuningError = 'Unable to load pipeline graph.';
    } finally {
      state.pipelineTuningLoading = false;
    }
  }

  function closePipelineTuningPanel(): void {
    state.pipelineTuningPanelOpen = false;
    state.pipelineTuningPipelineId = null;
    state.pipelineTuningDragState = null;
    state.pipelineTuningResizeState = null;
    state.pipelineTuningEngineConfigOpen = false;
    state.pipelineTuningUiOverride = null;
    state.pipelineTuningGraphOverride = null;
    state.pipelineTuningLiveGraph = null;
    stopPipelineTuningPointerTracking();
  }

  function applyPipelineOverridesToGraphForState(pipelineId: string, graph: unknown): unknown {
    return applyPipelineOverridesToGraph(
      pipelineId,
      graph,
      state.pipelineInputOverridesById,
      state.pipelineNodeOverridesById
    );
  }

  return {
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
    applyPipelineOverridesToGraph: applyPipelineOverridesToGraphForState,
    nodeValueSignature,
    pipelineOverrideSignature,
    clamp
  };
}
