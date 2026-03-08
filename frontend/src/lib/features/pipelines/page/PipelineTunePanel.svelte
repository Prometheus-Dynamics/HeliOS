<script lang="ts">
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { faPencil } from '@fortawesome/free-solid-svg-icons';
  import StreamPreview from '$lib/components/StreamPreview.svelte';
  import CameraControlsTab from '$lib/features/devices/camera/CameraControlsTab.svelte';
  import CameraPipelinesTab from '$lib/features/devices/camera/CameraPipelinesTab.svelte';
  import PipelineStreamOverridesPanel from '$lib/components/pipelines/PipelineStreamOverridesPanel.svelte';
  import PipelineUiEditorPanel from '$lib/components/pipelines/PipelineUiEditorPanel.svelte';
  import PipelineUiOverridesPanel from '$lib/components/pipelines/PipelineUiOverridesPanel.svelte';
  import PipelineTuneConstantsPanel from '$lib/features/pipelines/page/PipelineTuneConstantsPanel.svelte';
  import { extractGraphOutputPorts } from '$lib/features/pipelines/graphOutputPorts';
  import PipelineOutputsPanel from '$lib/components/pipelines/PipelineOutputsPanel.svelte';
  import { StreamsApi } from '$lib/api/streamsApi';
  import type { ControlMeta, StreamInfo, StreamPipelineGridSlot, StreamPipelineWire } from '$lib/ts-bindings/http/client';
  import type { PipelineOutputEntry, PipelineTuningConstantGroup } from '$lib/components/pipelines/types';
  import type {
    PipelineDataType,
    PipelineGraphPlan,
    PipelineNodeValue,
    PipelineOverviewPipeline,
    PipelineStreamNodeMetrics,
    PipelineTypeDescriptor
  } from '$lib/types/pipeline';
  import type { PipelineUi } from '$lib/features/pipelines/pipelineUiTypes';
  import type { PipelineNodeValueDescriptor } from '$lib/features/devices/camera/page/cameraPipelineTuningController';
  import type { StreamMetricsSummary } from './pipelineTuneMetricsRuntime';

  type TuneNodeErrorMap = Record<string, Record<string, string | null>>;
  type TuneStreamNodeErrorMap = Record<string, Record<string, Record<string, string | null>>>;
  type TuneStreamNodeOverrideMap = Record<string, Record<string, Record<string, PipelineNodeValue>>>;
  type TuneControlValueMap = Record<number, number | boolean | null>;
  type TuneControlBusyMap = Record<number, boolean>;
  type MetricsSource = {
    error?: string | null;
    metrics?: PipelineStreamNodeMetrics[];
  };
  type PipelineGraphListEntry = Record<string, unknown> & {
    id?: string;
    name?: string | null;
  };

  type PipelineTunePanelProps = {
    selectedPipeline?: PipelineOverviewPipeline | null;
    tuneStreamsForPipeline?: StreamInfo[];
    tuneStreamsLoading?: boolean;
    tuneStreamsError?: string | null;
    tunePlan?: PipelineGraphPlan | null;
    tuneNodeDescriptors?: PipelineNodeValueDescriptor[];
    tuneNodeErrors?: TuneNodeErrorMap;
    tuneConstantGroups?: PipelineTuningConstantGroup[];
    tuneFilteredConstantGroups?: PipelineTuningConstantGroup[];
    tuneStreamNodeOverridesById?: TuneStreamNodeOverrideMap;
    tuneStreamNodeErrorsById?: TuneStreamNodeErrorMap;
    tuneStreamApplyErrorById?: Record<string, string | null>;
    tunePreviewStream?: StreamInfo | null;
    tuneUiMode?: 'pipeline' | 'advanced';
    tuneStreamControls?: ControlMeta[];
    tuneControlsLoading?: boolean;
    tuneControlsError?: string | null;
    tuneFilteredControls?: () => ControlMeta[];
    metricsStatusLabel?: string;
    metricsUpdatedLabel?: string;
    metricsSource?: MetricsSource;
    pipelineMetricsSummary?: StreamMetricsSummary[];
    menuOptions?: (ctrl: ControlMeta) => Array<{ value: number; label: string }>;
    tuneMultiplexError?: string | null;
    tuneMultiplexPalettePipelineIds?: string[];
    tuneMultiplexRowIndices?: number[];
    tuneMultiplexColumnIndices?: number[];
    tuneMultiplexOutputOptionsCache?: Record<string, string[]>;
    tuneMultiplexLayoutSignature?: string | null;
    tunePipelineGraphs?: PipelineGraphListEntry[];
    tunePipelineAssignFilteredGraphs?: PipelineGraphListEntry[];
    tuneMultiplexGridIsSingle?: boolean;
    tuneAllowDrop?: (event: DragEvent) => void;
    streamLabel: (stream: StreamInfo) => string;
    normalizePortKey: (value: string) => string;
    readTuneNodeDraft: (nodeId: string, portKey: string) => string | null;
    updateGlobalNodeValue: (nodeId: string, portKey: string, dataType: PipelineDataType | null, raw: string) => void;
    clearTuneNodeDraft: (nodeId: string, portKey: string) => void;
    setTuneNodeError: (nodeId: string, portKey: string, error: string | null) => void;
    scheduleTuneGlobalAutoSave: () => void;
    scheduleTuneMultiplexAutoApply?: () => void;
    setNodeConstantValue: (nodeId: string, portKey: string, value: PipelineNodeValue) => void;
    handlePlanChange: (plan: PipelineGraphPlan) => void;
    isDaedalusPlan: (plan: PipelineGraphPlan | null | undefined) => boolean;
    safeClonePlan: (plan: PipelineGraphPlan) => PipelineGraphPlan;
    readTuneStreamNodeDraft: (streamId: string, nodeId: string, portKey: string) => string | null;
    updateStreamNodeValue: (
      streamId: string,
      nodeId: string,
      portKey: string,
      dataType: PipelineDataType | null,
      raw: string
    ) => void;
    saveTunePipelineUi: () => void;
    resetTunePipelineUi: () => void;
    applyStreamControl: (ctrl: ControlMeta, next: number | boolean | null) => void | Promise<void>;
    scheduleControlApply: (ctrl: ControlMeta, next: number | boolean | null) => void;
    displayControlValue: (ctrl: ControlMeta, value: number | boolean | null) => string;
    extractControlValue: (value: unknown) => number | boolean | null;
    controlMin: (ctrl: ControlMeta) => number | null;
    controlMax: (ctrl: ControlMeta) => number | null;
    controlStep: (ctrl: ControlMeta) => number | null;
    accessLabel: (access: ControlMeta['access'] | string | null | undefined) => string;
    accessBadgeClass: (access: ControlMeta['access'] | string | null | undefined) => string;
    startTuneMultiplexDrag: (pipelineId: string, source?: { row: number; column: number }) => (event: DragEvent) => void;
    pipelineLabelById: (pipelineId: string) => string;
    setTuneMultiplexGridDimensions: (rows: number, columns: number) => void;
    dropTuneMultiplexOn: (row: number, column: number) => (event: DragEvent) => void;
    clearTuneMultiplexCell: (row: number, column: number) => void;
    tunePipelineForCell: (row: number, column: number) => string | null;
    tuneOutputSelectionForPipeline: (pipelineId: string) => string | null;
    tuneOutputKeyForCell: (row: number, column: number) => string | null;
    setTuneOutputSelectionForPipeline: (pipelineId: string, output: string | null) => void;
    setTuneOutputKeyForCell: (row: number, column: number, outputKey: string | null) => void;
    setTuneLivePipelineOutput: (output: string | null) => void | Promise<void>;
    onRequestAssign: () => void;
    RAW_STREAM_PIPELINE_ID: string;
    RAW_STREAM_PIPELINE_UUID: string;

    tuneUiEditMode?: boolean;
    tuneScopeTab?: string;
    tunePipelineUiSearch?: string;
    tunePipelineUiDraft?: PipelineUi | null;
    tuneUiActiveTabId?: string;
    tuneUiSelectedItemId?: string | null;
    tuneUiSelectedItemAnchor?: { x: number; y: number } | null;
    tuneConstantSearch?: string;
    tunePerformanceTab?: 'metrics' | 'controls' | 'layout' | 'outputs';
    tuneControlsQuery?: string;
    tuneShowReadOnlyControls?: boolean;
    tuneControlState?: TuneControlValueMap;
    tuneControlAppliedState?: TuneControlValueMap;
    tuneControlBusy?: TuneControlBusyMap;
    tuneMultiplexRows?: number;
    tuneMultiplexColumns?: number;
    tuneSelectedPipelineOutput?: string | null;
    tunePipelineRemoveModalOpen?: boolean;
    tunePipelineRemoveCandidateId?: string | null;
    tunePipelineAssignModalOpen?: boolean;
    tunePipelineAssignQuery?: string;
    tunePipelineAssignDraft?: string[];

    pipelineOutputEntries?: PipelineOutputEntry[];
    typePalette?: Record<string, PipelineTypeDescriptor>;
  };

  let {
    selectedPipeline = null,
    tuneStreamsForPipeline = [],
    tuneStreamsLoading = false,
    tuneStreamsError = null,
    tunePlan = null,
    tuneNodeDescriptors = [],
    tuneNodeErrors = {},
    tuneConstantGroups = [],
    tuneFilteredConstantGroups = [],
    tuneStreamNodeOverridesById = {},
    tuneStreamNodeErrorsById = {},
    tuneStreamApplyErrorById = {},
    tunePreviewStream = null,
    tuneUiMode = 'pipeline',
    tuneStreamControls = [],
    tuneControlsLoading = false,
    tuneControlsError = null,
    tuneFilteredControls = () => [],
    metricsStatusLabel = 'Offline',
    metricsUpdatedLabel = '—',
    metricsSource = {},
    pipelineMetricsSummary = [],
    menuOptions = () => [],
    tuneMultiplexError = null,
    tuneMultiplexPalettePipelineIds = [],
    tuneMultiplexRowIndices = [],
    tuneMultiplexColumnIndices = [],
    tuneMultiplexOutputOptionsCache = {},
    tuneMultiplexLayoutSignature = null,
    tunePipelineGraphs = [],
    tunePipelineAssignFilteredGraphs = [],
    tuneMultiplexGridIsSingle = false,
    tuneAllowDrop = () => {},
    streamLabel,
    normalizePortKey,
    readTuneNodeDraft,
    updateGlobalNodeValue,
    clearTuneNodeDraft,
    setTuneNodeError,
    scheduleTuneGlobalAutoSave,
    scheduleTuneMultiplexAutoApply = undefined,
    setNodeConstantValue,
    handlePlanChange,
    isDaedalusPlan,
    safeClonePlan,
    readTuneStreamNodeDraft,
    updateStreamNodeValue,
    saveTunePipelineUi,
    resetTunePipelineUi,
    applyStreamControl,
    scheduleControlApply,
    displayControlValue,
    extractControlValue,
    controlMin,
    controlMax,
    controlStep,
    accessLabel,
    accessBadgeClass,
    startTuneMultiplexDrag,
    pipelineLabelById,
    setTuneMultiplexGridDimensions,
    dropTuneMultiplexOn,
    clearTuneMultiplexCell,
    tunePipelineForCell,
    tuneOutputSelectionForPipeline,
    tuneOutputKeyForCell,
    setTuneOutputSelectionForPipeline,
    setTuneOutputKeyForCell,
    setTuneLivePipelineOutput,
    onRequestAssign,
    RAW_STREAM_PIPELINE_ID,
    RAW_STREAM_PIPELINE_UUID,

    tuneUiEditMode = $bindable(false),
    tuneScopeTab = $bindable('global'),
    tunePipelineUiSearch = $bindable(''),
    tunePipelineUiDraft = $bindable(null),
    tuneUiActiveTabId = $bindable(''),
    tuneUiSelectedItemId = $bindable(null),
    tuneUiSelectedItemAnchor = $bindable(null),
    tuneConstantSearch = $bindable(''),
    tunePerformanceTab = $bindable('metrics'),
    tuneControlsQuery = $bindable(''),
    tuneShowReadOnlyControls = $bindable(false),
    tuneControlState = $bindable({}),
    tuneControlAppliedState = $bindable({}),
    tuneControlBusy = $bindable({}),
    tuneMultiplexRows = $bindable(1),
    tuneMultiplexColumns = $bindable(1),
    tuneSelectedPipelineOutput = $bindable(''),
    tunePipelineRemoveModalOpen = $bindable(false),
    tunePipelineRemoveCandidateId = $bindable(null),
    tunePipelineAssignModalOpen = $bindable(false),
    tunePipelineAssignQuery = $bindable(''),
    tunePipelineAssignDraft = $bindable(null),
    pipelineOutputEntries = [],
    typePalette = {}
  }: PipelineTunePanelProps = $props();

  export type $$Props = PipelineTunePanelProps;

  type NodeTimingRow = {
    nodeId: string;
    timeMs: number | null;
    fps: number | null;
    sampleCount: number;
    lastSampleAgeMs: number | null;
    lastError: string | null;
  };

  let tuneMetricsSort = $state<'desc' | 'asc'>('desc');

  const pipelineOutputOptions = $derived.by(() => {
    if (!tunePlan) return [];
    try {
      return extractGraphOutputPorts(tunePlan) ?? [];
    } catch {
      return [];
    }
  });

  const outputTypesByPort: Record<string, PipelineDataType | null | undefined> = $derived.by(() => {
    const map: Record<string, PipelineDataType | null | undefined> = {};
    for (const entry of pipelineOutputEntries ?? []) {
      const name = String(entry?.name ?? '').trim();
      if (!name) continue;
      map[name] = entry.dataType ?? null;
    }
    return map;
  });

  const activeMetricsStream = $derived.by(() =>
    tuneScopeTab === 'global' ? null : tuneStreamsForPipeline.find((stream) => stream.id === tuneScopeTab) ?? null
  );

  const activeMetricsSnapshot: PipelineStreamNodeMetrics | null = $derived.by(() => {
    if (tuneScopeTab === 'global') return null;
    const snapshots = metricsSource?.metrics ?? [];
    return snapshots.find((snapshot) => snapshot?.streamId === tuneScopeTab) ?? null;
  });

  const activeNodeTimingRows: NodeTimingRow[] = $derived.by(() => {
    if (tuneScopeTab === 'global') return [];
    const snapshot = activeMetricsSnapshot;
    const metrics = snapshot?.metrics ?? {};
    const rows: NodeTimingRow[] = Object.entries(metrics).map(([nodeId, runtime]) => {
      const sampleCount = typeof runtime?.metrics?.sampleCount === 'number' ? runtime.metrics.sampleCount : 0;
      const timeMsRaw = typeof runtime?.metrics?.averageTimeMs === 'number' ? runtime.metrics.averageTimeMs : null;
      const fpsRaw = typeof runtime?.metrics?.averageFps === 'number' ? runtime.metrics.averageFps : null;
      const lastSampleAgeMs =
        typeof runtime?.metrics?.lastSampleAgeMs === 'number' ? runtime.metrics.lastSampleAgeMs : null;

      return {
        nodeId,
        timeMs: sampleCount > 0 && timeMsRaw !== null && Number.isFinite(timeMsRaw) ? timeMsRaw : null,
        fps: sampleCount > 0 && fpsRaw !== null && Number.isFinite(fpsRaw) ? fpsRaw : null,
        sampleCount,
        lastSampleAgeMs,
        lastError: typeof runtime?.lastError === 'string' && runtime.lastError.trim() ? runtime.lastError : null
      };
    });

    rows.sort((a, b) => {
      if (a.timeMs === null && b.timeMs === null) return a.nodeId.localeCompare(b.nodeId);
      if (a.timeMs === null) return 1;
      if (b.timeMs === null) return -1;
      const diff = tuneMetricsSort === 'desc' ? b.timeMs - a.timeMs : a.timeMs - b.timeMs;
      return diff !== 0 ? diff : a.nodeId.localeCompare(b.nodeId);
    });

    return rows;
  });

  const normalizeId = (value: unknown): string => (typeof value === 'string' ? value.trim() : '');
  const normalizeKey = (value: unknown): string | null => {
    const normalized = normalizeId(value);
    return normalized.length ? normalized : null;
  };
  const normalizePort = (value: unknown, fallback = 'frame'): string => {
    const normalized = normalizeId(value);
    return normalized.length ? normalized : fallback;
  };
  const normalizePipelineIdForUi = (value: unknown): string => {
    const normalized = normalizeId(value);
    if (!normalized.length) return '';
    if (normalized === RAW_STREAM_PIPELINE_UUID) return RAW_STREAM_PIPELINE_ID;
    return normalized;
  };
  const readManifestWires = (stream: StreamInfo | null | undefined): StreamPipelineWire[] => {
    const manifestRecord =
      stream?.manifest && typeof stream.manifest === 'object'
        ? (stream.manifest as Record<string, unknown>)
        : null;
    const wires = stream?.manifest?.pipeline_wires ?? manifestRecord?.pipelineWires ?? [];
    return Array.isArray(wires) ? wires : [];
  };
  const buildNextPipelineWires = (
    previous: StreamPipelineWire[],
    args: {
      to: { pipelineId: string; outputKey?: string | null };
      from: { pipelineId: string; outputKey?: string | null; port?: string | null } | null;
    }
  ): StreamPipelineWire[] => {
    const toId = normalizeId(args.to?.pipelineId);
    if (!toId.length) return previous;
    const toWireId = toId === RAW_STREAM_PIPELINE_ID ? RAW_STREAM_PIPELINE_UUID : toId;
    const toOutputKey = normalizeKey(args.to?.outputKey);
    const next = previous.filter((wire) => {
      const to = wire?.to ?? null;
      const wireToId = normalizeId(to?.pipeline_id);
      if (!wireToId.length) return true;
      if (wireToId !== toWireId) return true;
      const wireToPort = normalizePort(to?.port, 'frame').toLowerCase();
      if (wireToPort !== 'frame') return true;
      const wireToKey = normalizeKey(to?.output_key);
      return (wireToKey ?? null) !== (toOutputKey ?? null);
    });
    if (!args.from) return next;
    const fromId = normalizeId(args.from.pipelineId);
    if (!fromId.length) return next;
    const fromWireId = fromId === RAW_STREAM_PIPELINE_ID ? RAW_STREAM_PIPELINE_UUID : fromId;
    next.push({
      from: {
        pipeline_id: fromWireId,
        output_key: normalizeKey(args.from.outputKey),
        port: normalizeKey(args.from.port)
      },
      to: {
        pipeline_id: toWireId,
        output_key: toOutputKey,
        port: 'frame'
      }
    });
    return next;
  };

  let tuneWiresByStreamId = $state<Record<string, StreamPipelineWire[]>>({});

  async function setFrameSourceForStream(
    stream: StreamInfo | null | undefined,
    args: {
      to: { pipelineId: string; outputKey?: string | null };
      from: { pipelineId: string; outputKey?: string | null; port?: string | null } | null;
    }
  ): Promise<void> {
    const streamId = normalizeId(stream?.id);
    if (!streamId.length) return;
    const current = (tuneWiresByStreamId[streamId] ?? readManifestWires(stream)).slice();
    const next = buildNextPipelineWires(current, args);
    tuneWiresByStreamId = { ...tuneWiresByStreamId, [streamId]: next };
    try {
      await StreamsApi.setPipelineWires({ id: streamId, requestBody: { wires: next } });
    } catch (error) {
      console.warn('Failed to update pipeline input source', error);
      tuneWiresByStreamId = { ...tuneWiresByStreamId, [streamId]: current };
    }
  }

  const tuneLayoutEditStream = $derived.by(() => tunePreviewStream ?? null);
  const tuneLayoutWires = $derived.by(() => {
    const streamId = normalizeId(tuneLayoutEditStream?.id);
    if (!streamId.length) return [];
    return tuneWiresByStreamId[streamId] ?? readManifestWires(tuneLayoutEditStream);
  });
  const setTuneLayoutFrameSourceForPipelineInstance = (args: {
    to: { pipelineId: string; outputKey?: string | null };
    from: { pipelineId: string; outputKey?: string | null; port?: string | null } | null;
  }): void => {
    if (!tuneLayoutEditStream) return;
    void setFrameSourceForStream(tuneLayoutEditStream, args);
  };

  const tuneInputTargetPipelineId = $derived.by(() => normalizeId(selectedPipeline?.id));
  const tuneInputConfigStream = $derived.by(() => {
    if (tuneScopeTab !== 'global') {
      return tuneStreamsForPipeline.find((stream) => stream.id === tuneScopeTab) ?? tunePreviewStream ?? null;
    }
    return tunePreviewStream ?? null;
  });
  const tuneInputWires = $derived.by(() => {
    const streamId = normalizeId(tuneInputConfigStream?.id);
    if (!streamId.length) return [];
    return tuneWiresByStreamId[streamId] ?? readManifestWires(tuneInputConfigStream);
  });
  const tuneInputLayoutSlots = $derived.by(() => {
    const slots = Array.isArray(tuneInputConfigStream?.manifest?.pipeline_layout?.slots)
      ? (tuneInputConfigStream.manifest.pipeline_layout.slots as StreamPipelineGridSlot[])
      : [];
    return slots
      .map((slot) => {
        const row = Math.trunc(Number(slot?.row));
        const column = Math.trunc(Number(slot?.column));
        if (!Number.isInteger(row) || !Number.isInteger(column)) return null;
        const pipelineId = normalizePipelineIdForUi(slot?.pipeline_id);
        if (!pipelineId.length) return null;
        const outputKey = normalizeKey(slot?.output_key);
        return { row, column, pipelineId, outputKey, resolvedPort: outputKey ?? 'frame' };
      })
      .filter(Boolean) as Array<{ row: number; column: number; pipelineId: string; outputKey: string | null; resolvedPort: string }>;
  });
  const tuneInputLayoutIsMultiplex = $derived.by(() => {
    const rows = Math.max(1, Math.trunc(Number(tuneInputConfigStream?.manifest?.pipeline_layout?.rows ?? 1)));
    const columns = Math.max(1, Math.trunc(Number(tuneInputConfigStream?.manifest?.pipeline_layout?.columns ?? 1)));
    return rows * columns > 1;
  });
  const tuneInputPipelineInLayout = $derived.by(() =>
    Boolean(tuneInputTargetPipelineId && tuneInputLayoutSlots.some((slot) => slot.pipelineId === tuneInputTargetPipelineId))
  );
  const showTuneInputSelector = $derived.by(() =>
    Boolean(
      tuneInputTargetPipelineId &&
        tuneInputTargetPipelineId !== RAW_STREAM_PIPELINE_ID &&
        tuneInputConfigStream &&
        (!tuneInputLayoutIsMultiplex || !tuneInputPipelineInLayout)
    )
  );
  const tuneInputTargetOutputKey = $derived.by(() => {
    if (!tuneInputTargetPipelineId.length) return null;
    const match = tuneInputLayoutSlots.find((slot) => slot.pipelineId === tuneInputTargetPipelineId) ?? null;
    return match?.outputKey ?? null;
  });
  const tuneInputCurrentSelection = $derived.by(() => {
    if (!tuneInputTargetPipelineId.length) return 'raw|raw';
    const toWireId = tuneInputTargetPipelineId === RAW_STREAM_PIPELINE_ID ? RAW_STREAM_PIPELINE_UUID : tuneInputTargetPipelineId;
    const frameWire = tuneInputWires.find((wire) => {
      const to = wire?.to ?? null;
      const wireToId = normalizeId(to?.pipeline_id);
      if (!wireToId.length) return false;
      if (wireToId !== toWireId) return false;
      const wireToPort = normalizePort(to?.port, 'frame').toLowerCase();
      if (wireToPort !== 'frame') return false;
      const wireToKey = normalizeKey(to?.output_key);
      return (wireToKey ?? null) === (tuneInputTargetOutputKey ?? null);
    });
    const from = frameWire?.from ?? null;
    const fromIdRaw = normalizeId(from?.pipeline_id);
    if (!fromIdRaw.length) return 'raw|raw';
    const fromId = normalizePipelineIdForUi(fromIdRaw);
    if (fromId === RAW_STREAM_PIPELINE_ID) {
      const selected = normalizeId(from?.port) || normalizeId(from?.output_key) || 'raw';
      const canonical = selected.toLowerCase() === 'frame' ? 'raw' : selected;
      return `raw|${canonical}`;
    }
    const fromKey = normalizeKey(from?.output_key) ?? '';
    const fromPort = normalizePort(from?.port, 'frame');
    return `pipe|${fromId}|${fromKey}|${fromPort}`;
  });
  const applyTuneInputSelection = (selection: string): void => {
    const pipelineId = tuneInputTargetPipelineId;
    if (!pipelineId || !tuneInputConfigStream) return;
    const trimmed = selection.trim();
    if (!trimmed.length) return;
    if (trimmed.startsWith('raw|')) {
      const parts = trimmed.split('|');
      const port = (parts[1] ?? 'raw').trim() || 'raw';
      void setFrameSourceForStream(tuneInputConfigStream, {
        to: { pipelineId, outputKey: tuneInputTargetOutputKey },
        from: { pipelineId: RAW_STREAM_PIPELINE_ID, outputKey: null, port }
      });
      return;
    }
    if (trimmed.startsWith('pipe|')) {
      const parts = trimmed.split('|');
      const fromId = (parts[1] ?? '').trim();
      if (!fromId.length) return;
      const fromKey = (parts[2] ?? '').trim() || null;
      const port = (parts[3] ?? '').trim() || null;
      void setFrameSourceForStream(tuneInputConfigStream, {
        to: { pipelineId, outputKey: tuneInputTargetOutputKey },
        from: { pipelineId: fromId, outputKey: fromKey, port }
      });
    }
  };
