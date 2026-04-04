import type { DeviceService as DeviceServiceApi } from '$lib/api/client';
import type { StreamsApi as SharedStreamsApi } from '$lib/api/streamsApi';
import type { PipelinesApi as PipelinesApiType } from '$lib/api/pipelinesApi';
import type { StreamCreationDefaults } from '$lib/api/streamDefaults';
import type { ControlMeta } from '$lib/api/client';
import type { EncoderSettingsDraft } from '$lib/api/streamEncoderSettings';
import { createCameraStreamState } from './cameraStreamStore.svelte';
import { createCameraStreamLifecycleController } from './cameraStreamLifecycleController';
import { createCameraStreamPresetController } from './cameraStreamPresetController';

type StreamState = ReturnType<typeof createCameraStreamState>;

type PipelineStateLike = {
  pipelineGridRows: number;
  pipelineGridColumns: number;
  pipelineGridSlots: Record<string, string | null>;
  pipelineGridSlotOutputKeys: Record<string, string | null>;
  pipelineOutputByPipelineId: Record<string, string | null>;
  assignedPipelineIds: string[];
  selectedPipelineId: string | null;
  selectedPipelineOutput: string | null;
  pipelineUiHydrated?: boolean;
};

export function createBoundCameraStreamLifecycleController({
  streamState,
  getStreamId,
  getStreamLookupDebug,
  setStreamLookupDebug,
  getStreamApiBase,
  setStreamApiBase,
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
}: {
  streamState: StreamState;
  getStreamId: () => string;
  getStreamLookupDebug: () => string | null;
  setStreamLookupDebug: (value: string | null) => void;
  getStreamApiBase: () => string | null;
  setStreamApiBase: (value: string | null) => void;
  StreamsApi: typeof SharedStreamsApi;
  DeviceService: typeof DeviceServiceApi;
  getHttpClientApiBase: () => string | null;
  toaster: { success: (payload: { title: string; description?: string }) => void };
  reportError: (args: { title: string; error: unknown; fallback: string }) => void;
  loadBackends: (manifest: StreamState['manifestState']) => Promise<void>;
  loadCodecs: (manifest: StreamState['manifestState']) => Promise<void>;
  applyManifestSelections: (manifest: StreamState['manifestState']) => void;
  refreshCalibrationImages: () => Promise<void>;
  refreshCalibrationImportSources: () => Promise<void>;
  refreshIpaStatus: () => Promise<void>;
  seedControlState: (entries: ControlMeta[]) => Record<number, number | boolean | null>;
}) {
  return createCameraStreamLifecycleController(
    {
      get stream() {
        return streamState.stream;
      },
      set stream(value) {
        streamState.stream = value;
      },
      get streamId() {
        return getStreamId();
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
        return getStreamLookupDebug();
      },
      set streamLookupDebug(value) {
        setStreamLookupDebug(value);
      },
      get streamApiBase() {
        return getStreamApiBase();
      },
      set streamApiBase(value) {
        setStreamApiBase(value);
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
    }
  );
}

export function createBoundCameraStreamPresetController({
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
  onExternalLayoutApplied
}: {
  streamState: StreamState;
  pipelineState: PipelineStateLike;
  getStreamId: () => string;
  PipelinesApi: typeof PipelinesApiType;
  StreamsApi: typeof SharedStreamsApi;
  apiBase?: string | null;
  toaster: {
    success: (payload: { title: string; description?: string }) => void;
    error: (payload: { title: string; description?: string }) => void;
  };
  currentBackend: Parameters<typeof createCameraStreamPresetController>[1]['currentBackend'];
  currentDevice: Parameters<typeof createCameraStreamPresetController>[1]['currentDevice'];
  currentMode: Parameters<typeof createCameraStreamPresetController>[1]['currentMode'];
  intervalsForSelection: Parameters<typeof createCameraStreamPresetController>[1]['intervalsForSelection'];
  pickCodecId: Parameters<typeof createCameraStreamPresetController>[1]['pickCodecId'];
  outputSelectionForPipeline: Parameters<typeof createCameraStreamPresetController>[1]['outputSelectionForPipeline'];
  applyPipelineOverridesToGraph: Parameters<typeof createCameraStreamPresetController>[1]['applyPipelineOverridesToGraph'];
  dropPipelineEverywhere: Parameters<typeof createCameraStreamPresetController>[1]['dropPipelineEverywhere'];
  reportError: Parameters<typeof createCameraStreamPresetController>[1]['reportError'];
  onExternalLayoutApplied?: () => void;
}) {
  return createCameraStreamPresetController(
    {
      get streamId() {
        return getStreamId();
      },
      get stream() {
        return streamState.stream;
      },
      get streamDefaults() {
        return streamState.streamDefaults as StreamCreationDefaults | null;
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
      get pipelineOutputByPipelineId() {
        return pipelineState.pipelineOutputByPipelineId;
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
      get encoderSelectionTouched() {
        return streamState.encoderSelectionMode === 'manual';
      },
      get encoderSettings() {
        return streamState.encoderSettings as EncoderSettingsDraft;
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
      get previewJpegQuality() {
        return streamState.previewJpegQuality;
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
      apiBase: apiBase ?? undefined,
      toaster,
      STREAM_PRESET_DEBOUNCE_MS: streamState.STREAM_PRESET_DEBOUNCE_MS,
      currentBackend,
      currentDevice,
      currentMode,
      intervalsForSelection,
      pickCodecId,
      outputSelectionForPipeline,
      applyPipelineOverridesToGraph,
      dropPipelineEverywhere,
      reportError,
      onExternalLayoutApplied
    }
  );
}
