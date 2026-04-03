import type { CodecInfo, Interval, Mode, ProbedBackend, ProbedDevice, StreamInfo, StreamManifest } from '$lib/api/client';
import { OpenAPI, getHttpClientBase } from '$lib/api/client';
import { extractError } from '$lib/api/errors';
import type { EncoderSettingsDraft } from '$lib/api/streamEncoderSettings';
import type { StreamCreationDefaults } from '$lib/api/streamDefaults';
import type { PipelinesApi } from '$lib/api/pipelinesApi';
import type { StreamsApi } from '$lib/api/streamsApi';
import { buildCameraStreamPresetManifest } from './cameraStreamPresetManifest';

type PresetState = {
  get streamId(): string;
  get stream(): StreamInfo | null;
  get streamDefaults(): StreamCreationDefaults | null;
  get applying(): boolean;
  set applying(value: boolean);
  get pendingStreamPresetApply(): boolean;
  set pendingStreamPresetApply(value: boolean);
  get pendingStreamPresetSilent(): boolean;
  set pendingStreamPresetSilent(value: boolean);
  get pipelineGridRows(): number;
  get pipelineGridColumns(): number;
  get pipelineGridSlots(): Record<string, string | null>;
  set pipelineGridSlots(value: Record<string, string | null>);
  get pipelineGridSlotOutputKeys(): Record<string, string | null>;
  get pipelineOutputByPipelineId(): Record<string, string | null>;
  get assignedPipelineIds(): string[];
  set assignedPipelineIds(value: string[]);
  get selectedPipelineId(): string | null;
  set selectedPipelineId(value: string | null);
  get selectedPipelineOutput(): string | null;
  set selectedPipelineOutput(value: string | null);
  get cameraAlias(): string;
  get encoderImpl(): string | null;
  get decoderImpl(): string | null;
  get decoders(): CodecInfo[];
  get encoderEnabled(): boolean;
  get decoderEnabled(): boolean;
  get encoderSelectionTouched(): boolean;
  get encoderSettings(): EncoderSettingsDraft;
  get encoderFpsLimit(): number | null;
  get decoderFpsLimit(): number | null;
  get decoderRotationDegrees(): number | null;
  get decoderMirrorHorizontal(): boolean;
  get shadowRecorderEnabled(): boolean;
  get libcameraTargetFps(): number | null;
  get netcamTargetFps(): number | null;
  get fileBackendFps(): number | null;
  get fileBackendLoop(): boolean;
  get fileBackendPathsText(): string;
  get hostBuffer(): number | null;
  get previewJpegQuality(): number | null;
  get selectedIntervalIdx(): number;
  get selectedModeKey(): string | null;
  get selectedFormat(): string;
  get selectedResolution(): string;
  get streamPresetApplyTimer(): number | null;
  set streamPresetApplyTimer(value: number | null);
};

type PresetDeps = {
  pipelinesApi: typeof PipelinesApi;
  streamsApi: typeof StreamsApi;
  apiBase?: string;
  toaster: {
    success: (payload: { title: string; description?: string }) => void;
    error: (payload: { title: string; description?: string }) => void;
  };
  STREAM_PRESET_DEBOUNCE_MS: number;
  currentBackend: () => ProbedBackend | null;
  currentDevice: () => ProbedDevice | null;
  currentMode: () => Mode | null;
  intervalsForSelection: () => Interval[];
  pickCodecId: (list: CodecInfo[], desired: string | null | undefined, preferred?: string[]) => string | null;
  outputSelectionForPipeline: (pipelineId: string) => string | null;
  applyPipelineOverridesToGraph: (pipelineId: string, graph: unknown) => unknown;
  dropPipelineEverywhere: (pipelineId: string) => void;
  onExternalLayoutApplied?: () => void;
  reportError: (params: { title: string; error: unknown; fallback: string }) => void;
};

