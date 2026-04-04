import { get, type Readable } from 'svelte/store';
import { untrack } from 'svelte';
import { toaster as appToaster } from '$lib';
import type { ControlMeta, StreamInfo } from '$lib/api/client';
import type { PipelineOverviewPipeline } from '$lib/types/pipeline';
import type { StreamsApi as SharedStreamsApi } from '$lib/api/streamsApi';
import { connectStreamControls, type StreamControlSocket } from '$lib/api/streamControls';
import { buildTuneMultiplexOutputOptionsCache, buildTuneMultiplexPalettePipelineIds } from './pipelineTuneDerived';
import { createTuneControlRuntime, type TuneControlRuntimeDeps } from './pipelineTuneControlRuntime';
import { createTuneMultiplexUi } from './pipelineTuneMultiplexUi';
import { createTuneMultiplexRuntime } from './pipelineTuneMultiplexRuntime';
import { createTuneMultiplexApply, type TuneMultiplexApplyDeps } from './pipelineTuneMultiplexApply';
import { multiplexKey } from './pipelineMultiplexUtils';

type Args = {
  browser: boolean;
  pipelines: Readable<PipelineOverviewPipeline[]>;
  getTunePreviewStream: () => StreamInfo | null;
  getTuneControlSocket: () => StreamControlSocket | null;
  setTuneControlSocket: (next: StreamControlSocket | null) => void;
  getTuneControlSocketStreamId: () => string | null;
  setTuneControlSocketStreamId: (next: string | null) => void;
  tuneControlApplyTimers: Map<number, number>;
  tuneControlApplySeqById: Map<number, number>;
  getTuneControlState: () => Record<number, number | boolean | null>;
  setTuneControlState: (next: Record<number, number | boolean | null>) => void;
  getTuneControlAppliedState: () => Record<number, number | boolean | null>;
  setTuneControlAppliedState: (next: Record<number, number | boolean | null>) => void;
  getTuneControlBusy: () => Record<number, boolean>;
  setTuneControlBusy: (next: Record<number, boolean>) => void;
  getTuneStreamControls: () => ControlMeta[];
  getTuneControlsQuery: () => string;
  getTuneShowReadOnlyControls: () => boolean;
  controlApplyDebounceMs: number;
  StreamsApi: Pick<
    typeof SharedStreamsApi,
    'setControl' | 'setPipelineLayout' | 'smokePipelineGraph' | 'setPipelineGraph' | 'setPipelineOutput'
  >;
  toaster: typeof appToaster;
  reportError: TuneControlRuntimeDeps['reportError'];
  buildErrorMessage: TuneControlRuntimeDeps['buildErrorMessage'];
  clampControlValue: TuneControlRuntimeDeps['clampControlValue'];
  buildControlValue: TuneControlRuntimeDeps['buildControlValue'];
  getTuneMultiplexRows: () => number;
  setTuneMultiplexRows: (next: number) => void;
  getTuneMultiplexColumns: () => number;
  setTuneMultiplexColumns: (next: number) => void;
  getTuneMultiplexSlots: () => Record<string, string | null>;
  setTuneMultiplexSlots: (next: Record<string, string | null>) => void;
  getTuneMultiplexSlotOutputs: () => Record<string, string | null>;
  setTuneMultiplexSlotOutputs: (next: Record<string, string | null>) => void;
  getTuneMultiplexDragPipelineId: () => string | null;
  setTuneMultiplexDragPipelineId: (next: string | null) => void;
  getTuneMultiplexDragSource: () => { row: number; column: number } | null;
  setTuneMultiplexDragSource: (next: { row: number; column: number } | null) => void;
  getTuneSelectedPipelineOutput: () => string | null;
  setTuneSelectedPipelineOutput: (next: string | null) => void;
  getTuneMultiplexAutoApplyTimer: () => number | null;
  setTuneMultiplexAutoApplyTimer: (next: number | null) => void;
  getTuneMultiplexBusy: () => boolean;
  setTuneMultiplexBusy: (next: boolean) => void;
  setTuneMultiplexError: (next: string | null) => void;
  setTuneMultiplexDirty: (next: boolean) => void;
  getTuneMultiplexLastAppliedSignature: () => string | null;
  setTuneMultiplexLastAppliedSignature: (next: string | null) => void;
  setTuneMultiplexHydratedSignature: (next: string | null) => void;
  setTuneMultiplexHydratedStreamId: (next: string | null) => void;
  resolvePipelineLabel: TuneMultiplexApplyDeps['resolvePipelineLabel'];
  RAW_STREAM_PIPELINE_ID: string;
  RAW_STREAM_PIPELINE_UUID: string;
  outputOptionsForPipeline: (pipelineId: string | null) => string[];
  serializeGraphPlan: (plan: unknown) => unknown;
  fetchTuneStreams: () => Promise<StreamInfo[]>;
  buildTuneMultiplexSignature: (
    rows: number,
    columns: number,
    slots: Record<string, string | null>,
    slotOutputs: Record<string, string | null>
  ) => string;
};

