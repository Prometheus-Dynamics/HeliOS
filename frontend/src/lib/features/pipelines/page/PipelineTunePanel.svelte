<script lang="ts">
  import { buildStreamPreviewProps } from '$lib/components/streamViewerSurface';
  import PipelineTuneInputsSection from '$lib/features/pipelines/page/PipelineTuneInputsSection.svelte';
  import PipelineTuneInspectorSection from '$lib/features/pipelines/page/PipelineTuneInspectorSection.svelte';
  import type {
    MetricsSource,
    NodeTimingRow,
    PipelineGraphListEntry,
    PipelineTunePanelProps,
    TuneControlBusyMap,
    TuneControlValueMap,
    TuneNodeErrorMap,
    TuneStreamNodeErrorMap,
    TuneStreamNodeOverrideMap
  } from '$lib/features/pipelines/page/PipelineTunePanel.types';
  import { extractGraphOutputPorts } from '$lib/features/pipelines/graphOutputPorts';
  import { streamRecordingActive } from '$lib/api/streamRuntime';
  import { StreamsApi } from '$lib/api/streamsApi';
  import type { ControlMeta, StreamInfo, StreamPipelineGridSlot, StreamPipelineWire } from '$lib/api/client';
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
  let tuneMetricsSort = $state<'desc' | 'asc'>('desc');
  let CameraControlsTabComponent = $state<(typeof import('$lib/features/devices/camera/CameraControlsTab.svelte'))['default'] | null>(null);
  let CameraPipelinesTabComponent = $state<(typeof import('$lib/features/devices/camera/CameraPipelinesEditorTab.svelte'))['default'] | null>(null);
  const tunePanelLoads: Partial<Record<'controls' | 'layout', Promise<void>>> = {};

  function loadTunePanelOnce(key: 'controls' | 'layout', loader: () => Promise<void>): Promise<void> {
    const inFlight = tunePanelLoads[key];
    if (inFlight) {
      return inFlight;
    }
    const next = loader().finally(() => {
      tunePanelLoads[key] = undefined;
    });
    tunePanelLoads[key] = next;
    return next;
  }

  async function loadCameraControlsTab(): Promise<void> {
    if (CameraControlsTabComponent) return;
    await loadTunePanelOnce('controls', async () => {
      const module = await import('$lib/features/devices/camera/CameraControlsTab.svelte');
      CameraControlsTabComponent = module.default;
    });
  }

  async function loadCameraPipelinesTab(): Promise<void> {
    if (CameraPipelinesTabComponent) return;
    await loadTunePanelOnce('layout', async () => {
      const module = await import('$lib/features/devices/camera/CameraPipelinesEditorTab.svelte');
      CameraPipelinesTabComponent = module.default;
    });
  }

  $effect(() => {
    if (tunePerformanceTab === 'controls') {
      void loadCameraControlsTab();
      return;
    }
    if (tunePerformanceTab === 'layout') {
      void loadCameraPipelinesTab();
    }
  });

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
    const wires = stream?.manifest?.pipeline_wires ?? [];
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
  const tunePreviewProps = $derived.by(() =>
    tunePreviewStream
      ? buildStreamPreviewProps(
          {
            name: streamLabel(tunePreviewStream),
            status: 'live',
            captureSessionId: tunePreviewStream.id ?? null,
            captureSessionAlias: streamLabel(tunePreviewStream),
            cameraUid: null,
            recordingActive: streamRecordingActive(tunePreviewStream)
          },
          'pipeline-tune'
        )
      : null
  );
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

  const inputsSectionState = $derived.by(() => ({
    get tuneUiEditMode() {
      return tuneUiEditMode;
    },
    set tuneUiEditMode(value: boolean) {
      tuneUiEditMode = value;
    },
    toggleTuneUiEditMode: () => {
      tuneUiEditMode = !tuneUiEditMode;
    },
    get tuneScopeTab() {
      return tuneScopeTab;
    },
    set tuneScopeTab(value: string) {
      tuneScopeTab = value;
    },
    setTuneScopeTab: (value: string) => {
      tuneScopeTab = value;
    },
    tuneStreamsForPipeline,
    streamLabel,
    onRequestAssign,
    tuneStreamsLoading,
    tuneStreamsError,
    selectedPipeline,
    tunePlan,
    showTuneInputSelector,
    tuneInputCurrentSelection,
    applyTuneInputSelection,
    tuneInputLayoutSlots,
    tuneInputTargetPipelineId,
    RAW_STREAM_PIPELINE_ID,
    pipelineLabelById,
    get tunePipelineUiSearch() {
      return tunePipelineUiSearch;
    },
    set tunePipelineUiSearch(value: string) {
      tunePipelineUiSearch = value;
    },
    get tuneUiMode() {
      return tuneUiMode;
    },
    get tunePipelineUiDraft() {
      return tunePipelineUiDraft;
    },
    set tunePipelineUiDraft(value) {
      tunePipelineUiDraft = value;
    },
    pipelineOutputOptions,
    tuneNodeDescriptors,
    tuneNodeErrors,
    readTuneNodeDraft,
    updateGlobalNodeValue,
    get tuneUiActiveTabId() {
      return tuneUiActiveTabId;
    },
    set tuneUiActiveTabId(value: string | null) {
      tuneUiActiveTabId = value;
    },
    get tuneUiSelectedItemId() {
      return tuneUiSelectedItemId;
    },
    set tuneUiSelectedItemId(value: string | null) {
      tuneUiSelectedItemId = value;
    },
    get tuneUiSelectedItemAnchor() {
      return tuneUiSelectedItemAnchor;
    },
    set tuneUiSelectedItemAnchor(value) {
      tuneUiSelectedItemAnchor = value;
    },
    get tuneConstantSearch() {
      return tuneConstantSearch;
    },
    set tuneConstantSearch(value: string) {
      tuneConstantSearch = value;
    },
    tuneConstantGroups,
    tuneFilteredConstantGroups,
    isDaedalusPlan,
    safeClonePlan,
    handlePlanChange,
    normalizePortKey,
    clearTuneNodeDraft,
    setTuneNodeError,
    scheduleTuneGlobalAutoSave,
    setNodeConstantValue,
    RAW_STREAM_PIPELINE_UUID,
    tuneStreamNodeOverridesById,
    tuneStreamNodeErrorsById,
    readTuneStreamNodeDraft,
    updateStreamNodeValue,
    tuneStreamApplyErrorById
  }));

  const inspectorSectionState = $derived.by(() => ({
    tunePreviewStream,
    tunePreviewProps,
    streamLabel,
    get tuneUiMode() {
      return tuneUiMode;
    },
    get tuneUiEditMode() {
      return tuneUiEditMode;
    },
    get tunePipelineUiDraft() {
      return tunePipelineUiDraft;
    },
    set tunePipelineUiDraft(value) {
      tunePipelineUiDraft = value;
    },
    saveTunePipelineUi,
    resetTunePipelineUi,
    get tunePerformanceTab() {
      return tunePerformanceTab;
    },
    set tunePerformanceTab(value: 'metrics' | 'controls' | 'layout' | 'outputs') {
      tunePerformanceTab = value;
    },
    metricsStatusLabel,
    metricsUpdatedLabel,
    metricsSource,
    tuneScopeTab,
    pipelineMetricsSummary,
    activeMetricsStream,
    activeMetricsSnapshot,
    activeNodeTimingRows,
    get tuneMetricsSort() {
      return tuneMetricsSort;
    },
    set tuneMetricsSort(value: 'asc' | 'desc') {
      tuneMetricsSort = value;
    },
    tuneControlsLoading,
    tuneControlsError,
    CameraControlsTabComponent,
    tuneStreamControls,
    get tuneControlsQuery() {
      return tuneControlsQuery;
    },
    set tuneControlsQuery(value: string) {
      tuneControlsQuery = value;
    },
    get tuneShowReadOnlyControls() {
      return tuneShowReadOnlyControls;
    },
    set tuneShowReadOnlyControls(value: boolean) {
      tuneShowReadOnlyControls = value;
    },
    get tuneControlState() {
      return tuneControlState;
    },
    set tuneControlState(value) {
      tuneControlState = value;
    },
    get tuneControlAppliedState() {
      return tuneControlAppliedState;
    },
    set tuneControlAppliedState(value) {
      tuneControlAppliedState = value;
    },
    get tuneControlBusy() {
      return tuneControlBusy;
    },
    set tuneControlBusy(value) {
      tuneControlBusy = value;
    },
    tuneFilteredControls,
    menuOptions,
    applyStreamControl,
    scheduleControlApply,
    displayControlValue,
    extractControlValue,
    controlMin,
    controlMax,
    controlStep,
    accessLabel,
    accessBadgeClass,
    outputTypesByPort,
    typePalette,
    CameraPipelinesTabComponent,
    tuneMultiplexError,
    tuneMultiplexPalettePipelineIds,
    RAW_STREAM_PIPELINE_ID,
    RAW_STREAM_PIPELINE_UUID,
    tuneMultiplexGridIsSingle,
    setTuneMultiplexGridDimensions,
    get tuneMultiplexRows() {
      return tuneMultiplexRows;
    },
    set tuneMultiplexRows(value: number) {
      tuneMultiplexRows = value;
    },
    get tuneMultiplexColumns() {
      return tuneMultiplexColumns;
    },
    set tuneMultiplexColumns(value: number) {
      tuneMultiplexColumns = value;
    },
    tuneMultiplexRowIndices,
    tuneMultiplexColumnIndices,
    tunePipelineForCell,
    tuneOutputSelectionForPipeline,
    tuneOutputKeyForCell,
    tuneLayoutWires,
    setTuneLayoutFrameSourceForPipelineInstance,
    tuneMultiplexOutputOptionsCache,
    tuneMultiplexLayoutSignature,
    tuneAllowDrop,
    dropTuneMultiplexOn,
    clearTuneMultiplexCell,
    get tuneSelectedPipelineOutput() {
      return tuneSelectedPipelineOutput;
    },
    set tuneSelectedPipelineOutput(value: string | null) {
      tuneSelectedPipelineOutput = value;
    },
    setTuneOutputSelectionForPipeline,
    setTuneOutputKeyForCell,
    setTuneLivePipelineOutput,
    get tunePipelineRemoveModalOpen() {
      return tunePipelineRemoveModalOpen;
    },
    set tunePipelineRemoveModalOpen(value: boolean) {
      tunePipelineRemoveModalOpen = value;
    },
    get tunePipelineRemoveCandidateId() {
      return tunePipelineRemoveCandidateId;
    },
    set tunePipelineRemoveCandidateId(value: string | null) {
      tunePipelineRemoveCandidateId = value;
    },
    get tunePipelineAssignModalOpen() {
      return tunePipelineAssignModalOpen;
    },
    set tunePipelineAssignModalOpen(value: boolean) {
      tunePipelineAssignModalOpen = value;
    },
    get tunePipelineAssignQuery() {
      return tunePipelineAssignQuery;
    },
    set tunePipelineAssignQuery(value: string) {
      tunePipelineAssignQuery = value;
    },
    get tunePipelineAssignDraft() {
      return tunePipelineAssignDraft;
    },
    set tunePipelineAssignDraft(value: string[]) {
      tunePipelineAssignDraft = value;
    },
    tunePipelineAssignFilteredGraphs,
    tunePipelineGraphs,
    scheduleTuneMultiplexAutoApply,
    startTuneMultiplexDrag,
    pipelineLabelById
  }));
</script>

<section class="flex min-h-0 flex-1 flex-col gap-6 overflow-hidden">
  <div class="flex min-h-0 flex-1 flex-col gap-6 overflow-hidden lg:flex-row">
    <PipelineTuneInputsSection state={inputsSectionState} />
    <PipelineTuneInspectorSection state={inspectorSectionState} />
  </div>
</section>