export function createCameraStreamPresetController(state: PresetState, deps: PresetDeps) {
  function ensureApiBase(): void {
    try {
      getHttpClientBase();
      return;
    } catch {
      // Use the server-provided base when local resolution is unavailable.
    }
    if (!deps.apiBase) return;
    OpenAPI.BASE = deps.apiBase.replace(/\/+$/, '');
  }

  function scheduleStreamPresetApply(): void {
    if (state.streamPresetApplyTimer != null) {
      clearTimeout(state.streamPresetApplyTimer);
    }
    state.streamPresetApplyTimer = window.setTimeout(() => {
      state.streamPresetApplyTimer = null;
      void applyStreamPreset({ silent: true });
    }, deps.STREAM_PRESET_DEBOUNCE_MS);
  }

  async function applyStreamPreset(options: { silent?: boolean } = {}): Promise<void> {
    ensureApiBase();
    const backend = deps.currentBackend();
    const device = deps.currentDevice();
    const mode = deps.currentMode();
    const interval = deps.intervalsForSelection()[state.selectedIntervalIdx] ?? deps.intervalsForSelection()[0] ?? null;
    if (!backend || !device || !mode) {
      deps.reportError({
        title: 'No backend available',
        error: new Error('No backend selected for stream preset apply.'),
        fallback: 'Select a device/backend before applying.'
      });
      return;
    }
    if (state.applying) {
      state.pendingStreamPresetApply = true;
      if (!options.silent) {
        state.pendingStreamPresetSilent = false;
      }
      return;
    }
    state.applying = true;
    try {
      const streamDefaults = state.streamDefaults;
      if (!streamDefaults) {
        throw new Error('Stream defaults are unavailable');
      }
      const payload = buildCameraStreamPresetManifest({
        stream: state.stream,
        streamDefaults,
        backend,
        device,
        mode,
        interval,
        pipelineGridRows: state.pipelineGridRows,
        pipelineGridColumns: state.pipelineGridColumns,
        pipelineGridSlots: state.pipelineGridSlots,
        pipelineGridSlotOutputKeys: state.pipelineGridSlotOutputKeys,
        assignedPipelineIds: state.assignedPipelineIds,
        selectedPipelineId: state.selectedPipelineId,
        cameraAlias: state.cameraAlias,
        encoderImpl: state.encoderImpl,
        decoderImpl: state.decoderImpl,
        encoderEnabled: state.encoderEnabled,
        decoderEnabled: state.decoderEnabled,
        encoderSettings: state.encoderSettings,
        encoderFpsLimit: state.encoderFpsLimit,
        decoderFpsLimit: state.decoderFpsLimit,
        decoderRotationDegrees: state.decoderRotationDegrees,
        decoderMirrorHorizontal: state.decoderMirrorHorizontal,
        shadowRecorderEnabled: state.shadowRecorderEnabled,
        libcameraTargetFps: state.libcameraTargetFps,
        netcamTargetFps: state.netcamTargetFps,
        fileBackendFps: state.fileBackendFps,
        fileBackendLoop: state.fileBackendLoop,
        fileBackendPathsText: state.fileBackendPathsText,
        hostBuffer: state.hostBuffer,
        previewJpegQuality: state.previewJpegQuality,
        outputSelectionForPipeline: deps.outputSelectionForPipeline
      });

      await deps.streamsApi.startStream({ requestBody: payload });
      deps.onExternalLayoutApplied?.();
      if (!options.silent) {
        deps.toaster.success({ title: 'Stream updated', description: 'Pipeline & capture settings applied.' });
      }
    } catch (err) {
      console.warn('Failed to apply stream preset', err);
      deps.reportError({
        title: 'Apply failed',
        error: err,
        fallback: extractError(err)
      });
    } finally {
      state.applying = false;
      if (state.pendingStreamPresetApply) {
        const silent = state.pendingStreamPresetSilent;
        state.pendingStreamPresetApply = false;
        state.pendingStreamPresetSilent = true;
        void applyStreamPreset({ silent });
      }
    }
  }

  return { scheduleStreamPresetApply, applyStreamPreset };
}