export function createTuneWorkspaceInteractionState(args: Args) {
  const {
    browser,
    pipelines,
    getTunePreviewStream,
    getTuneControlSocket,
    setTuneControlSocket,
    getTuneControlSocketStreamId,
    setTuneControlSocketStreamId,
    tuneControlApplyTimers,
    tuneControlApplySeqById,
    getTuneControlState,
    setTuneControlState,
    getTuneControlAppliedState,
    setTuneControlAppliedState,
    getTuneControlBusy,
    setTuneControlBusy,
    getTuneStreamControls,
    getTuneControlsQuery,
    getTuneShowReadOnlyControls,
    controlApplyDebounceMs,
    StreamsApi,
    toaster,
    reportError,
    buildErrorMessage,
    clampControlValue,
    buildControlValue,
    getTuneMultiplexRows,
    setTuneMultiplexRows,
    getTuneMultiplexColumns,
    setTuneMultiplexColumns,
    getTuneMultiplexSlots,
    setTuneMultiplexSlots,
    getTuneMultiplexSlotOutputs,
    setTuneMultiplexSlotOutputs,
    getTuneMultiplexDragPipelineId,
    setTuneMultiplexDragPipelineId,
    getTuneMultiplexDragSource,
    setTuneMultiplexDragSource,
    getTuneSelectedPipelineOutput,
    setTuneSelectedPipelineOutput,
    getTuneMultiplexAutoApplyTimer,
    setTuneMultiplexAutoApplyTimer,
    getTuneMultiplexBusy,
    setTuneMultiplexBusy,
    setTuneMultiplexError,
    setTuneMultiplexDirty,
    getTuneMultiplexLastAppliedSignature,
    setTuneMultiplexLastAppliedSignature,
    setTuneMultiplexHydratedSignature,
    setTuneMultiplexHydratedStreamId,
    resolvePipelineLabel,
    RAW_STREAM_PIPELINE_ID,
    RAW_STREAM_PIPELINE_UUID,
    outputOptionsForPipeline,
    serializeGraphPlan,
    fetchTuneStreams,
    buildTuneMultiplexSignature
  } = args;

  const {
    scheduleControlApply,
    closeTuneControlSocket,
    ensureTuneControlSocket,
    applyStreamControl,
    tuneFilteredControls
  } = untrack(() =>
    createTuneControlRuntime({
      getTunePreviewStream,
      getTuneControlSocket,
      setTuneControlSocket,
      getTuneControlSocketStreamId,
      setTuneControlSocketStreamId,
      tuneControlApplyTimers,
      tuneControlApplySeqById,
      getTuneControlState,
      setTuneControlState,
      getTuneControlAppliedState,
      setTuneControlAppliedState,
      getTuneControlBusy,
      setTuneControlBusy,
      getTuneStreamControls,
      getTuneControlsQuery,
      getTuneShowReadOnlyControls,
      controlApplyDebounceMs,
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
    Array.from({ length: Math.min(Math.max(Math.trunc(getTuneMultiplexRows()), 1), 6) }, (_, i) => i)
  );
  const tuneMultiplexColumnIndices: number[] = $derived.by(() =>
    Array.from({ length: Math.min(Math.max(Math.trunc(getTuneMultiplexColumns()), 1), 6) }, (_, i) => i)
  );

  const tuneMultiplexPalettePipelineIds: string[] = $derived.by(() =>
    buildTuneMultiplexPalettePipelineIds({
      tunePreviewStream: getTunePreviewStream(),
      pipelineLabelById: resolvePipelineLabel,
      RAW_STREAM_PIPELINE_ID,
      RAW_STREAM_PIPELINE_UUID
    })
  );

  const tuneMultiplexOutputOptionsCache: Record<string, string[]> = $derived.by(() => {
    void getTunePreviewStream();
    return buildTuneMultiplexOutputOptionsCache({
      tuneMultiplexPalettePipelineIds,
      tuneMultiplexSlots: getTuneMultiplexSlots(),
      outputOptionsForPipeline
    });
  });

  const tuneMultiplexLayoutSignature = $derived.by(() =>
    buildTuneMultiplexSignature(
      getTuneMultiplexRows(),
      getTuneMultiplexColumns(),
      getTuneMultiplexSlots(),
      getTuneMultiplexSlotOutputs()
    )
  );

  const tuneMultiplexGridIsSingle = $derived.by(
    () => Math.trunc(getTuneMultiplexRows()) === 1 && Math.trunc(getTuneMultiplexColumns()) === 1
  );

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
    getTuneMultiplexRows,
    setTuneMultiplexRows,
    getTuneMultiplexColumns,
    setTuneMultiplexColumns,
    getTuneMultiplexSlots,
    setTuneMultiplexSlots,
    getTuneMultiplexSlotOutputs,
    setTuneMultiplexSlotOutputs,
    getTuneMultiplexGridIsSingle: () => tuneMultiplexGridIsSingle,
    setTuneMultiplexDragPipelineId,
    setTuneMultiplexDragSource,
    getTuneMultiplexDragPipelineId,
    getTuneMultiplexDragSource,
    outputOptionsForPipeline,
    markTuneMultiplexDirty: () => markTuneMultiplexDirty()
  });

  let applyTuneMultiplex: (options?: { quiet?: boolean }) => Promise<void> = async () => {};

  const {
    buildTuneMultiplexStateSignature,
    scheduleTuneMultiplexAutoApply,
    markTuneMultiplexDirty: markTuneMultiplexDirtyRuntime
  } = createTuneMultiplexRuntime({
    browser,
    getTunePreviewStream,
    getTuneMultiplexAutoApplyTimer,
    setTuneMultiplexAutoApplyTimer: (next) => {
      setTuneMultiplexAutoApplyTimer(next);
    },
    getTuneMultiplexBusy,
    setTuneMultiplexDirty,
    getTuneMultiplexRows,
    getTuneMultiplexColumns,
    getTuneMultiplexSlots,
    getTuneMultiplexSlotOutputs,
    getTuneMultiplexLastAppliedSignature,
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
      getTunePreviewStream,
      getTuneMultiplexRows,
      getTuneMultiplexColumns,
      getTuneMultiplexSlots,
      setTuneMultiplexSlots,
      getTuneMultiplexSlotOutputs,
      setTuneMultiplexSlotOutputs,
      getTuneMultiplexGridIsSingle: () => tuneMultiplexGridIsSingle,
      getTuneMultiplexAutoApplyTimer,
      setTuneMultiplexAutoApplyTimer,
      getTuneMultiplexBusy,
      setTuneMultiplexBusy,
      setTuneMultiplexError,
      setTuneMultiplexDirty,
      setTuneMultiplexLastAppliedSignature,
      setTuneMultiplexHydratedSignature,
      setTuneMultiplexHydratedStreamId,
      outputOptionsForPipeline,
      resolvePipelineLabel,
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
    const next = getTuneMultiplexSlotOutputs()[key] ?? null;
    if (getTuneSelectedPipelineOutput() !== next) {
      setTuneSelectedPipelineOutput(next);
    }
  });

  return {
    applyStreamControl,
    applyTuneMultiplex,
    boundTunePipelines,
    buildTuneMultiplexLayout,
    buildTuneMultiplexStateSignature,
    clearTuneMultiplexCell,
    closeTuneControlSocket,
    dropTuneMultiplexOn,
    ensureTuneControlSocket,
    markTuneMultiplexDirty: () => markTuneMultiplexDirty(),
    resolveTunePipelineGraph,
    resolveTunePipelineOutput,
    scheduleControlApply,
    scheduleTuneMultiplexAutoApply,
    setTuneLivePipelineOutput,
    setTuneMultiplexGridDimensions,
    setTuneOutputKeyForCell,
    setTuneOutputSelectionForPipeline,
    startTuneMultiplexDrag,
    get tuneAllowDrop() {
      return tuneAllowDrop;
    },
    get tuneFilteredControls() {
      return tuneFilteredControls;
    },
    get tuneMultiplexColumnIndices() {
      return tuneMultiplexColumnIndices;
    },
    get tuneMultiplexGridIsSingle() {
      return tuneMultiplexGridIsSingle;
    },
    get tuneMultiplexLayoutSignature() {
      return tuneMultiplexLayoutSignature;
    },
    get tuneMultiplexOutputOptionsCache() {
      return tuneMultiplexOutputOptionsCache;
    },
    get tuneMultiplexPalettePipelineIds() {
      return tuneMultiplexPalettePipelineIds;
    },
    get tuneMultiplexRowIndices() {
      return tuneMultiplexRowIndices;
    },
    get tuneOutputKeyForCell() {
      return tuneOutputKeyForCell;
    },
    get tuneOutputSelectionForPipeline() {
      return tuneOutputSelectionForPipeline;
    },
    get tunePipelineForCell() {
      return tunePipelineForCell;
    }
  };
}
