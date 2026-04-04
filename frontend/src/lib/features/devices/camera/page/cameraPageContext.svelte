<script lang="ts" module>
  import { onDestroy, onMount, untrack, type ComponentProps, type Snippet } from 'svelte';
  import type { PageData } from '../../../../../routes/devices/[cameraId]/$types';
  import { DeviceService, apiUrl, getHttpClientApiBase } from '$lib/api/client';
  import { encoderSelectionId } from '$lib/api/streamEncoderSettings';
  import { apiFetchResponse } from '$lib/api/core/http';
  import { PipelinesApi } from '$lib/api/pipelinesApi';
  import { loadOwnedStreamCapabilities } from '$lib/api/streamResources';
  import { fromApiGraphPlan } from '$lib/features/pipelines/graphConverters';
  import { serializeGraphPlan } from '$lib/features/pipelines/graph';
  import { buildDaedalusGraphPatch } from '$lib/features/pipelines/daedalusGraph';
  import { StreamsApi } from '$lib/api/streamsApi';
  import { connectStreamControls } from '$lib/api/streamControls';
  import { connectStreamUpdates } from '$lib/api/streamUpdates';
  import { resourceTelemetryStore } from '$lib/api/telemetry';
  import { reportError } from '$lib/ui/errorPolicy';
  import { invalidateSWR, invalidateSWRPrefix } from '$lib/utils/swrCache';
  import { toaster } from '$lib';
  import { DEFAULT_PIPELINE_UI } from '$lib/features/pipelines/pipelineUiTypes';
  import { buildNodeValueFromInput, resolveDataTypeKey } from '$lib/features/pipelines/valueFormatting';
  import type CameraPageView from './CameraPageView.svelte';
  import { createCameraStreamState } from './cameraStreamStore.svelte';
  import { createCameraPagePipelineRuntime } from './cameraPagePipelineRuntime.svelte';
  import { createCameraPageCalibrationRuntime } from './cameraPageCalibrationRuntime.svelte';
  import {
    createBoundCameraStreamLifecycleController,
    createBoundCameraStreamPresetController
  } from './cameraPageControllerAdapters';
  import { createCameraStreamPresetHelpers } from './cameraStreamPresetHelpers';
  import { buildCameraPageConstants, buildCameraPageCore, buildCameraPageDerived } from './cameraPageUiBuilders';
  import { buildCameraTabs } from './cameraPageTabs';
  import { setupStreamViewerResize } from './cameraPageViewHelpers';
  import { createCameraPageControllers } from './cameraPageControllers';
  import {
    buildCameraPageHelpers,
    buildCameraPageServices,
    mergeCameraPageContext
  } from './cameraPageContextAssembly';
  import {
    asRecord,
    guidedModeFromManifest,
    normalizePipelineUuid,
    normalizeStreamCrosshair,
    normalizeStreamCrop,
    normalizeStreamOrderingMode,
    type StreamCrop,
    type StreamCrosshair,
    type StreamOrderingMode
  } from './cameraPageContextUtils';
  import { createCameraPageStateBindings } from './cameraPageStateBindings';
  import type {
    CameraPagePipelineBindingTarget,
    CameraPageStreamBindingTarget
  } from './cameraPageStateBindings';
  import { createCameraStreamOverlayRuntime } from './cameraStreamOverlayRuntime';
  import {
    DEFAULT_LIBCAMERA_TARGET_FPS,
    PIPELINE_UI_METADATA_KEY,
    type CameraPageTabId
  } from './cameraPageStateTypes';
  import { backendLabel, isTimeoutError, modeKey } from './cameraPageHelpers';
  import { resolvePoseCameraRef, normalizeCalibrationSolveResult, extractCurrentCalibrationParams, parseMetadataValue } from './cameraStateUtils';
  import { normalizeGridSlots, normalizeGridOutputKeys, layoutSignature, parseManifestLayout as parsePipelineManifestLayout } from './cameraPipelineState';
  import { fpsToFrameRate, frameRateToFps, gcd, intervalToFps, mediaFormatMatches, normalizeFpsLimit, normalizeRotationDegrees } from './cameraStreamState';
  import {
    PIPELINE_OUTPUT_CELL_KEY,
    PIPELINE_UI_STORAGE_PREFIX,
    RAW_LOOPBACK_GRAPH,
    RAW_PIPELINE_ID,
    RAW_PIPELINE_UUID,
    setRawPipelineUuid
  } from './cameraPipelineShared';
  import {
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
    safeCloneGraph
  } from './cameraPipelineTuningController';

  type CameraPageCtx = ComponentProps<typeof CameraPageView>['ctx'];

  export function createCameraPageContext(getData: () => PageData): CameraPageCtx {
    const streamId = $derived.by(() => getData().streamId);
    const getStreamId = () => streamId;
    const apiPath = (path: string): string => apiUrl(path);
    const apiBase = getHttpClientApiBase();
    type TabId = CameraPageTabId;
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
  const { pipelineStateBindings, streamBindings, pipelineBindings } = createCameraPageStateBindings({
    streamState: streamState as CameraPageStreamBindingTarget,
    pipelineState: pipelineState as CameraPagePipelineBindingTarget,
    normalizeStreamOrderingMode
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
      pipelineState
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
    const manifestGuided = guidedModeFromManifest(manifest, calibrationModePipelineUuid);
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
        streamState.encoders.find((c) => encoderSelectionId(c) === streamState.encoderImpl || c.implementation === streamState.encoderImpl || c.name === streamState.encoderImpl)
          ?.tunables?.encoder_settings
    )
  );

  $effect(() => {
    if (!encoderSettingsAvailable) streamState.encoderSettingsOpen = false;
  });

  const tabs: Array<{ id: TabId; label: string; ready: boolean }> = buildCameraTabs(mediaTabReady);

  const loadStreamCapabilities = async (): Promise<void> => {
    try {
      const capabilities = await loadOwnedStreamCapabilities();
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

  const streamLifecycleController = createBoundCameraStreamLifecycleController({
    streamState,
    getStreamId,
    getStreamLookupDebug: () => streamLookupDebug,
    setStreamLookupDebug: (value) => {
      streamLookupDebug = value;
    },
    getStreamApiBase: () => streamApiBase,
    setStreamApiBase: (value) => {
      streamApiBase = value;
    },
    StreamsApi,
    DeviceService,
    getHttpClientApiBase,
    toaster,
    reportError,
    loadBackends,
    loadCodecs,
    applyManifestSelections,
    refreshCalibrationImages,
    refreshCalibrationImportSources,
    refreshIpaStatus,
    seedControlState
  });

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












  
  const streamPresetController = createBoundCameraStreamPresetController({
    streamState,
    pipelineState,
    getStreamId,
    PipelinesApi,
    StreamsApi,
    apiBase,
    toaster,
    currentBackend,
    currentDevice,
    currentMode,
    intervalsForSelection,
    pickCodecId,
    outputSelectionForPipeline,
    applyPipelineOverridesToGraph,
    dropPipelineEverywhere,
    reportError,
    onExternalLayoutApplied: () => dismissGuidedCalibrationOverlay()
  });

  const { applyStreamPreset } = streamPresetController;
  const scheduleStreamPresetApplyRaw = streamPresetController.scheduleStreamPresetApply;

  const { scheduleStreamPresetApply } = createCameraStreamPresetHelpers({
    getPipelineUiHydrated: () => pipelineState.pipelineUiHydrated,
    scheduleStreamPresetApplyRaw
  });
  scheduleStreamPresetApplyFn = scheduleStreamPresetApply;
  const {
    applyStreamCrop,
    refreshStreamInputUsageWarnings,
    applyStreamCrosshair,
    applyStreamOrdering
  } = createCameraStreamOverlayRuntime({
    streamState,
    getStreamId,
    apiPath,
    apiFetchResponse,
    normalizeStreamCrop,
    normalizeStreamCrosshair,
    normalizeStreamOrderingMode
  });

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
      data: getData(),
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

  const services = buildCameraPageServices({
    PipelinesApi,
    StreamsApi,
    connectStreamControls,
    connectStreamUpdates,
    resourceTelemetryStore,
    toaster
  });

  const helpers = buildCameraPageHelpers({
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
  });

  const ctx = untrack(() =>
    mergeCameraPageContext<CameraPageCtx>(
      core,
      streamState,
      pipelineRuntime,
      calibrationRuntime,
      constants,
      services,
      helpers,
      derivedState,
      {
        streamBindings,
        pipelineBindings
      }
    )
  );

  return ctx;
  }
</script>
