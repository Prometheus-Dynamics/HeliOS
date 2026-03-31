import { createCameraModeController } from './cameraModeController';
import { createCameraControlController } from './cameraControlController';
import { createCameraBackendController } from './cameraBackendController';
import { createCameraStreamState } from './cameraStreamStore.svelte';
import type { Mode } from '$lib/api/httpClient';

type CameraControlControllerDeps = Parameters<typeof createCameraControlController>[1];
type CameraBackendControllerDeps = Parameters<typeof createCameraBackendController>[1];

export type CameraPageControllerDeps = {
  streamState: ReturnType<typeof createCameraStreamState>;
  streamId: string;
  StreamsApi: CameraBackendControllerDeps['streamsApi'];
  toaster: CameraControlControllerDeps['toaster'];
  reportError: (params: { title: string; error?: unknown; message?: string; fallback?: string }) => void;
  modeKey: (value: unknown) => string | null;
  mediaFormatMatches: (mode: unknown, value: unknown) => boolean;
  intervalToFps: (interval: unknown) => number | null;
  frameRateToFps: (rate: unknown) => number | null;
  normalizeFpsLimit: (value: unknown) => number | null;
  normalizeRotationDegrees: (value: unknown) => number | null;
  parseManifestLayout: (layout: unknown) => {
    rows: number;
    columns: number;
    slots: Record<string, string | null>;
    outputKeys: Record<string, string | null>;
  } | null;
  outputSelectionForPipeline: (pipelineId: string) => string | null;
  setOutputSelectionForPipeline: (pipelineId: string, output: string | null) => void;
  DEFAULT_LIBCAMERA_TARGET_FPS: number;
  pipelineState: {
    pipelineGridRows: number;
    pipelineGridColumns: number;
    pipelineGridSlots: Record<string, string | null>;
    pipelineGridSlotOutputKeys: Record<string, string | null>;
    assignedPipelineIds: string[];
    pipelineAssignDraft: string[];
    pipelineOutputByPipelineId: Record<string, string | null>;
    selectedPipelineId: string | null;
    selectedPipelineOutput: string | null;
    pipelineLayoutTouched: boolean;
    pipelineAssignmentsTouched: boolean;
  };
};

