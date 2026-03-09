<script lang="ts">

  import { onDestroy, onMount, untrack, type ComponentProps, type Snippet } from 'svelte';
  import type { PageData } from '../../../../../routes/devices/[cameraId]/$types';
  import { DeviceService, OpenAPI, getHttpClientBase } from '$lib/api/httpClient';
  import { PipelinesApi } from '$lib/api/pipelinesApi';
  import { fromApiGraphPlan } from '$lib/features/pipelines/model';
  import { serializeGraphPlan } from '$lib/features/pipelines/graph';
  import { buildDaedalusGraphPatch } from '$lib/features/pipelines/daedalusGraph';
  import { StreamsApi } from '$lib/api/streamsApi';
  import { connectStreamControls } from '$lib/api/streamControls';
  import { connectStreamUpdates } from '$lib/api/streamUpdates';
  import { resourceTelemetryStore } from '$lib/api/telemetry';
  import { reportError } from '$lib/ui/errorPolicy';
  import { invalidateSWR, invalidateSWRPrefix } from '$lib/utils/swrCache';
  import { PipelineIcon, StreamMetricsPanel, StreamPreview, toaster } from '$lib';
  import { DEFAULT_PIPELINE_UI } from '$lib/features/pipelines/pipelineUiTypes';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { faCamera } from '@fortawesome/free-solid-svg-icons';
  import { buildNodeValueFromInput, resolveDataTypeKey } from '$lib/features/pipelines/valueFormatting';
  import CameraPoseTab from '$lib/features/devices/camera/CameraPoseTab.svelte';
  import CameraStreamTab from '$lib/features/devices/camera/CameraStreamTab.svelte';
  import CameraControlsTab from '$lib/features/devices/camera/CameraControlsTab.svelte';
  import CameraMediaTab from '$lib/features/devices/camera/CameraMediaTab.svelte';
  import CameraPipelinesTab from '$lib/features/devices/camera/CameraPipelinesTab.svelte';
  import CameraCalibrationTab from '$lib/features/devices/camera/CameraCalibrationTab.svelte';
  import CameraPageView from './CameraPageView.svelte';
  import CalibrationGuidanceOverlay from '$lib/features/devices/camera/CalibrationGuidanceOverlay.svelte';
  import CameraHeader from '$lib/features/devices/camera/page/CameraHeader.svelte';
  import CameraStreamSidebar from '$lib/features/devices/camera/page/CameraStreamSidebar.svelte';
  import CameraPipelineOverrides from '$lib/features/devices/camera/page/CameraPipelineOverrides.svelte';
  import { createCameraStreamState } from './cameraStreamStore.svelte';
  import { createCameraPagePipelineRuntime } from './cameraPagePipelineRuntime.svelte';
  import { createCameraPageCalibrationRuntime } from './cameraPageCalibrationRuntime.svelte';
  import { createCameraStreamLifecycleController } from './cameraStreamLifecycleController';
  import { createCameraStreamPresetController } from './cameraStreamPresetController';
  import { createCameraStreamPresetHelpers } from './cameraStreamPresetHelpers';
  import { buildCameraPageConstants, buildCameraPageCore, buildCameraPageDerived, buildCameraPageUi } from './cameraPageUiBuilders';
  import { buildCameraTabs } from './cameraPageTabs';
  import { setupStreamViewerResize } from './cameraPageViewHelpers';
  import { createCameraPageControllers } from './cameraPageControllers';
  import {
    DEFAULT_LIBCAMERA_TARGET_FPS,
    PIPELINE_UI_METADATA_KEY,
    type CameraPageTabId
  } from './cameraPageStateTypes';
  import { backendLabel, buildApiPath, isTimeoutError, modeKey } from './cameraPageHelpers';
  import { resolvePoseCameraRef, normalizeCalibrationSolveResult, extractCurrentCalibrationParams, parseMetadataValue } from './cameraStateUtils';
  import { normalizeGridSlots, normalizeGridOutputKeys, layoutSignature, parseManifestLayout as parsePipelineManifestLayout } from './cameraPipelineState';
  import { fpsToFrameRate, frameRateToFps, gcd, intervalToFps, mediaFormatMatches, normalizeFpsLimit, normalizeRotationDegrees } from './cameraStreamState';
  import {
    PIPELINE_OUTPUT_CELL_KEY,
    PIPELINE_UI_STORAGE_PREFIX,
    RAW_LOOPBACK_GRAPH,
    RAW_PIPELINE_ID,
    RAW_PIPELINE_UUID,
    applyDaedalusNodeOverrides,
    extractNodeOverrides,
    extractNodeValueDescriptors,
    findPortMetadata,
    isDaedalusPlan,
    mergeNodeOverrides,
    normalizeAssignedPipelineIds,
    normalizeNodePortKey,
    normalizePipelineOutputMap,
    normalizePortMetadataMap,
    numberFromRegistryValue,
    pipelineDataTypeFromTypeExpr,
    registryPortMetadataFor,
    registryPortTypeFor,
    setRawPipelineUuid,
    safeCloneGraph
  } from './cameraPipelineTuningController';

  type CameraPageCtx = ComponentProps<typeof CameraPageView>['ctx'];

  const { data, children } = $props<{ data: PageData; children?: Snippet<[ { ctx: CameraPageCtx } ]> }>();
  const streamId = $derived.by(() => data.streamId);
  const asRecord = (value: unknown): Record<string, unknown> | null =>
    value && typeof value === 'object' ? (value as Record<string, unknown>) : null;
  const resolveApiBase = (): string => {
    try {
      return `${getHttpClientBase()}/v1`;
    } catch {
      const current = String(OpenAPI.BASE ?? '').replace(/\/+$/, '');
      return current.length ? current : '/v1';
    }
  };
  const apiPath = (path: string): string => buildApiPath(resolveApiBase(), path);
  const apiBase = resolveApiBase();
  type TabId = CameraPageTabId;
  type StreamCrop = [number, number, number, number];
  type StreamCrosshair = [number, number];
  type StreamOrderingMode =
    | 'none'
    | 'largest_to_smallest'
    | 'smallest_to_largest'
    | 'top_most'
    | 'bottom_most'
    | 'left_most'
    | 'right_most'
    | 'top_left'
    | 'top_right'
    | 'bottom_left'
    | 'bottom_right'
    | 'center_most'
    | 'crosshair';
  const streamState = createCameraStreamState();
  let calibrationModePipelineUuid = $state('');
  let rawPipelineUuid = $state(RAW_PIPELINE_UUID);
  let streamLookupDebug = $state<string | null>(null);
  let streamApiBase = $state<string | null>(null);
  let descriptor = streamState.stream?.descriptor ?? { modes: [], controls: [] };
  $effect(() => {
    descriptor = streamState.stream?.descriptor ?? { modes: [], controls: [] };
  });
  let currentCalibrationParams = extractCurrentCalibrationParams(streamState.manifestState, streamState.stream);
  $effect(() => {
    currentCalibrationParams = extractCurrentCalibrationParams(streamState.manifestState, streamState.stream);
  });
  let refreshFn = async () => {};
  let awaitStreamUpdatesSocketFn: (streamId: string, timeoutMs?: number) => Promise<unknown> = async () => null;
  let scheduleStreamPresetApplyFn = () => {};
  let inputUsageAuditKey = $state<string | null>(null);
  let inputUsageAuditTimer: ReturnType<typeof setTimeout> | null = null;
  const refresh = async () => {
    await refreshFn();
  };
  const awaitStreamUpdatesSocket = (id: string, timeoutMs?: number) => awaitStreamUpdatesSocketFn(id, timeoutMs);
  let dismissGuidedCalibrationOverlay = () => {};

  const normalizePipelineUuid = (value: unknown): string | null => {
    if (typeof value !== 'string') return null;
    const trimmed = value.trim().toLowerCase();
    return trimmed.length ? trimmed : null;
  };

  const isCalibrationPipeline = (value: unknown): boolean =>
    normalizePipelineUuid(value) === normalizePipelineUuid(calibrationModePipelineUuid);

  const clampCropValue = (value: number): number => {
    if (!Number.isFinite(value)) return 0;
    return Math.max(-1, Math.min(1, value));
  };

  const normalizeStreamCrop = (crop: StreamCrop): StreamCrop => {
    let [x0, x1, y0, y1] = crop;
    x0 = clampCropValue(x0);
    x1 = clampCropValue(x1);
    y0 = clampCropValue(y0);
    y1 = clampCropValue(y1);
    if (x1 < x0) [x0, x1] = [x1, x0];
    if (y1 < y0) [y0, y1] = [y1, y0];
    return [x0, x1, y0, y1];
  };

  const normalizeStreamCrosshair = (crosshair: StreamCrosshair): StreamCrosshair => {
    const x = clampCropValue(Number(crosshair[0] ?? 0));
    const y = clampCropValue(Number(crosshair[1] ?? 0));
    return [x, y];
  };

  const parseResolutionText = (value: unknown): { width: number; height: number } | null => {
    if (typeof value !== 'string') return null;
    const match = value.trim().match(/^(\d+)\s*[xX]\s*(\d+)$/);
    if (!match) return null;
    const width = Number.parseInt(match[1], 10);
    const height = Number.parseInt(match[2], 10);
    if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) return null;
    return { width, height };
  };

  const activeStreamResolution = (): { width: number; height: number } | null => {
    const candidates = [
      asRecord(asRecord(asRecord(asRecord(asRecord(streamState.stream)?.manifest)?.capture)?.mode)?.format)?.resolution,
      asRecord(asRecord(asRecord(asRecord(streamState.manifestState)?.capture)?.mode)?.format)?.resolution,
      parseResolutionText(streamState.selectedResolution)
    ];
    for (const candidate of candidates) {
      const resolution = asRecord(candidate);
      if (!resolution) continue;
      const width = Number(resolution.width ?? 0);
      const height = Number(resolution.height ?? 0);
      if (Number.isFinite(width) && Number.isFinite(height) && width > 0 && height > 0) {
        return { width, height };
      }
    }
    return null;
  };

  const crosshairToPixels = (crosshair: StreamCrosshair): { x: number; y: number } | null => {
    const resolution = activeStreamResolution();
    if (!resolution) return null;
    const width = Math.max(1, Math.trunc(resolution.width));
    const height = Math.max(1, Math.trunc(resolution.height));
    const xNorm = clampCropValue(Number(crosshair[0] ?? 0));
    const yNorm = clampCropValue(Number(crosshair[1] ?? 0));
    const x = Math.max(0, Math.min(width - 1, Math.round(((xNorm + 1) * 0.5) * width)));
    const y = Math.max(0, Math.min(height - 1, Math.round(((yNorm + 1) * 0.5) * height)));
    return { x, y };
  };

  const normalizeStreamOrderingMode = (mode: unknown): StreamOrderingMode => {
    const normalized = String(mode ?? 'none')
      .trim()
      .toLowerCase()
      .replaceAll('-', '_')
      .replaceAll(' ', '_');
    const allowed: StreamOrderingMode[] = [
      'none',
      'largest_to_smallest',
      'smallest_to_largest',
      'top_most',
      'bottom_most',
      'left_most',
      'right_most',
      'top_left',
      'top_right',
      'bottom_left',
      'bottom_right',
      'center_most',
      'crosshair',
    ];
    return (allowed as string[]).includes(normalized) ? (normalized as StreamOrderingMode) : 'none';
  };

  const guidedModeFromManifest = (manifest: unknown): boolean | null => {
    if (!manifest || typeof manifest !== 'object') return null;
    const record = manifest as Record<string, unknown>;
    if (
      isCalibrationPipeline(record.active_pipeline_id) ||
      isCalibrationPipeline(record.activePipelineId) ||
      isCalibrationPipeline(record.pipeline_id) ||
      isCalibrationPipeline(record.pipelineId)
    ) {
      return true;
    }
    const pipelines = record.pipelines;
    if (Array.isArray(pipelines)) {
      for (const entry of pipelines) {
        if (!entry || typeof entry !== 'object') continue;
        const pipelineRecord = entry as Record<string, unknown>;
        if (isCalibrationPipeline(pipelineRecord.pipeline_id ?? pipelineRecord.pipelineId ?? pipelineRecord.id)) {
          return true;
        }
      }
    }
    const layout = record.pipeline_layout ?? record.pipelineLayout;
    if (layout && typeof layout === 'object') {
      const slots = (layout as Record<string, unknown>).slots;
      if (Array.isArray(slots)) {
        for (const slot of slots) {
          if (!slot || typeof slot !== 'object') continue;
          const slotRecord = slot as Record<string, unknown>;
          if (isCalibrationPipeline(slotRecord.pipeline_id ?? slotRecord.pipelineId)) {
            return true;
          }
        }
      }
    }
    return false;
  };

  const pipelineRuntime = untrack(() =>
    createCameraPagePipelineRuntime({
      streamId,
      stream: () => streamState.stream,
      manifestState: () => streamState.manifestState,
      streamMetrics: () => streamState.streamMetrics,
      refresh: () => refreshFn(),
      scheduleStreamPresetApply: () => scheduleStreamPresetApplyFn(),
      onExternalLayoutApplied: () => dismissGuidedCalibrationOverlay(),
      awaitStreamUpdatesSocket: (id, timeoutMs) => awaitStreamUpdatesSocketFn(id, timeoutMs),
      apiPath,
      apiBase,
      PIPELINE_UI_METADATA_KEY,
      DEFAULT_PIPELINE_UI,
      RAW_PIPELINE_ID,
      RAW_LOOPBACK_GRAPH
    })
  );

  let {
    pipelineState,
    outputSelectionForPipeline,
    setOutputSelectionForPipeline,
    dropPipelineEverywhere,
    allowDrop,
    updatePipelineInputDraft,
    updatePipelineNodeDraft,
    updatePipelineNodeValue,
    applyPipelineOverridesToGraph
  } = pipelineRuntime;
  $effect(() => {
    ({
      pipelineState,
      outputSelectionForPipeline,
      setOutputSelectionForPipeline,
      dropPipelineEverywhere,
      allowDrop,
      updatePipelineInputDraft,
      updatePipelineNodeDraft,
      updatePipelineNodeValue,
      applyPipelineOverridesToGraph
    } = pipelineRuntime);
  });

  let closeControlSocket = () => {};
  let ensureControlSocket: (streamId: string) => void = () => {};
  let closeStreamUpdatesSocket = () => {};
  let ensureStreamUpdatesSocket: (streamId: string) => void = () => {};
  let stopStream = async () => {};

  const parseManifestLayout = (layout: unknown) =>
    parsePipelineManifestLayout(layout, { rawPipelineId: RAW_PIPELINE_ID, rawPipelineUuid });

  const pipelineStateBindings = {
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
    get pipelineOutputByPipelineId() {
      return pipelineState.pipelineOutputByPipelineId;
    },
    set pipelineOutputByPipelineId(value) {
      pipelineState.pipelineOutputByPipelineId = value;
    },
    get selectedPipelineId() {
      return pipelineState.selectedPipelineId;
    },
    set selectedPipelineId(value) {
      pipelineState.selectedPipelineId = value;
    },
    get selectedPipelineOutput() {
      return pipelineState.selectedPipelineOutput;
    },
    set selectedPipelineOutput(value) {
      pipelineState.selectedPipelineOutput = value;
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
    }
  };
  const streamBindings = $state({
    get activeTab() {
      return streamState.activeTab;
    },
    set activeTab(value) {
      streamState.activeTab = value;
    },
    get cameraAlias() {
      return streamState.cameraAlias;
    },
    set cameraAlias(value) {
      streamState.cameraAlias = value;
    },
    get selectedBackendIndex() {
      return streamState.selectedBackendIndex;
    },
    set selectedBackendIndex(value) {
      streamState.selectedBackendIndex = value;
    },
    get selectedFormat() {
      return streamState.selectedFormat;
    },
    set selectedFormat(value) {
      streamState.selectedFormat = value;
    },
    get selectedResolution() {
      return streamState.selectedResolution;
    },
    set selectedResolution(value) {
      streamState.selectedResolution = value;
    },
    get selectedIntervalIdx() {
      return streamState.selectedIntervalIdx;
    },
    set selectedIntervalIdx(value) {
      streamState.selectedIntervalIdx = value;
    },
    get selectedInterval() {
      return streamState.selectedInterval;
    },
    set selectedInterval(value) {
      streamState.selectedInterval = value;
    },
    get libcameraTargetFps() {
      return streamState.libcameraTargetFps;
    },
    set libcameraTargetFps(value) {
      streamState.libcameraTargetFps = value;
    },
    get netcamTargetFps() {
      return streamState.netcamTargetFps;
    },
    set netcamTargetFps(value) {
      streamState.netcamTargetFps = value;
    },
    get fileBackendFps() {
      return streamState.fileBackendFps;
    },
    set fileBackendFps(value) {
      streamState.fileBackendFps = value;
    },
    get fileBackendLoop() {
      return streamState.fileBackendLoop;
    },
    set fileBackendLoop(value) {
      streamState.fileBackendLoop = value;
    },
    get fileBackendPathsText() {
      return streamState.fileBackendPathsText;
    },
    set fileBackendPathsText(value) {
      streamState.fileBackendPathsText = value;
    },
    get decoderImpl() {
      return streamState.decoderImpl;
    },
    set decoderImpl(value) {
      streamState.decoderImpl = value;
    },
    get encoderImpl() {
      return streamState.encoderImpl;
    },
    set encoderImpl(value) {
      streamState.encoderImpl = value;
    },
    get decoderEnabled() {
      return streamState.decoderEnabled;
    },
    set decoderEnabled(value) {
      streamState.decoderEnabled = value;
    },
    get encoderEnabled() {
      return streamState.encoderEnabled;
    },
    set encoderEnabled(value) {
      streamState.encoderEnabled = value;
    },
    get decoderSelectionTouched() {
      return streamState.decoderSelectionTouched;
    },
    set decoderSelectionTouched(value) {
      streamState.decoderSelectionTouched = value;
    },
    get encoderSelectionTouched() {
      return streamState.encoderSelectionTouched;
    },
    set encoderSelectionTouched(value) {
      streamState.encoderSelectionTouched = value;
    },
    get hostBuffer() {
      return streamState.hostBuffer;
    },
    set hostBuffer(value) {
      streamState.hostBuffer = value;
    },
    get decoderFpsLimit() {
      return streamState.decoderFpsLimit;
    },
    set decoderFpsLimit(value) {
      streamState.decoderFpsLimit = value;
    },
    get decoderRotationDegrees() {
      return streamState.decoderRotationDegrees;
    },
    set decoderRotationDegrees(value) {
      streamState.decoderRotationDegrees = value;
    },
    get decoderMirrorHorizontal() {
      return streamState.decoderMirrorHorizontal;
    },
    set decoderMirrorHorizontal(value) {
      streamState.decoderMirrorHorizontal = value;
    },
    get shadowRecorderEnabled() {
      return streamState.shadowRecorderEnabled;
    },
    set shadowRecorderEnabled(value) {
      streamState.shadowRecorderEnabled = value;
    },
    get encoderFpsLimit() {
      return streamState.encoderFpsLimit;
    },
    set encoderFpsLimit(value) {
      streamState.encoderFpsLimit = value;
    },
    get encoderSettingsOpen() {
      return streamState.encoderSettingsOpen;
    },
    set encoderSettingsOpen(value) {
      streamState.encoderSettingsOpen = value;
    },
    get encoderSettings() {
      return streamState.encoderSettings;
    },
    set encoderSettings(value) {
      streamState.encoderSettings = value;
    },
    get streamCrop() {
      return streamState.streamCrop;
    },
    set streamCrop(value) {
      streamState.streamCrop = value;
    },
    get streamCropGuidesEnabled() {
      return streamState.streamCropGuidesEnabled;
    },
    set streamCropGuidesEnabled(value) {
      streamState.streamCropGuidesEnabled = value;
    },
    get streamCropApplying() {
      return streamState.streamCropApplying;
    },
    set streamCropApplying(value) {
      streamState.streamCropApplying = value;
    },
    get streamCropError() {
      return streamState.streamCropError;
    },
    set streamCropError(value) {
      streamState.streamCropError = value;
    },
    get streamCropWarning() {
      return streamState.streamCropWarning;
    },
    set streamCropWarning(value) {
      streamState.streamCropWarning = value;
    },
    get streamCrosshair() {
      return streamState.streamCrosshair;
    },
    set streamCrosshair(value) {
      streamState.streamCrosshair = value;
    },
    get streamCrosshairEnabled() {
      return streamState.streamCrosshairEnabled;
    },
    set streamCrosshairEnabled(value) {
      streamState.streamCrosshairEnabled = Boolean(value);
    },
    get streamCrosshairGuidesEnabled() {
      return streamState.streamCrosshairGuidesEnabled;
    },
    set streamCrosshairGuidesEnabled(value) {
      streamState.streamCrosshairGuidesEnabled = value;
    },
    get streamCrosshairApplying() {
      return streamState.streamCrosshairApplying;
    },
    set streamCrosshairApplying(value) {
      streamState.streamCrosshairApplying = value;
    },
    get streamCrosshairError() {
      return streamState.streamCrosshairError;
    },
    set streamCrosshairError(value) {
      streamState.streamCrosshairError = value;
    },
    get streamCrosshairWarning() {
      return streamState.streamCrosshairWarning;
    },
    set streamCrosshairWarning(value) {
      streamState.streamCrosshairWarning = value;
    },
    get streamOrderingMode() {
      return streamState.streamOrderingMode;
    },
    set streamOrderingMode(value) {
      streamState.streamOrderingMode = normalizeStreamOrderingMode(value);
    },
    get streamOrderingApplying() {
      return streamState.streamOrderingApplying;
    },
    set streamOrderingApplying(value) {
      streamState.streamOrderingApplying = value;
    },
    get streamOrderingError() {
      return streamState.streamOrderingError;
    },
    set streamOrderingError(value) {
      streamState.streamOrderingError = value;
    },
    get streamOrderingWarning() {
      return streamState.streamOrderingWarning;
    },
    set streamOrderingWarning(value) {
      streamState.streamOrderingWarning = value;
    },
    get controlsQuery() {
      return streamState.controlsQuery;
    },
    set controlsQuery(value) {
      streamState.controlsQuery = value;
    },
    get controlState() {
      return streamState.controlState;
    },
    set controlState(value) {
      streamState.controlState = value;
    },
    get controlAppliedState() {
      return streamState.controlAppliedState;
    },
    set controlAppliedState(value) {
      streamState.controlAppliedState = value;
    },
    get controlBusy() {
      return streamState.controlBusy;
    },
    set controlBusy(value) {
      streamState.controlBusy = value;
    },
    get streamMetrics() {
      return streamState.streamMetrics;
    },
    set streamMetrics(value) {
      streamState.streamMetrics = value;
    }
  });
  const pipelineBindings = $state({
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
    get selectedPipelineOutput() {
      return pipelineState.selectedPipelineOutput;
    },
    set selectedPipelineOutput(value) {
      pipelineState.selectedPipelineOutput = value;
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
    get pipelineAssignModalOpen() {
      return pipelineState.pipelineAssignModalOpen;
    },
    set pipelineAssignModalOpen(value) {
      pipelineState.pipelineAssignModalOpen = value;
    },
    get pipelineAssignQuery() {
      return pipelineState.pipelineAssignQuery;
    },
    set pipelineAssignQuery(value) {
      pipelineState.pipelineAssignQuery = value;
    },
    get pipelineAssignDraft() {
      return pipelineState.pipelineAssignDraft;
    },
    set pipelineAssignDraft(value) {
      pipelineState.pipelineAssignDraft = value;
    }
  });

  const { modeController, controlController, backendController } = untrack(() =>
    createCameraPageControllers({
      streamState,
      streamId,
      StreamsApi,
      toaster,
      reportError,
      modeKey,
      mediaFormatMatches,
      intervalToFps,
      frameRateToFps,
      normalizeFpsLimit,
      normalizeRotationDegrees,
      parseManifestLayout,
      outputSelectionForPipeline,
      setOutputSelectionForPipeline,
      DEFAULT_LIBCAMERA_TARGET_FPS,
      pipelineState: pipelineStateBindings
    })
  );

  const {
    effectiveModes,
    modeFormat,
    modeResolution,
    colorLabel,
    modeLabel,
    resolutionKey,
    uniqueFormats,
    resolutionsForFormat,
    intervalsForSelection,
    fpsLabel,
    firstFormat,
    firstResolution,
    firstInterval,
    decodersForCaptureFormat,
    currentMode,
    syncModeSelection
  } = modeController;
  let streamViewerHost = $state<HTMLDivElement | null>(null);
  let streamViewerBounds = $state({ width: 0, height: 0 });

  const calibrationRuntime = untrack(() =>
    createCameraPageCalibrationRuntime({
      streamId,
      stream: () => streamState.stream,
      apiPath,
      refresh
    })
  );

  const {
    calibrationState,
    mediaTabReady,
    refreshIpaStatus,
    refreshCalibrationImages,
    refreshCalibrationImportSources,
  } = calibrationRuntime;

  let lastGuidedModeFromManifest: boolean | null = null;
  $effect(() => {
    const manifest = streamState.manifestState ?? asRecord(streamState.stream)?.manifest ?? null;
    const manifestGuided = guidedModeFromManifest(manifest);
    if (manifestGuided === null || manifestGuided === lastGuidedModeFromManifest) return;
    const wasGuided = lastGuidedModeFromManifest === true;
    lastGuidedModeFromManifest = manifestGuided;
    calibrationState.calibrationGuidedMode = manifestGuided;
    if (!manifestGuided) {
      calibrationState.calibrationGuidedBusy = false;
      calibrationState.calibrationGuidedAccumulateLive = false;
      if (wasGuided) calibrationState.calibrationGuidedResetToken += 1;
    }
  });

  dismissGuidedCalibrationOverlay = () => {
    if (!calibrationState.calibrationGuidedMode && !calibrationState.calibrationGuidedAccumulateLive) return;
    calibrationState.calibrationGuidedBusy = false;
    calibrationState.calibrationGuidedAccumulateLive = false;
    calibrationState.calibrationGuidedResetToken += 1;
  };

  $effect(() => {
    const model = currentCalibrationParams?.lensModel;
    if (model === 'pinhole' || model === 'fisheye') {
      calibrationState.calibrationLensModel = model;
    }
  });

  const encoderSettingsAvailable = $derived(
    Boolean(
      streamState.encoderEnabled &&
        streamState.encoderImpl &&
        streamState.encoders.find((c) => c.implementation === streamState.encoderImpl || c.name === streamState.encoderImpl)
          ?.tunables?.encoder_settings
    )
  );

  $effect(() => {
    if (!encoderSettingsAvailable) streamState.encoderSettingsOpen = false;
  });

  const tabs: Array<{ id: TabId; label: string; ready: boolean }> = buildCameraTabs(mediaTabReady);

  const loadStreamCapabilities = async (): Promise<void> => {
    try {
      const capabilities = await StreamsApi.streamCapabilities();
      const rawId = normalizePipelineUuid(capabilities?.rawPipelineId);
      if (rawId) {
        rawPipelineUuid = setRawPipelineUuid(rawId);
      }
      const calibrationId = normalizePipelineUuid(capabilities?.calibrationModePipelineId);
      if (calibrationId) {
        calibrationModePipelineUuid = calibrationId;
      }
    } catch {
      // Keep previously loaded IDs; avoid local hardcoded fallback IDs.
    }
  };

  onMount(() => {
    void loadStreamCapabilities();
    return setupStreamViewerResize(streamViewerHost, (bounds) => {
      streamViewerBounds = bounds;
    });
  });

  const {
    timeoutApplyHint,
    downloadManifest,
    seedControlState,
    extractValue,
    controlStep,
    displayValue,
    accessLabel,
    accessBadgeClass,
    filteredControls,
    isControlChanged,
    buildControlValue,
    menuOptions,
    menuValueDisplay,
    controlMin,
    controlMax,
    clampNumber,
    clampControlValue,
    scheduleControlApply,
    applyControl
  } = controlController;

  const {
    isFileBackend,
    isNetcamBackend,
    extractFileHandle,
    buildNetcamBackend,
    buildFileBackend,
    loadBackends,
    loadCodecs,
    dedupeCodecs,
    pickCodecId,
    applyManifestSelections,
    currentDevice,
    currentBackend
  } = backendController;

  const streamLifecycleController = createCameraStreamLifecycleController(
    {
      get stream() {
        return streamState.stream;
      },
      set stream(value) {
        streamState.stream = value;
      },
      get streamId() {
        return streamId;
      },
      get manifestState() {
        return streamState.manifestState;
      },
      set manifestState(value) {
        streamState.manifestState = value;
      },
      get loading() {
        return streamState.loading;
      },
      set loading(value) {
        streamState.loading = value;
      },
      get refreshing() {
        return streamState.refreshing;
      },
      set refreshing(value) {
        streamState.refreshing = value;
      },
      get stopping() {
        return streamState.stopping;
      },
      set stopping(value) {
        streamState.stopping = value;
      },
      get error() {
        return streamState.error;
      },
      set error(value) {
        streamState.error = value;
      },
      get streamLookupDebug() {
        return streamLookupDebug;
      },
      set streamLookupDebug(value) {
        streamLookupDebug = value;
      },
      get streamApiBase() {
        return streamApiBase;
      },
      set streamApiBase(value) {
        streamApiBase = value;
      },
      get controls() {
        return streamState.controls;
      },
      set controls(value) {
        streamState.controls = value;
      },
      get controlState() {
        return streamState.controlState;
      },
      set controlState(value) {
        streamState.controlState = value;
      },
      get controlAppliedState() {
        return streamState.controlAppliedState;
      },
      set controlAppliedState(value) {
        streamState.controlAppliedState = value;
      },
      get controlSocket() {
        return streamState.controlSocket;
      },
      set controlSocket(value) {
        streamState.controlSocket = value;
      },
      get controlSocketStreamId() {
        return streamState.controlSocketStreamId;
      },
      set controlSocketStreamId(value) {
        streamState.controlSocketStreamId = value;
      },
      get streamUpdatesSocket() {
        return streamState.streamUpdatesSocket;
      },
      set streamUpdatesSocket(value) {
        streamState.streamUpdatesSocket = value;
      },
      get streamUpdatesSocketStreamId() {
        return streamState.streamUpdatesSocketStreamId;
      },
      set streamUpdatesSocketStreamId(value) {
        streamState.streamUpdatesSocketStreamId = value;
      }
    },
    {
      streamsApi: StreamsApi,
      deviceService: DeviceService,
      getHttpClientBase,
      toaster,
      reportError,
      loadBackends,
      loadCodecs,
      applyManifestSelections,
      refreshCalibrationImages,
      refreshCalibrationImportSources,
      refreshIpaStatus,
      seedControlState
    }
  );

  ({
    refresh: refreshFn,
    closeControlSocket,
    ensureControlSocket,
    closeStreamUpdatesSocket,
    ensureStreamUpdatesSocket,
    awaitStreamUpdatesSocket: awaitStreamUpdatesSocketFn,
    stopStream
  } = streamLifecycleController);

  const activeMode = $derived(
    (() =>
      streamState.stream?.descriptor?.modes?.find((m) => modeKey(m?.id) === streamState.selectedModeKey) ??
      streamState.stream?.descriptor?.modes?.[0] ??
      null)()
  );

  const streamViewerAspect = $derived(
    (() => {
      const res = activeMode?.format?.resolution ?? null;
      const w = Number(res?.width ?? 0);
      const h = Number(res?.height ?? 0);
      return w > 0 && h > 0 ? w / h : 16 / 9;
    })()
  );

  const streamViewerFit = $derived(
    (() => {
      const W = Math.max(0, Number(streamViewerBounds.width ?? 0));
      const H = Math.max(0, Number(streamViewerBounds.height ?? 0));
      const aspect = Number(streamViewerAspect);
      if (!W || !H || !Number.isFinite(aspect) || aspect <= 0) return { width: 0, height: 0 };

      let width = W;
      let height = width / aspect;
      if (height > H) {
        height = H;
        width = height * aspect;
      }
      return { width: Math.floor(width), height: Math.floor(height) };
    })()
  );



  $effect(() => {
    void pipelineState.pipelineGridRows;
    void pipelineState.pipelineGridColumns;
    void pipelineState.pipelineGridSlots;
    void pipelineState.assignedPipelineIds;
    void pipelineState.selectedPipelineId;
    void pipelineState.pipelineOutputByPipelineId;
    void streamState.encoders;
    void streamState.decoders;
    void streamState.selectedFormat;

    const rows = Math.min(Math.max(Math.trunc(pipelineState.pipelineGridRows), 1), 6);
    const columns = Math.min(Math.max(Math.trunc(pipelineState.pipelineGridColumns), 1), 6);
    const multiplex = rows * columns > 1;
    const hasPipelines =
      Boolean(pipelineState.selectedPipelineId) ||
      (pipelineState.assignedPipelineIds?.length ?? 0) > 0 ||
      Object.values(pipelineState.pipelineGridSlots ?? {}).some(Boolean);
    if (!multiplex && !hasPipelines) {
      return;
    }

    if (!streamState.encoderSelectionTouched && streamState.encoders.length) {
      const preferred = pickCodecId(streamState.encoders, streamState.encoderImpl, ['mjpeg']);
      if (preferred) {
        if (streamState.encoderImpl !== preferred) {
          streamState.encoderImpl = preferred;
        }
      }
    }

    if (!streamState.decoderSelectionTouched) {
      const compatible = decodersForCaptureFormat(streamState.selectedFormat);
      const preferred = pickCodecId(
        compatible.length ? compatible : streamState.decoders,
        streamState.decoderImpl,
        ['mono8-replicate']
      );
      if (preferred) {
        if (streamState.decoderImpl !== preferred) {
          streamState.decoderImpl = preferred;
        }
      }
    }
  });




























  
  const streamPresetController = createCameraStreamPresetController(
    {
      get streamId() {
        return streamId;
      },
      get stream() {
        return streamState.stream;
      },
      get applying() {
        return streamState.applying;
      },
      set applying(value) {
        streamState.applying = value;
      },
      get pendingStreamPresetApply() {
        return streamState.pendingStreamPresetApply;
      },
      set pendingStreamPresetApply(value) {
        streamState.pendingStreamPresetApply = value;
      },
      get pendingStreamPresetSilent() {
        return streamState.pendingStreamPresetSilent;
      },
      set pendingStreamPresetSilent(value) {
        streamState.pendingStreamPresetSilent = value;
      },
      get pipelineGridRows() {
        return pipelineState.pipelineGridRows;
      },
      get pipelineGridColumns() {
        return pipelineState.pipelineGridColumns;
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
      get assignedPipelineIds() {
        return pipelineState.assignedPipelineIds;
      },
      set assignedPipelineIds(value) {
        pipelineState.assignedPipelineIds = value;
      },
      get selectedPipelineId() {
        return pipelineState.selectedPipelineId;
      },
      set selectedPipelineId(value) {
        pipelineState.selectedPipelineId = value;
      },
      get selectedPipelineOutput() {
        return pipelineState.selectedPipelineOutput;
      },
      set selectedPipelineOutput(value) {
        pipelineState.selectedPipelineOutput = value;
      },
      get cameraAlias() {
        return streamState.cameraAlias;
      },
      get encoderImpl() {
        return streamState.encoderImpl;
      },
      get decoderImpl() {
        return streamState.decoderImpl;
      },
      get decoders() {
        return streamState.decoders;
      },
      get encoderEnabled() {
        return streamState.encoderEnabled;
      },
      get decoderEnabled() {
        return streamState.decoderEnabled;
      },
      get encoderSettings() {
        return streamState.encoderSettings;
      },
      get encoderSelectionTouched() {
        return streamState.encoderSelectionTouched;
      },
      get encoderFpsLimit() {
        return streamState.encoderFpsLimit;
      },
      get decoderFpsLimit() {
        return streamState.decoderFpsLimit;
      },
      get decoderRotationDegrees() {
        return streamState.decoderRotationDegrees;
      },
      get decoderMirrorHorizontal() {
        return streamState.decoderMirrorHorizontal;
      },
      get shadowRecorderEnabled() {
        return streamState.shadowRecorderEnabled;
      },
      get libcameraTargetFps() {
        return streamState.libcameraTargetFps;
      },
      get netcamTargetFps() {
        return streamState.netcamTargetFps;
      },
      get fileBackendFps() {
        return streamState.fileBackendFps;
      },
      get fileBackendLoop() {
        return streamState.fileBackendLoop;
      },
      get fileBackendPathsText() {
        return streamState.fileBackendPathsText;
      },
      get hostBuffer() {
        return streamState.hostBuffer;
      },
      get selectedIntervalIdx() {
        return streamState.selectedIntervalIdx;
      },
      get selectedModeKey() {
        return streamState.selectedModeKey;
      },
      get selectedFormat() {
        return streamState.selectedFormat;
      },
      get selectedResolution() {
        return streamState.selectedResolution;
      },
      get streamPresetApplyTimer() {
        return streamState.streamPresetApplyTimer;
      },
      set streamPresetApplyTimer(value) {
        streamState.streamPresetApplyTimer = value;
      }
    },
    {
      pipelinesApi: PipelinesApi,
      streamsApi: StreamsApi,
      apiBase,
      toaster,
      STREAM_PRESET_DEBOUNCE_MS: streamState.STREAM_PRESET_DEBOUNCE_MS,
      currentBackend,
      currentDevice,
      currentMode,
      intervalsForSelection,
      decodersForCaptureFormat,
      pickCodecId,
      outputSelectionForPipeline,
      applyPipelineOverridesToGraph,
      dropPipelineEverywhere,
      reportError,
      onExternalLayoutApplied: () => dismissGuidedCalibrationOverlay()
    }
  );

  const { applyStreamPreset } = streamPresetController;
  const scheduleStreamPresetApplyRaw = streamPresetController.scheduleStreamPresetApply;

  const { scheduleStreamPresetApply } = createCameraStreamPresetHelpers({
    getPipelineUiHydrated: () => pipelineState.pipelineUiHydrated,
    scheduleStreamPresetApplyRaw
  });
  scheduleStreamPresetApplyFn = scheduleStreamPresetApply;

  async function applyStreamCrop(crop: StreamCrop): Promise<void> {
    const effectiveId = streamState.stream?.id ?? streamId;
    if (!effectiveId) return;

    const normalized = normalizeStreamCrop(crop);
    streamState.streamCrop = normalized;
    streamState.streamCropError = null;
    streamState.streamCropWarning = null;
    streamState.streamCropApplying = true;

    try {
      const response = await fetch(apiPath(`/streams/${encodeURIComponent(effectiveId)}/crop`), {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ crop: normalized })
      });
      if (!response.ok) {
        const text = await response.text().catch(() => '');
        throw new Error(text || `HTTP ${response.status}`);
      }
      const payload = (await response.json().catch(() => null)) as { crop?: number[]; warnings?: string[]; roi_inputs_used?: boolean } | null;
      if (Array.isArray(payload?.crop) && payload.crop.length === 4) {
        streamState.streamCrop = normalizeStreamCrop([
          Number(payload.crop[0] ?? normalized[0]),
          Number(payload.crop[1] ?? normalized[1]),
          Number(payload.crop[2] ?? normalized[2]),
          Number(payload.crop[3] ?? normalized[3])
        ]);
      }
      const warnings =
        Array.isArray(payload?.warnings) && payload?.warnings
          ? payload.warnings.map((value) => String(value).trim()).filter((value) => value.length > 0)
          : [];
      if (warnings.length > 0) {
        streamState.streamCropWarning = warnings[0];
      } else if (payload?.roi_inputs_used === false) {
        streamState.streamCropWarning = 'ROI inputs are not consumed by the active graph.';
      } else {
        streamState.streamCropWarning = null;
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Crop update failed.';
      streamState.streamCropError = message;
      streamState.streamCropWarning = null;
      console.error('Failed to apply stream crop', error);
    } finally {
      streamState.streamCropApplying = false;
    }
  }

  function normalizeWarningList(raw: unknown): string[] {
    if (!Array.isArray(raw)) return [];
    return raw.map((value) => String(value).trim()).filter((value) => value.length > 0);
  }

  async function refreshStreamInputUsageWarnings(): Promise<void> {
    const effectiveId = streamState.stream?.id ?? streamId;
    if (!effectiveId) return;
    try {
      const response = await fetch(apiPath(`/streams/${encodeURIComponent(effectiveId)}/pipeline/input-usage`), {
        method: 'GET',
        headers: { accept: 'application/json' }
      });
      if (!response.ok) {
        if (response.status === 404) return;
        const text = await response.text().catch(() => '');
        throw new Error(text || `HTTP ${response.status}`);
      }
      const payload = (await response.json().catch(() => null)) as {
        roi_warnings?: string[];
        crosshair_warnings?: string[];
        ordering_warnings?: string[];
        roi_inputs_used?: boolean;
        crosshair_inputs_used?: boolean;
        ordering_inputs_used?: boolean;
      } | null;

      const roiWarnings = normalizeWarningList(payload?.roi_warnings);
      if (roiWarnings.length > 0) {
        streamState.streamCropWarning = roiWarnings[0];
      } else if (payload?.roi_inputs_used === false) {
        streamState.streamCropWarning = 'ROI inputs are not consumed by the active graph.';
      } else {
        streamState.streamCropWarning = null;
      }

      const crosshairWarnings = normalizeWarningList(payload?.crosshair_warnings);
      if (crosshairWarnings.length > 0) {
        streamState.streamCrosshairWarning = crosshairWarnings[0];
      } else if (payload?.crosshair_inputs_used === false) {
        streamState.streamCrosshairWarning = 'Crosshair inputs are not consumed by the active graph.';
      } else {
        streamState.streamCrosshairWarning = null;
      }

      const orderingWarnings = normalizeWarningList(payload?.ordering_warnings);
      if (orderingWarnings.length > 0) {
        streamState.streamOrderingWarning = orderingWarnings[0];
      } else if (payload?.ordering_inputs_used === false) {
        streamState.streamOrderingWarning = 'Ordering input is not consumed by the active graph.';
      } else {
        streamState.streamOrderingWarning = null;
      }
    } catch (error) {
      console.warn('Failed to refresh stream input usage warnings', error);
    }
  }

  async function applyStreamCrosshair(crosshair: StreamCrosshair, enabled: boolean = streamState.streamCrosshairEnabled): Promise<void> {
    const effectiveId = streamState.stream?.id ?? streamId;
    if (!effectiveId) return;

    const normalized = normalizeStreamCrosshair(crosshair);
    const normalizedEnabled = Boolean(enabled);
    streamState.streamCrosshair = normalized;
    streamState.streamCrosshairEnabled = normalizedEnabled;
    streamState.streamCrosshairError = null;
    streamState.streamCrosshairWarning = null;
    streamState.streamCrosshairApplying = true;

    try {
      const response = await fetch(apiPath(`/streams/${encodeURIComponent(effectiveId)}/crosshair`), {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ crosshair: normalized, enabled: normalizedEnabled })
      });
      if (!response.ok) {
        if (response.status === 404) {
          const px = crosshairToPixels(normalized);
          if (px) {
            const fallbackResponse = await fetch(apiPath(`/streams/${encodeURIComponent(effectiveId)}/pipeline/inputs`), {
              method: 'POST',
              headers: { 'content-type': 'application/json' },
              body: JSON.stringify({
                inputs: {
                  crosshair_x: px.x,
                  crosshair_y: px.y,
                  draw_crosshair: normalizedEnabled
                }
              })
            });
            if (fallbackResponse.ok) {
              streamState.streamCrosshairWarning = 'Using compatibility crosshair fallback on this device backend.';
              return;
            }
          }
        }
        const text = await response.text().catch(() => '');
        throw new Error(text || `HTTP ${response.status}`);
      }
      const payload = (await response.json().catch(() => null)) as {
        crosshair?: number[];
        enabled?: boolean;
        warnings?: string[];
        crosshair_inputs_used?: boolean;
      } | null;
      if (Array.isArray(payload?.crosshair) && payload.crosshair.length === 2) {
        streamState.streamCrosshair = normalizeStreamCrosshair([
          Number(payload.crosshair[0] ?? normalized[0]),
          Number(payload.crosshair[1] ?? normalized[1])
        ]);
      }
      if (typeof payload?.enabled === 'boolean') {
        streamState.streamCrosshairEnabled = payload.enabled;
      }
      const warnings =
        Array.isArray(payload?.warnings) && payload?.warnings
          ? payload.warnings.map((value) => String(value).trim()).filter((value) => value.length > 0)
          : [];
      if (warnings.length > 0) {
        streamState.streamCrosshairWarning = warnings[0];
      } else if (payload?.crosshair_inputs_used === false) {
        streamState.streamCrosshairWarning = 'Crosshair inputs are not consumed by the active graph.';
      } else {
        streamState.streamCrosshairWarning = null;
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Crosshair update failed.';
      streamState.streamCrosshairError = message;
      streamState.streamCrosshairWarning = null;
      console.error('Failed to apply stream crosshair', error);
    } finally {
      streamState.streamCrosshairApplying = false;
    }
  }

  async function applyStreamOrdering(mode: StreamOrderingMode): Promise<void> {
    const effectiveId = streamState.stream?.id ?? streamId;
    if (!effectiveId) return;

    const normalized = normalizeStreamOrderingMode(mode);
    streamState.streamOrderingMode = normalized;
    streamState.streamOrderingError = null;
    streamState.streamOrderingWarning = null;
    streamState.streamOrderingApplying = true;

    try {
      const response = await fetch(apiPath(`/streams/${encodeURIComponent(effectiveId)}/ordering`), {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ mode: normalized })
      });
      if (!response.ok) {
        const text = await response.text().catch(() => '');
        throw new Error(text || `HTTP ${response.status}`);
      }
      const payload = (await response.json().catch(() => null)) as {
        mode?: string;
        warnings?: string[];
        ordering_inputs_used?: boolean;
      } | null;
      if (typeof payload?.mode === 'string') {
        streamState.streamOrderingMode = normalizeStreamOrderingMode(payload.mode);
      }
      const warnings =
        Array.isArray(payload?.warnings) && payload?.warnings
          ? payload.warnings.map((value) => String(value).trim()).filter((value) => value.length > 0)
          : [];
      if (warnings.length > 0) {
        streamState.streamOrderingWarning = warnings[0];
      } else if (payload?.ordering_inputs_used === false) {
        streamState.streamOrderingWarning = 'Ordering input is not consumed by the active graph.';
      } else {
        streamState.streamOrderingWarning = null;
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Ordering update failed.';
      streamState.streamOrderingError = message;
      streamState.streamOrderingWarning = null;
      console.error('Failed to apply stream ordering', error);
    } finally {
      streamState.streamOrderingApplying = false;
    }
  }

  $effect(() => {
    const effectiveId = streamState.stream?.id ?? streamId;
    const manifest = streamState.manifestState ?? asRecord(streamState.stream)?.manifest ?? null;
    const manifestRecord = asRecord(manifest);
    const activePipelineId = String(manifestRecord?.active_pipeline_id ?? '');
    const pipelines = Array.isArray(manifestRecord?.pipelines) ? manifestRecord.pipelines : [];
    const pipelineSignature = pipelines
      .map((binding) => {
        const bindingRecord = asRecord(binding);
        const id = String(bindingRecord?.pipeline_id ?? '');
        const hasPatch = bindingRecord?.pipeline_patch ? '1' : '0';
        const hasInlineGraph = bindingRecord?.pipeline_graph ? '1' : '0';
        const output = String(bindingRecord?.pipeline_output ?? '');
        return `${id}:${hasPatch}:${hasInlineGraph}:${output}`;
      })
      .join('|');

    if (!effectiveId) {
      inputUsageAuditKey = null;
      if (inputUsageAuditTimer !== null) {
        clearTimeout(inputUsageAuditTimer);
        inputUsageAuditTimer = null;
      }
      return;
    }

    const nextKey = `${effectiveId}|${activePipelineId}|${pipelineSignature}`;
    if (nextKey === inputUsageAuditKey) return;
    inputUsageAuditKey = nextKey;

    if (inputUsageAuditTimer !== null) {
      clearTimeout(inputUsageAuditTimer);
      inputUsageAuditTimer = null;
    }
    inputUsageAuditTimer = setTimeout(() => {
      inputUsageAuditTimer = null;
      void refreshStreamInputUsageWarnings();
    }, 120);
  });

  $effect(() => {
    const effectiveId = streamState.stream?.id ?? streamId;
    if (streamState.activeTab !== 'controls' || !effectiveId) {
      closeControlSocket();
      return;
    }
    ensureControlSocket(effectiveId);
  });

  onDestroy(() => {
    if (inputUsageAuditTimer !== null) {
      clearTimeout(inputUsageAuditTimer);
      inputUsageAuditTimer = null;
    }
    closeControlSocket();
    closeStreamUpdatesSocket();
  });

  const core = $derived.by(() =>
    buildCameraPageCore({
      loading: streamState.loading,
      refreshing: streamState.refreshing,
      stopping: streamState.stopping,
      error: streamState.error,
      stream: streamState.stream,
      manifestState: streamState.manifestState,
      streamMetrics: streamState.streamMetrics,
      controls: streamState.controls,
      controlState: streamState.controlState,
      controlAppliedState: streamState.controlAppliedState,
      controlBusy: streamState.controlBusy,
      streamLookupDebug,
      streamApiBase,
      descriptor,
      currentCalibrationParams,
      streamViewerHost,
      streamViewerBounds,
      streamId,
      data,
      streamState,
      pipelineState,
      calibrationState
    })
  );

  const derivedState = $derived.by(() =>
    buildCameraPageDerived({
      activeMode,
      streamViewerAspect,
      streamViewerFit,
      encoderSettingsAvailable,
      tabs
    })
  );

  const ui = buildCameraPageUi({
    CalibrationGuidanceOverlay,
    CameraCalibrationTab,
    CameraControlsTab,
    CameraHeader,
    CameraMediaTab,
    CameraPipelineOverrides,
    CameraPipelinesTab,
    CameraPoseTab,
    CameraStreamSidebar,
    CameraStreamTab,
    PipelineIcon,
    StreamMetricsPanel,
    StreamPreview,
    FaIcon,
    faCamera
  });

  const constants = $derived.by(() =>
    buildCameraPageConstants({
      DEFAULT_LIBCAMERA_TARGET_FPS,
      DEFAULT_PIPELINE_UI,
      PIPELINE_OUTPUT_CELL_KEY,
      PIPELINE_UI_METADATA_KEY,
      PIPELINE_UI_STORAGE_PREFIX,
      RAW_LOOPBACK_GRAPH,
      RAW_PIPELINE_ID,
      RAW_PIPELINE_UUID: rawPipelineUuid
    })
  );

  const services = {
      OpenAPI,
      PipelinesApi,
      StreamsApi,
      connectStreamControls,
      connectStreamUpdates,
      resourceTelemetryStore,
      toaster
    
  };

  const helpers = {
      accessBadgeClass,
      accessLabel,
      allowDrop,
      apiPath,
      applyControl,
      applyDaedalusNodeOverrides,
      applyManifestSelections,
      applyStreamCrop,
      applyStreamCrosshair,
      applyStreamOrdering,
      applyStreamPreset,
      awaitStreamUpdatesSocket,
      backendLabel,
      buildControlValue,
      buildDaedalusGraphPatch,
      buildFileBackend,
      buildNetcamBackend,
      buildNodeValueFromInput,
      clampControlValue,
      clampNumber,
      controlMax,
      controlMin,
      controlStep,
      closeControlSocket,
      closeStreamUpdatesSocket,
      colorLabel,
      currentBackend,
      currentDevice,
      currentMode,
      decodersForCaptureFormat,
      dedupeCodecs,
      descriptor,
      displayValue,
      downloadManifest,
      effectiveModes,
      ensureControlSocket,
      ensureStreamUpdatesSocket,
      extractCurrentCalibrationParams,
      extractFileHandle,
      extractNodeOverrides,
      extractNodeValueDescriptors,
      extractValue,
      findPortMetadata,
      filteredControls,
      firstFormat,
      firstInterval,
      firstResolution,
      fpsLabel,
      fpsToFrameRate,
      frameRateToFps,
      fromApiGraphPlan,
      gcd,
      intervalToFps,
      intervalsForSelection,
      invalidateSWR,
      invalidateSWRPrefix,
      isControlChanged,
      isDaedalusPlan,
      isFileBackend,
      isNetcamBackend,
      isTimeoutError,
      layoutSignature,
      loadBackends,
      loadCodecs,
      mediaFormatMatches,
      menuOptions,
      menuValueDisplay,
      mergeNodeOverrides,
      modeFormat,
      modeKey,
      modeLabel,
      modeResolution,
      normalizeAssignedPipelineIds,
      normalizeCalibrationSolveResult,
      normalizeFpsLimit,
      normalizeGridOutputKeys,
      normalizeGridSlots,
      normalizeNodePortKey,
      normalizePipelineOutputMap,
      normalizePortMetadataMap,
      normalizeRotationDegrees,
      numberFromRegistryValue,
      parseManifestLayout,
      parseMetadataValue,
      pickCodecId,
      pipelineDataTypeFromTypeExpr,
      refresh,
      registryPortMetadataFor,
      registryPortTypeFor,
      reportError,
      resolutionKey,
      resolutionsForFormat,
      resolveDataTypeKey,
      resolvePoseCameraRef,
      safeCloneGraph,
      scheduleControlApply,
      scheduleStreamPresetApply,
      seedControlState,
      serializeGraphPlan,
      syncModeSelection,
      stopStream,
      timeoutApplyHint,
      onDestroy,
      onMount,
      untrack,
      uniqueFormats,
      updatePipelineInputDraft,
      updatePipelineNodeDraft,
      updatePipelineNodeValue
    
  };

  const mergeCtxParts = (...sources: object[]): CameraPageCtx => {
    const merged: Record<string, unknown> = {};
    for (const source of sources) {
      Object.defineProperties(merged, Object.getOwnPropertyDescriptors(source));
    }
    return merged as CameraPageCtx;
  };

  const ctx = $derived.by(() =>
    mergeCtxParts(core, streamState, pipelineRuntime, calibrationRuntime, ui, constants, services, helpers, derivedState, {
      streamBindings,
      pipelineBindings
    })
  );

</script>

{@render children?.({ ctx })}