</script>

<section class="flex min-h-0 flex-1 flex-col gap-6 overflow-hidden">
  <div class="flex min-h-0 flex-1 flex-col gap-6 overflow-hidden lg:flex-row">
    <div class="min-h-0 flex-1 overflow-auto pr-1 space-y-4">
      <div class="rounded border border-surface-800/60 bg-surface-950/60 p-5 shadow-lg shadow-black/30">
        <header class="flex items-start justify-between gap-3">
          <div class="space-y-1">
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Tune</p>
            <h2 class="text-lg font-semibold text-white">Pipeline inputs & constants</h2>
            <p class="text-xs text-surface-400">
              Adjust the default values that apply to every consumer. Changes save automatically.
            </p>
          </div>
          <button
            class={`rounded-full border border-surface-700 p-2 text-surface-200 transition ${
              tuneUiEditMode ? 'bg-surface-800 text-white border-surface-600' : 'hover:border-primary-400/60 hover:text-primary-200'
            }`}
            type="button"
            title={tuneUiEditMode ? 'Close UI editor' : 'Edit pipeline UI'}
            aria-pressed={tuneUiEditMode}
            onclick={() => (tuneUiEditMode = !tuneUiEditMode)}
          >
            <FaIcon icon={faPencil} class="h-3.5 w-3.5" />
          </button>
        </header>

        <div class="mt-4 flex flex-wrap items-center gap-2">
          <button
            type="button"
            class={`rounded px-2.5 py-1.5 text-micro-tight uppercase tracking-[0.2em] transition shadow-sm border ${
              tuneScopeTab === 'global'
                ? 'bg-primary-500/20 text-primary-100 border-primary-500/60'
                : 'border-surface-700 text-surface-300 hover:text-primary-200 hover:border-primary-400/60'
            }`}
            onclick={() => (tuneScopeTab = 'global')}
          >
            Global
          </button>
          {#each tuneStreamsForPipeline as stream (stream.id)}
            <button
              type="button"
              class={`rounded px-2.5 py-1.5 text-micro-tight uppercase tracking-[0.2em] transition shadow-sm border ${
                tuneScopeTab === stream.id
                  ? 'bg-primary-500/20 text-primary-100 border-primary-500/60'
                  : 'border-surface-700 text-surface-300 hover:text-primary-200 hover:border-primary-400/60'
              }`}
              onclick={() => (tuneScopeTab = stream.id)}
            >
              {streamLabel(stream)}
            </button>
          {/each}
          <button
            type="button"
            class="rounded px-2.5 py-1.5 text-micro-tight uppercase tracking-[0.2em] transition shadow-sm border border-surface-700 text-surface-300 hover:text-primary-200 hover:border-primary-400/60"
            title="Attach this pipeline to a stream"
            onclick={onRequestAssign}
          >
            +
          </button>
        </div>
        {#if tuneStreamsLoading}
          <p class="mt-2 text-micro-tight text-surface-500">Loading streams…</p>
        {/if}
        {#if tuneStreamsError}
          <div class="mt-2 rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-200">
            {tuneStreamsError}
          </div>
        {/if}
        {#if !selectedPipeline || !tunePlan}
          <div class="mt-4 rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
            Select a pipeline to tune its inputs and constants.
          </div>
        {:else}
          <div class="mt-4 flex flex-wrap items-center gap-2">
            {#if showTuneInputSelector}
              <div class="w-full">
                <span class="block text-micro-tight uppercase tracking-[0.2em] text-surface-500">Input</span>
                <select
                  class="mt-1 w-full rounded border border-surface-800/70 bg-surface-900/60 px-3 py-2 text-xs text-surface-200"
                  value={tuneInputCurrentSelection}
                  onchange={(event) => applyTuneInputSelection((event.currentTarget as HTMLSelectElement).value ?? '')}
                >
                  <option value="raw|raw">Raw stream: raw</option>
                  <option value="raw|undistorted">Raw stream: undistorted</option>
                  {#each tuneInputLayoutSlots as source (`${source.row}:${source.column}`)}
                    {#if source.pipelineId !== tuneInputTargetPipelineId && source.pipelineId !== RAW_STREAM_PIPELINE_ID}
                      <option value={`pipe|${source.pipelineId}|${source.outputKey ?? ''}|${source.resolvedPort}`}>
                        {pipelineLabelById(source.pipelineId)} ({source.row + 1}:{source.column + 1}) - {source.resolvedPort}
                      </option>
                    {/if}
                  {/each}
                </select>
              </div>
            {/if}
            <input
              class="w-full min-w-[220px] flex-1 rounded border border-surface-800/70 bg-surface-900/60 px-3 py-2 text-xs text-surface-200"
              type="search"
              placeholder="Search controls…"
              bind:value={tunePipelineUiSearch}
            />
            {#if tunePipelineUiSearch}
              <button
                class="btn btn-3xs preset-outline uppercase tracking-[0.2em]"
                type="button"
                onclick={() => (tunePipelineUiSearch = '')}
              >
                Clear
              </button>
            {/if}
          </div>
          {#if tuneScopeTab === 'global'}
            {#if tuneUiMode === 'pipeline'}
              <div class="mt-4">
                <PipelineUiOverridesPanel
                  ui={tunePipelineUiDraft}
                  streamLabel="Global"
                  streamId="global"
                  pipelineId={selectedPipeline?.id ?? null}
                  rawPipelineId={RAW_STREAM_PIPELINE_ID}
                  rawPipelineUuid={RAW_STREAM_PIPELINE_UUID}
                  pipelineOutputOptions={pipelineOutputOptions}
                  nodeDescriptors={tuneNodeDescriptors}
                  streamNodeOverrides={{}}
                  streamNodeErrors={tuneNodeErrors}
                  streamError={null}
                readNodeDraft={(nodeId, portKey) => readTuneNodeDraft(nodeId, portKey)}
                updateStreamNodeValue={(nodeId, portKey, dataType, raw) =>
                updateGlobalNodeValue(nodeId, portKey, dataType, raw)
                }
                editMode={tuneUiEditMode}
                searchQuery={tunePipelineUiSearch}
                onEditItem={(item, anchor) => {
                  tuneUiSelectedItemId = item.id ?? null;
                  tuneUiSelectedItemAnchor = anchor ?? null;
                }}
                  onChangeUi={(next) => (tunePipelineUiDraft = next)}
                  activeTabId={tuneUiActiveTabId}
                  onActiveTabChange={(id) => (tuneUiActiveTabId = id)}
                  selectedItemId={tuneUiSelectedItemId}
                  selectedItemAnchor={tuneUiSelectedItemAnchor}
                  onSelectItem={(id) => {
                    tuneUiSelectedItemId = id;
                    if (!id) tuneUiSelectedItemAnchor = null;
                  }}
                />
              </div>
            {:else}
              <div class="mt-4 space-y-4">
                <PipelineTuneConstantsPanel
                  tuneConstantSearch={tuneConstantSearch}
                  tuneConstantGroups={tuneConstantGroups}
                  tuneFilteredConstantGroups={tuneFilteredConstantGroups}
                  tuneNodeErrors={tuneNodeErrors}
                  tunePlan={tunePlan}
                  isDaedalusPlan={isDaedalusPlan}
                  safeClonePlan={safeClonePlan}
                  handlePlanChange={handlePlanChange}
                  normalizePortKey={normalizePortKey}
                  readTuneNodeDraft={readTuneNodeDraft}
                  updateGlobalNodeValue={updateGlobalNodeValue}
                  clearTuneNodeDraft={clearTuneNodeDraft}
                  setTuneNodeError={setTuneNodeError}
                  scheduleTuneGlobalAutoSave={scheduleTuneGlobalAutoSave}
                  setNodeConstantValue={setNodeConstantValue}
                  onSearch={(value) => (tuneConstantSearch = value)}
                />
              </div>
            {/if}
          {:else}
          {@const streamId = tuneScopeTab}
          {@const tunedStream = tuneStreamsForPipeline.find((stream) => stream.id === streamId) ?? null}
          {#if tuneStreamsForPipeline.length === 0}
            <div class="mt-4 rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
              No active streams currently use this pipeline.
            </div>
          {:else if !tunedStream}
            <div class="mt-4 rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
              Pick a stream to apply overrides.
            </div>
          {:else if tuneUiMode === 'pipeline'}
            <div class="mt-4">
              <PipelineUiOverridesPanel
                ui={tunePipelineUiDraft}
                streamLabel={streamLabel(tunedStream)}
                streamId={streamId}
                pipelineId={selectedPipeline?.id ?? null}
                rawPipelineId={RAW_STREAM_PIPELINE_ID}
                rawPipelineUuid={RAW_STREAM_PIPELINE_UUID}
                pipelineOutputOptions={pipelineOutputOptions}
                streamLayout={tunedStream?.manifest?.pipeline_layout ?? null}
                nodeDescriptors={tuneNodeDescriptors}
                streamNodeOverrides={tuneStreamNodeOverridesById[streamId] ?? {}}
                streamNodeErrors={tuneStreamNodeErrorsById[streamId] ?? {}}
                streamError={tuneStreamsError}
                readNodeDraft={(nodeId, portKey) => readTuneStreamNodeDraft(streamId, nodeId, portKey)}
                updateStreamNodeValue={(nodeId, portKey, dataType, raw) =>
                updateStreamNodeValue(streamId, nodeId, portKey, dataType, raw)
                }
                editMode={tuneUiEditMode}
                searchQuery={tunePipelineUiSearch}
                onEditItem={(item, anchor) => {
                  tuneUiSelectedItemId = item.id ?? null;
                  tuneUiSelectedItemAnchor = anchor ?? null;
                }}
                onChangeUi={(next) => (tunePipelineUiDraft = next)}
                activeTabId={tuneUiActiveTabId}
                onActiveTabChange={(id) => (tuneUiActiveTabId = id)}
                selectedItemId={tuneUiSelectedItemId}
                selectedItemAnchor={tuneUiSelectedItemAnchor}
                onSelectItem={(id) => {
                  tuneUiSelectedItemId = id;
                  if (!id) tuneUiSelectedItemAnchor = null;
                }}
              />
              {#if tuneStreamApplyErrorById[streamId]}
                <p class="mt-2 text-xs text-error-300">{tuneStreamApplyErrorById[streamId]}</p>
              {/if}
            </div>
          {:else}
            <div class="mt-4">
              <PipelineStreamOverridesPanel
                streamLabel={streamLabel(tunedStream)}
                streamId={streamId}
                constantSearch={tuneConstantSearch}
                constantGroups={tuneConstantGroups}
                filteredConstantGroups={tuneFilteredConstantGroups}
                streamNodeOverrides={tuneStreamNodeOverridesById[streamId] ?? {}}
                streamNodeErrors={tuneStreamNodeErrorsById[streamId] ?? {}}
                streamError={tuneStreamsError}
                readNodeDraft={(nodeId, portKey) => readTuneStreamNodeDraft(streamId, nodeId, portKey)}
                updateStreamNodeValue={(nodeId, portKey, dataType, raw) =>
                  updateStreamNodeValue(streamId, nodeId, portKey, dataType, raw)
                }
                onSearch={(value) => (tuneConstantSearch = value)}
              />
              {#if tuneStreamApplyErrorById[streamId]}
                <p class="mt-2 text-xs text-error-300">{tuneStreamApplyErrorById[streamId]}</p>
              {/if}
            </div>

          {/if}
        {/if}
      {/if}
      </div>

    </div>

    <div class="min-h-0 w-full pr-1 lg:w-1/3 lg:shrink-0 flex flex-col gap-4">
      <div class="rounded border border-surface-800/60 bg-surface-950/60 p-4 shadow-lg shadow-black/30">
        <div class="flex flex-wrap items-start justify-between gap-3">
          <div>
            <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Live preview</p>
            <p class="text-xs text-surface-400">{tunePreviewStream ? streamLabel(tunePreviewStream) : 'No stream selected'}</p>
          </div>
        </div>
        <div class="mt-3 tune-preview-frame aspect-video min-h-[9rem] overflow-hidden rounded border border-surface-800/70 bg-black">
          {#if tunePreviewStream}
            <StreamPreview
              className="h-full w-full"
              captureSessionId={tunePreviewStream.id}
              captureSessionAlias={streamLabel(tunePreviewStream)}
              recording={Boolean(tunePreviewStream?.status?.recording_active)}
              showCaption={false}
              showFrame={false}
              fitMode="contain"
              enablePopout
            />
          {:else}
            <div class="flex h-full w-full items-center justify-center text-xs text-surface-500">
              No stream available
            </div>
          {/if}
        </div>
      </div>

      {#if tuneUiMode === 'pipeline' && tuneUiEditMode}
        <div class="min-h-0 flex-1 flex flex-col">
          <PipelineUiEditorPanel
            value={tunePipelineUiDraft}
            onChange={(next) => (tunePipelineUiDraft = next)}
            onSave={saveTunePipelineUi}
            onReset={resetTunePipelineUi}
          />
        </div>
      {:else}
        <div class="flex flex-wrap justify-center gap-2">
          <button
            type="button"
            class={`rounded px-2.5 py-1.5 text-[0.7rem] uppercase tracking-[0.14em] transition shadow-sm border ${
              tunePerformanceTab === 'metrics'
                ? 'bg-primary-500/20 text-primary-100 border-primary-500/60'
                : 'border-surface-700 text-surface-300 hover:text-primary-200 hover:border-primary-400/60'
            }`}
            onclick={() => (tunePerformanceTab = 'metrics')}
            aria-pressed={tunePerformanceTab === 'metrics'}
          >
            Metrics
          </button>
          <button
            type="button"
            class={`rounded px-2.5 py-1.5 text-[0.7rem] uppercase tracking-[0.14em] transition shadow-sm border ${
              tunePerformanceTab === 'controls'
                ? 'bg-primary-500/20 text-primary-100 border-primary-500/60'
                : 'border-surface-700 text-surface-300 hover:text-primary-200 hover:border-primary-400/60'
            } ${tunePreviewStream ? '' : 'opacity-60 cursor-not-allowed'}`}
            onclick={() => tunePreviewStream && (tunePerformanceTab = 'controls')}
            disabled={!tunePreviewStream}
            aria-pressed={tunePerformanceTab === 'controls'}
          >
            Controls
          </button>
          <button
            type="button"
            class={`rounded px-2.5 py-1.5 text-[0.7rem] uppercase tracking-[0.14em] transition shadow-sm border ${
              tunePerformanceTab === 'outputs'
                ? 'bg-primary-500/20 text-primary-100 border-primary-500/60'
                : 'border-surface-700 text-surface-300 hover:text-primary-200 hover:border-primary-400/60'
            } ${tunePreviewStream ? '' : 'opacity-60 cursor-not-allowed'}`}
            onclick={() => tunePreviewStream && (tunePerformanceTab = 'outputs')}
            disabled={!tunePreviewStream}
            aria-pressed={tunePerformanceTab === 'outputs'}
          >
            Outputs
          </button>
          <button
            type="button"
            class={`rounded px-2.5 py-1.5 text-[0.7rem] uppercase tracking-[0.14em] transition shadow-sm border ${
              tunePerformanceTab === 'layout'
                ? 'bg-primary-500/20 text-primary-100 border-primary-500/60'
                : 'border-surface-700 text-surface-300 hover:text-primary-200 hover:border-primary-400/60'
            } ${tunePreviewStream ? '' : 'opacity-60 cursor-not-allowed'}`}
            onclick={() => tunePreviewStream && (tunePerformanceTab = 'layout')}
            disabled={!tunePreviewStream}
            aria-pressed={tunePerformanceTab === 'layout'}
          >
            Layout
          </button>
        </div>

        <div class="min-h-0 flex-1 overflow-hidden rounded border border-surface-800/60 bg-surface-950/60 p-5 shadow-lg shadow-black/30">

        {#if tunePerformanceTab === 'metrics'}
          <div class="flex h-full min-h-0 flex-col">
          <div class="flex flex-wrap items-center gap-3 text-xs text-surface-400">
            <span class={`rounded border px-2 py-1 text-micro uppercase tracking-[0.3em] ${
              metricsStatusLabel === 'Live'
                ? 'border-emerald-400/60 text-emerald-200'
                : metricsStatusLabel === 'Offline'
                  ? 'border-error-400/60 text-error-200'
                  : 'border-surface-700 text-surface-300'
            }`}>
              {metricsStatusLabel}
            </span>
            <span>Updated {metricsUpdatedLabel}</span>
          </div>

          {#if metricsSource.error}
            <div class="mt-3 rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-200">
              {metricsSource.error}
            </div>
          {/if}

          {#if tuneScopeTab === 'global'}
            {#if pipelineMetricsSummary.length === 0}
              <div class="mt-4 rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
                No stream metrics reported yet. Start a capture session to see live stats.
              </div>
            {:else}
              <div class="mt-4 min-h-0 flex-1 overflow-y-auto overflow-x-hidden rounded border border-surface-800/70 bg-surface-900/40">
                {#each pipelineMetricsSummary as summary (summary.streamId)}
                  <div class="border-b border-surface-800/70 px-3 py-2 last:border-b-0">
                    <div class="flex items-start justify-between gap-3">
                      <div class="min-w-0">
                        <p class="truncate text-xs font-semibold text-surface-100">{summary.streamLabel}</p>
                        <p class="truncate text-micro-tight text-surface-500">{summary.streamId}</p>
                      </div>
                      {#if summary.errorCount > 0}
                        <span class="shrink-0 rounded border border-error-400/60 bg-error-500/10 px-2 py-1 text-micro-tight uppercase tracking-[0.3em] text-error-200">
                          {summary.errorCount} error{summary.errorCount === 1 ? '' : 's'}
                        </span>
                      {:else}
                        <span class="shrink-0 rounded border border-emerald-400/60 bg-emerald-500/10 px-2 py-1 text-micro-tight uppercase tracking-[0.3em] text-emerald-200">
                          OK
                        </span>
                      {/if}
                    </div>
                    <div class="mt-2 grid grid-cols-3 gap-2 text-micro-tight text-surface-500">
                      <span>Nodes: <span class="text-surface-200">{summary.nodeCount}</span></span>
                      <span>FPS: <span class="text-surface-200">{summary.avgFps === null ? '—' : summary.avgFps.toFixed(1)}</span></span>
                      <span>ms: <span class="text-surface-200">{summary.avgMs === null ? '—' : summary.avgMs.toFixed(1)}</span></span>
                    </div>
                    <div class="mt-2 space-y-1 text-micro-tight text-surface-500">
                      {#if summary.worstNodeId}
                        <p>Slowest: {summary.worstNodeId} · {summary.worstNodeMs?.toFixed(1)} ms</p>
                      {/if}
                      {#if summary.lastError}
                        <p class="line-clamp-2 text-error-300">{summary.lastError}</p>
                      {/if}
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
          {:else}
            <div class="mt-4 flex flex-wrap items-start justify-between gap-3">
              <div class="min-w-0">
                <p class="text-xs uppercase tracking-[0.3em] text-surface-500">Node timings</p>
                <p class="truncate text-xs text-surface-400">
                  {activeMetricsStream ? streamLabel(activeMetricsStream) : activeMetricsSnapshot?.streamPath ?? tuneScopeTab}
                </p>
              </div>
              <div class="flex flex-wrap gap-2">
                <button
                  type="button"
                  class={`rounded px-2 py-1 text-micro uppercase tracking-[0.3em] transition shadow-sm border ${
                    tuneMetricsSort === 'desc'
                      ? 'bg-primary-500/20 text-primary-100 border-primary-500/60'
                      : 'border-surface-700 text-surface-300 hover:text-primary-200 hover:border-primary-400/60'
                  }`}
                  onclick={() => (tuneMetricsSort = 'desc')}
                  aria-pressed={tuneMetricsSort === 'desc'}
                >
                  Most ms
                </button>
                <button
                  type="button"
                  class={`rounded px-2 py-1 text-micro uppercase tracking-[0.3em] transition shadow-sm border ${
                    tuneMetricsSort === 'asc'
                      ? 'bg-primary-500/20 text-primary-100 border-primary-500/60'
                      : 'border-surface-700 text-surface-300 hover:text-primary-200 hover:border-primary-400/60'
                  }`}
                  onclick={() => (tuneMetricsSort = 'asc')}
                  aria-pressed={tuneMetricsSort === 'asc'}
                >
                  Least ms
                </button>
              </div>
            </div>

            {#if !activeMetricsSnapshot}
              <div class="mt-3 rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
                No metrics reported for this stream yet. Start the stream to see live node timings.
              </div>
            {:else if activeNodeTimingRows.length === 0}
              <div class="mt-3 rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
                No node metrics available yet.
              </div>
            {:else}
              <div class="mt-3 min-h-0 flex-1 overflow-y-auto overflow-x-hidden rounded border border-surface-800/70 bg-surface-900/40">
                {#each activeNodeTimingRows as row (row.nodeId)}
                  <div class="border-b border-surface-800/70 px-3 py-2 last:border-b-0">
                    <div class="flex items-start justify-between gap-3">
                      <div class="min-w-0">
                        <p class="truncate text-xs font-semibold text-surface-100">{row.nodeId}</p>
                        <p class="truncate text-micro-tight text-surface-500">
                          FPS: <span class="text-surface-200">{row.fps === null ? '—' : row.fps.toFixed(1)}</span>
                          · Samples: <span class="text-surface-200">{row.sampleCount}</span>
                          · Age:
                          <span class="text-surface-200">{row.lastSampleAgeMs === null ? '—' : `${Math.round(row.lastSampleAgeMs)} ms`}</span>
                        </p>
                        {#if row.lastError}
                          <p class="line-clamp-2 text-micro-tight text-error-300">{row.lastError}</p>
                        {/if}
                      </div>
                      <div class="shrink-0 text-right">
                        <p class="text-xs font-semibold text-surface-100">{row.timeMs === null ? '—' : row.timeMs.toFixed(1)}</p>
                        <p class="text-micro-tight text-surface-500">ms</p>
                      </div>
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
          {/if}
          </div>
        {:else if tunePerformanceTab === 'controls'}
          {#if !tunePreviewStream}
            <div class="rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
              Start a stream to access live controls.
            </div>
          {:else if tuneControlsLoading}
            <p class="text-xs text-surface-400">Loading stream controls…</p>
          {:else if tuneControlsError}
            <div class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-xs text-error-200">
              {tuneControlsError}
            </div>
          {:else}
            <div class="h-full min-h-0 overflow-y-auto pr-1">
              <CameraControlsTab
                controls={tuneStreamControls}
                bind:controlsQuery={tuneControlsQuery}
                bind:showReadOnlyControls={tuneShowReadOnlyControls}
                bind:controlState={tuneControlState}
                bind:controlAppliedState={tuneControlAppliedState}
                bind:controlBusy={tuneControlBusy}
                filteredControls={tuneFilteredControls}
                menuOptions={menuOptions}
                applyControl={applyStreamControl}
                scheduleControlApply={scheduleControlApply}
                displayValue={displayControlValue}
                extractValue={extractControlValue}
                controlMin={controlMin}
                controlMax={controlMax}
                controlStep={controlStep}
                accessLabel={accessLabel}
                accessBadgeClass={accessBadgeClass}
                compact
              />
            </div>
          {/if}
        {:else if tunePerformanceTab === 'outputs'}
          {#if !tunePreviewStream}
            <div class="rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
              Start a stream to view live output samples.
            </div>
          {:else}
            <PipelineOutputsPanel streamId={tunePreviewStream.id} portTypesByName={outputTypesByPort} typePalette={typePalette} />
          {/if}
        {:else}
          {#if !tunePreviewStream}
            <div class="rounded border border-surface-800/60 bg-surface-900/60 px-3 py-2 text-xs text-surface-400">
              Start a stream to manage multiplex layout.
            </div>
          {:else}
            <CameraPipelinesTab
              pipelineGraphError={tuneMultiplexError}
              openPipelineAssignModal={() => {}}
              pipelineGraphLoading={false}
              assignedPipelineIds={tuneMultiplexPalettePipelineIds}
              handlePipelineDragStart={(pipelineId, from) => startTuneMultiplexDrag(pipelineId, from)}
              RAW_PIPELINE_ID={RAW_STREAM_PIPELINE_ID}
              RAW_PIPELINE_UUID={RAW_STREAM_PIPELINE_UUID}
              pipelineLabel={pipelineLabelById}
              openPipelineTuningPanel={undefined}
              openPipelineRemoveModal={() => (tunePipelineRemoveModalOpen = true)}
              pipelineGridIsSingle={tuneMultiplexGridIsSingle}
              setPipelineGridDimensions={setTuneMultiplexGridDimensions}
              bind:pipelineGridRows={tuneMultiplexRows}
              bind:pipelineGridColumns={tuneMultiplexColumns}
              pipelineGridRowIndices={tuneMultiplexRowIndices}
              pipelineGridColumnIndices={tuneMultiplexColumnIndices}
              pipelineForCell={tunePipelineForCell}
              outputSelectionForPipeline={tuneOutputSelectionForPipeline}
              outputKeyForCell={tuneOutputKeyForCell}
              pipelineWires={tuneLayoutWires}
              setFrameSourceForPipelineInstance={setTuneLayoutFrameSourceForPipelineInstance}
              pipelineOutputOptionsCache={tuneMultiplexOutputOptionsCache}
              gridSignature={tuneMultiplexLayoutSignature}
              allowDrop={tuneAllowDrop}
              dropOnCell={dropTuneMultiplexOn}
              clearCell={clearTuneMultiplexCell}
              refreshPipelineGraphs={undefined}
              bind:selectedPipelineOutput={tuneSelectedPipelineOutput}
              setOutputSelectionForPipeline={setTuneOutputSelectionForPipeline}
              setOutputKeyForCell={setTuneOutputKeyForCell}
              setLivePipelineOutput={setTuneLivePipelineOutput}
              bind:pipelineRemoveModalOpen={tunePipelineRemoveModalOpen}
              bind:pipelineRemoveCandidateId={tunePipelineRemoveCandidateId}
              closePipelineRemoveModal={() => (tunePipelineRemoveModalOpen = false)}
              confirmPipelineRemove={() => {}}
              bind:pipelineAssignModalOpen={tunePipelineAssignModalOpen}
              bind:pipelineAssignQuery={tunePipelineAssignQuery}
              bind:pipelineAssignDraft={tunePipelineAssignDraft}
              pipelineAssignFilteredGraphs={tunePipelineAssignFilteredGraphs}
              pipelineGraphs={tunePipelineGraphs}
              closePipelineAssignModal={() => (tunePipelineAssignModalOpen = false)}
              savePipelineAssignModal={() => {}}
              showAssignControls={false}
              showRemoveControls={false}
              schedulePipelineLayoutApply={scheduleTuneMultiplexAutoApply}
            />
          {/if}
        {/if}
      </div>
      {/if}
    </div>
  </div>
</section>