export const createCameraPageControllers = (deps: CameraPageControllerDeps) => {
  const modeController = createCameraModeController(
    {
      get stream() {
        return deps.streamState.stream;
      },
      backendModes: () => {
        const device = deps.streamState.devices?.[deps.streamState.selectedDeviceIndex] ?? null;
        const backend = device?.backends?.[deps.streamState.selectedBackendIndex] ?? null;
        const modes = backend?.descriptor?.modes ?? [];
        return Array.isArray(modes) ? (modes.filter(Boolean) as Mode[]) : null;
      },
      get selectedModeKey() {
        return deps.streamState.selectedModeKey;
      },
      set selectedModeKey(value) {
        deps.streamState.selectedModeKey = value;
      },
      get selectedFormat() {
        return deps.streamState.selectedFormat;
      },
      set selectedFormat(value) {
        deps.streamState.selectedFormat = value;
      },
      get selectedResolution() {
        return deps.streamState.selectedResolution;
      },
      set selectedResolution(value) {
        deps.streamState.selectedResolution = value;
      },
      get decoders() {
        return deps.streamState.decoders;
      }
    },
    {
      modeKey: deps.modeKey
    }
  );

  const controlController = createCameraControlController(
    {
      get stream() {
        return deps.streamState.stream;
      },
      get streamId() {
        return deps.streamId;
      },
      get manifestState() {
        return deps.streamState.manifestState;
      },
      get controls() {
        return deps.streamState.controls;
      },
      set controls(value) {
        deps.streamState.controls = value;
      },
      get controlsQuery() {
        return deps.streamState.controlsQuery;
      },
      set controlsQuery(value) {
        deps.streamState.controlsQuery = value;
      },
      get controlState() {
        return deps.streamState.controlState;
      },
      set controlState(value) {
        deps.streamState.controlState = value;
      },
      get controlAppliedState() {
        return deps.streamState.controlAppliedState;
      },
      set controlAppliedState(value) {
        deps.streamState.controlAppliedState = value;
      },
      get controlBusy() {
        return deps.streamState.controlBusy;
      },
      set controlBusy(value) {
        deps.streamState.controlBusy = value;
      },
      get controlApplyTimers() {
        return deps.streamState.controlApplyTimers;
      },
      get controlApplySeqById() {
        return deps.streamState.controlApplySeqById;
      },
      get controlSocket() {
        return deps.streamState.controlSocket;
      }
    },
    {
      streamsApi: deps.StreamsApi,
      toaster: deps.toaster,
      reportError: deps.reportError,
      controlApplyDebounceMs: deps.streamState.CONTROL_APPLY_DEBOUNCE_MS
    }
  );

  const backendController = createCameraBackendController(
    {
      get stream() {
        return deps.streamState.stream;
      },
      get streamId() {
        return deps.streamId;
      },
      get manifestState() {
        return deps.streamState.manifestState;
      },
      get devices() {
        return deps.streamState.devices;
      },
      set devices(value) {
        deps.streamState.devices = value;
      },
      get selectedDeviceIndex() {
        return deps.streamState.selectedDeviceIndex;
      },
      set selectedDeviceIndex(value) {
        deps.streamState.selectedDeviceIndex = value;
      },
      get selectedBackendIndex() {
        return deps.streamState.selectedBackendIndex;
      },
      set selectedBackendIndex(value) {
        deps.streamState.selectedBackendIndex = value;
      },
      get codecs() {
        return deps.streamState.codecs;
      },
      set codecs(value) {
        deps.streamState.codecs = value;
      },
      get encoders() {
        return deps.streamState.encoders;
      },
      set encoders(value) {
        deps.streamState.encoders = value;
      },
      get decoders() {
        return deps.streamState.decoders;
      },
      set decoders(value) {
        deps.streamState.decoders = value;
      },
      get decoderDefaultIdsByCaptureFormat() {
        return deps.streamState.decoderDefaultIdsByCaptureFormat;
      },
      set decoderDefaultIdsByCaptureFormat(value) {
        deps.streamState.decoderDefaultIdsByCaptureFormat = value;
      },
      get encoderImpl() {
        return deps.streamState.encoderImpl;
      },
      set encoderImpl(value) {
        deps.streamState.encoderImpl = value;
      },
      get decoderImpl() {
        return deps.streamState.decoderImpl;
      },
      set decoderImpl(value) {
        deps.streamState.decoderImpl = value;
      },
      get encoderEnabled() {
        return deps.streamState.encoderEnabled;
      },
      set encoderEnabled(value) {
        deps.streamState.encoderEnabled = value;
      },
      get decoderEnabled() {
        return deps.streamState.decoderEnabled;
      },
      set decoderEnabled(value) {
        deps.streamState.decoderEnabled = value;
      },
      get encoderSelectionTouched() {
        return deps.streamState.encoderSelectionTouched;
      },
      set encoderSelectionTouched(value) {
        deps.streamState.encoderSelectionTouched = value;
      },
      get decoderSelectionTouched() {
        return deps.streamState.decoderSelectionTouched;
      },
      set decoderSelectionTouched(value) {
        deps.streamState.decoderSelectionTouched = value;
      },
      get encoderSettings() {
        return deps.streamState.encoderSettings;
      },
      set encoderSettings(value) {
        deps.streamState.encoderSettings = value;
      },
      get encoderFpsLimit() {
        return deps.streamState.encoderFpsLimit;
      },
      set encoderFpsLimit(value) {
        deps.streamState.encoderFpsLimit = value;
      },
      get decoderFpsLimit() {
        return deps.streamState.decoderFpsLimit;
      },
      set decoderFpsLimit(value) {
        deps.streamState.decoderFpsLimit = value;
      },
      get decoderRotationDegrees() {
        return deps.streamState.decoderRotationDegrees;
      },
      set decoderRotationDegrees(value) {
        deps.streamState.decoderRotationDegrees = value;
      },
      get decoderMirrorHorizontal() {
        return deps.streamState.decoderMirrorHorizontal;
      },
      set decoderMirrorHorizontal(value) {
        deps.streamState.decoderMirrorHorizontal = value;
      },
      get shadowRecorderEnabled() {
        return deps.streamState.shadowRecorderEnabled;
      },
      set shadowRecorderEnabled(value) {
        deps.streamState.shadowRecorderEnabled = value;
      },
      get hostBuffer() {
        return deps.streamState.hostBuffer;
      },
      set hostBuffer(value) {
        deps.streamState.hostBuffer = value;
      },
      get previewJpegQuality() {
        return deps.streamState.previewJpegQuality;
      },
      set previewJpegQuality(value) {
        deps.streamState.previewJpegQuality = value;
      },
      get cameraAlias() {
        return deps.streamState.cameraAlias;
      },
      set cameraAlias(value) {
        deps.streamState.cameraAlias = value;
      },
      get selectedModeKey() {
        return deps.streamState.selectedModeKey;
      },
      set selectedModeKey(value) {
        deps.streamState.selectedModeKey = value;
      },
      get selectedFormat() {
        return deps.streamState.selectedFormat;
      },
      set selectedFormat(value) {
        deps.streamState.selectedFormat = value;
      },
      get selectedResolution() {
        return deps.streamState.selectedResolution;
      },
      set selectedResolution(value) {
        deps.streamState.selectedResolution = value;
      },
      get selectedInterval() {
        return deps.streamState.selectedInterval;
      },
      set selectedInterval(value) {
        deps.streamState.selectedInterval = value;
      },
      get selectedIntervalIdx() {
        return deps.streamState.selectedIntervalIdx;
      },
      set selectedIntervalIdx(value) {
        deps.streamState.selectedIntervalIdx = value;
      },
      get libcameraTargetFps() {
        return deps.streamState.libcameraTargetFps;
      },
      set libcameraTargetFps(value) {
        deps.streamState.libcameraTargetFps = value;
      },
      get netcamTargetFps() {
        return deps.streamState.netcamTargetFps;
      },
      set netcamTargetFps(value) {
        deps.streamState.netcamTargetFps = value;
      },
      get fileBackendFps() {
        return deps.streamState.fileBackendFps;
      },
      set fileBackendFps(value) {
        deps.streamState.fileBackendFps = value;
      },
      get fileBackendLoop() {
        return deps.streamState.fileBackendLoop;
      },
      set fileBackendLoop(value) {
        deps.streamState.fileBackendLoop = value;
      },
      get fileBackendPathsText() {
        return deps.streamState.fileBackendPathsText;
      },
      set fileBackendPathsText(value) {
        deps.streamState.fileBackendPathsText = value;
      },
      get pipelineGridRows() {
        return deps.pipelineState.pipelineGridRows;
      },
      set pipelineGridRows(value) {
        deps.pipelineState.pipelineGridRows = value;
      },
      get pipelineGridColumns() {
        return deps.pipelineState.pipelineGridColumns;
      },
      set pipelineGridColumns(value) {
        deps.pipelineState.pipelineGridColumns = value;
      },
      get pipelineGridSlots() {
        return deps.pipelineState.pipelineGridSlots;
      },
      set pipelineGridSlots(value) {
        deps.pipelineState.pipelineGridSlots = value;
      },
      get pipelineGridSlotOutputKeys() {
        return deps.pipelineState.pipelineGridSlotOutputKeys;
      },
      set pipelineGridSlotOutputKeys(value) {
        deps.pipelineState.pipelineGridSlotOutputKeys = value;
      },
      get assignedPipelineIds() {
        return deps.pipelineState.assignedPipelineIds;
      },
      set assignedPipelineIds(value) {
        deps.pipelineState.assignedPipelineIds = value;
      },
      get pipelineAssignDraft() {
        return deps.pipelineState.pipelineAssignDraft;
      },
      set pipelineAssignDraft(value) {
        deps.pipelineState.pipelineAssignDraft = value;
      },
      get pipelineOutputByPipelineId() {
        return deps.pipelineState.pipelineOutputByPipelineId;
      },
      set pipelineOutputByPipelineId(value) {
        deps.pipelineState.pipelineOutputByPipelineId = value;
      },
      get selectedPipelineId() {
        return deps.pipelineState.selectedPipelineId;
      },
      set selectedPipelineId(value) {
        deps.pipelineState.selectedPipelineId = value;
      },
      get selectedPipelineOutput() {
        return deps.pipelineState.selectedPipelineOutput;
      },
      set selectedPipelineOutput(value) {
        deps.pipelineState.selectedPipelineOutput = value;
      },
      get pipelineLayoutTouched() {
        return deps.pipelineState.pipelineLayoutTouched;
      },
      set pipelineLayoutTouched(value) {
        deps.pipelineState.pipelineLayoutTouched = value;
      },
      get pipelineAssignmentsTouched() {
        return deps.pipelineState.pipelineAssignmentsTouched;
      },
      set pipelineAssignmentsTouched(value) {
        deps.pipelineState.pipelineAssignmentsTouched = value;
      }
    },
    {
      streamsApi: deps.StreamsApi,
      effectiveModes: modeController.effectiveModes,
      modeKey: deps.modeKey,
      modeFormat: modeController.modeFormat,
      modeResolution: modeController.modeResolution,
      mediaFormatMatches: deps.mediaFormatMatches,
      intervalsForSelection: modeController.intervalsForSelection,
      fpsLabel: modeController.fpsLabel,
      intervalToFps: deps.intervalToFps,
      frameRateToFps: deps.frameRateToFps,
      normalizeFpsLimit: deps.normalizeFpsLimit,
      normalizeRotationDegrees: deps.normalizeRotationDegrees,
      decodersForCaptureFormat: modeController.decodersForCaptureFormat,
      parseManifestLayout: deps.parseManifestLayout,
      outputSelectionForPipeline: deps.outputSelectionForPipeline,
      setOutputSelectionForPipeline: deps.setOutputSelectionForPipeline,
      extractValue: controlController.extractValue,
      DEFAULT_LIBCAMERA_TARGET_FPS: deps.DEFAULT_LIBCAMERA_TARGET_FPS
    }
  );

  return { modeController, controlController, backendController };
};
