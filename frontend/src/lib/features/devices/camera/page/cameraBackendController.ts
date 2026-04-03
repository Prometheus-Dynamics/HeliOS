import type {
  CodecInfo,
  EncoderSettings,
  ProbedBackend,
  ProbedDevice,
  StreamInfo,
  StreamManifest,
  Mode,
  Interval
} from '$lib/api/client';
import type { StreamsApi } from '$lib/api/streamsApi';
import { createEncoderSettingsDraft, encoderSettingsDraftFromUnknown, type EncoderSettingsDraft } from '$lib/api/streamEncoderSettings';
import {
  defaultPreviewJpegQualityForEncoder,
  resolveStreamCreationDefaults,
  type StreamCreationDefaults
} from '$lib/api/streamDefaults';
import { recordingModeEnabled } from '$lib/api/streamRecordingMode';
import {
  dedupeCodecs,
  normalizeDecoderDefaultIdsByCaptureFormat,
  pickCodecId
} from './cameraBackendCodecs';
import { deriveStreamCodecSelections } from './cameraStreamConfigBuilder';
import {
  asInterval,
  asJpegQuality,
  asPositiveNumber,
  asRecord,
  asTrimmedString,
  buildFileBackend,
  buildNetcamBackend,
  captureRecordFor,
  extractFileHandle,
  identityRecordFor,
  isFileBackend,
  isNetcamBackend,
  normalizeFormatKey,
  resolvedCodecId
} from './cameraBackendSupport';
import {
  applyCameraStreamModeSelection,
  clampCameraStreamIntervalSelection,
  syncCameraStreamEditorCodecs,
  type CameraStreamEditorState,
  type StreamSelectionMode
} from './cameraStreamEditorReducer';
import { cameraIntervalsForSelection } from './cameraModeSelectors';
import { layoutSignature } from './cameraPipelineState';
import {
  PIPELINE_OUTPUT_CELL_KEY,
  RAW_PIPELINE_ID,
  RAW_PIPELINE_UUID,
  normalizeAssignedPipelineIds,
  normalizePipelineOutputMap
} from './cameraPipelineShared';

type BackendState = {
  get stream(): StreamInfo | null;
  get streamId(): string;
  get manifestState(): StreamManifest | null;
  get devices(): ProbedDevice[];
  set devices(value: ProbedDevice[]);
  get selectedDeviceIndex(): number;
  set selectedDeviceIndex(value: number);
  get selectedBackendIndex(): number;
  set selectedBackendIndex(value: number);
  get codecs(): CodecInfo[];
  set codecs(value: CodecInfo[]);
  get encoders(): CodecInfo[];
  set encoders(value: CodecInfo[]);
  get decoders(): CodecInfo[];
  set decoders(value: CodecInfo[]);
  get streamDefaults(): StreamCreationDefaults | null;
  set streamDefaults(value: StreamCreationDefaults | null);
  get decoderDefaultIdsByCaptureFormat(): Record<string, string>;
  set decoderDefaultIdsByCaptureFormat(value: Record<string, string>);
  get encoderImpl(): string | null;
  set encoderImpl(value: string | null);
  get decoderImpl(): string | null;
  set decoderImpl(value: string | null);
  get encoderEnabled(): boolean;
  set encoderEnabled(value: boolean);
  get decoderEnabled(): boolean;
  set decoderEnabled(value: boolean);
  get encoderSelectionMode(): StreamSelectionMode;
  set encoderSelectionMode(value: StreamSelectionMode);
  get decoderSelectionMode(): StreamSelectionMode;
  set decoderSelectionMode(value: StreamSelectionMode);
  get encoderSettings(): EncoderSettingsDraft;
  set encoderSettings(value: EncoderSettingsDraft);
  get encoderFpsLimit(): number | null;
  set encoderFpsLimit(value: number | null);
  get decoderFpsLimit(): number | null;
  set decoderFpsLimit(value: number | null);
  get decoderRotationDegrees(): number | null;
  set decoderRotationDegrees(value: number | null);
  get decoderMirrorHorizontal(): boolean;
  set decoderMirrorHorizontal(value: boolean);
  get shadowRecorderEnabled(): boolean;
  set shadowRecorderEnabled(value: boolean);
  get hostBuffer(): number | null;
  set hostBuffer(value: number | null);
  get previewJpegQuality(): number | null;
  set previewJpegQuality(value: number | null);
  get cameraAlias(): string;
  set cameraAlias(value: string);
  get selectedModeKey(): string | null;
  set selectedModeKey(value: string | null);
  get selectedFormat(): string;
  set selectedFormat(value: string);
  get selectedResolution(): string;
  set selectedResolution(value: string);
  get selectedIntervalIdx(): number;
  set selectedIntervalIdx(value: number);
  get libcameraTargetFps(): number | null;
  set libcameraTargetFps(value: number | null);
  get netcamTargetFps(): number | null;
  set netcamTargetFps(value: number | null);
  get fileBackendFps(): number | null;
  set fileBackendFps(value: number | null);
  get fileBackendLoop(): boolean;
  set fileBackendLoop(value: boolean);
  get fileBackendPathsText(): string;
  set fileBackendPathsText(value: string);
  get pipelineGridRows(): number;
  set pipelineGridRows(value: number);
  get pipelineGridColumns(): number;
  set pipelineGridColumns(value: number);
  get pipelineGridSlots(): Record<string, string | null>;
  set pipelineGridSlots(value: Record<string, string | null>);
  get pipelineGridSlotOutputKeys(): Record<string, string | null>;
  set pipelineGridSlotOutputKeys(value: Record<string, string | null>);
  get assignedPipelineIds(): string[];
  set assignedPipelineIds(value: string[]);
  get pipelineAssignDraft(): string[];
  set pipelineAssignDraft(value: string[]);
  get pipelineOutputByPipelineId(): Record<string, string | null>;
  set pipelineOutputByPipelineId(value: Record<string, string | null>);
  get selectedPipelineId(): string | null;
  set selectedPipelineId(value: string | null);
  get selectedPipelineOutput(): string | null;
  set selectedPipelineOutput(value: string | null);
  get pipelineLayoutTouched(): boolean;
  set pipelineLayoutTouched(value: boolean);
  get pipelineAssignmentsTouched(): boolean;
  set pipelineAssignmentsTouched(value: boolean);
};

type BackendDeps = {
  streamsApi: typeof StreamsApi;
  effectiveModes: () => Mode[];
  modeKey: (value: unknown) => string | null;
  modeFormat: (mode: Mode | null | undefined) => string;
  modeResolution: (mode: Mode | null | undefined) => string;
  mediaFormatMatches: (mode: Mode | null | undefined, value: unknown) => boolean;
  intervalsForSelection: () => Interval[];
  fpsLabel: (interval: Interval | undefined | null) => string;
  intervalToFps: (interval: Interval | null | undefined) => number | null;
  frameRateToFps: (rate: unknown) => number | null;
  normalizeFpsLimit: (value: unknown) => number | null;
  normalizeRotationDegrees: (value: unknown) => number | null;
  parseManifestLayout: (layout: unknown) => { rows: number; columns: number; slots: Record<string, string | null>; outputKeys: Record<string, string | null> } | null;
  outputSelectionForPipeline: (pipelineId: string) => string | null;
  setOutputSelectionForPipeline: (pipelineId: string, output: string | null) => void;
  extractValue: (value: unknown) => number | boolean | null;
  DEFAULT_LIBCAMERA_TARGET_FPS: number;
};

export function createCameraBackendController(state: BackendState, deps: BackendDeps) {
  const editorStateFor = (): CameraStreamEditorState => ({
    selectedBackendIndex: state.selectedBackendIndex,
    selectedModeKey: state.selectedModeKey,
    selectedFormat: state.selectedFormat,
    selectedResolution: state.selectedResolution,
    selectedIntervalIdx: state.selectedIntervalIdx,
    shadowRecorderEnabled: state.shadowRecorderEnabled,
    encoderImpl: state.encoderImpl,
    decoderImpl: state.decoderImpl,
    encoderEnabled: state.encoderEnabled,
    decoderEnabled: state.decoderEnabled,
    encoderSelectionMode: state.encoderSelectionMode,
    decoderSelectionMode: state.decoderSelectionMode
  });
  const applyEditorState = (next: CameraStreamEditorState): void => {
    state.selectedBackendIndex = next.selectedBackendIndex;
    state.selectedModeKey = next.selectedModeKey;
    state.selectedFormat = next.selectedFormat;
    state.selectedResolution = next.selectedResolution;
    state.selectedIntervalIdx = next.selectedIntervalIdx;
    state.shadowRecorderEnabled = next.shadowRecorderEnabled;
    state.encoderImpl = next.encoderImpl;
    state.decoderImpl = next.decoderImpl;
    state.encoderEnabled = next.encoderEnabled;
    state.decoderEnabled = next.decoderEnabled;
    state.encoderSelectionMode = next.encoderSelectionMode;
    state.decoderSelectionMode = next.decoderSelectionMode;
  };
  const resolvedEncoderStateFor = (): StreamInfo['resolved']['encoder'] | null =>
    state.stream?.resolved?.encoder ?? null;
  const resolvedDecoderStateFor = (): StreamInfo['resolved']['decoder'] | null =>
    state.stream?.resolved?.decoder ?? null;
  const requestedEncoderRecordFor = (manifest?: StreamManifest | null): Record<string, unknown> | null =>
    asRecord(asRecord(manifest)?.encoder);
  const requestedEncoderIdFor = (manifest?: StreamManifest | null): string | null => {
    const requestedId = asTrimmedString(requestedEncoderRecordFor(manifest)?.id);
    if (requestedId) return requestedId;
    const legacyId = asTrimmedString(asRecord(manifest)?.encoder_id);
    return legacyId || null;
  };
  const requestedEncoderStateFor = (manifest?: StreamManifest | null): 'enabled' | 'disabled' | null => {
    const requestedState = asTrimmedString(requestedEncoderRecordFor(manifest)?.state).toLowerCase();
    if (requestedState === 'enabled' || requestedState === 'disabled') return requestedState;
    const legacyEnabled = asRecord(manifest)?.encoder_enabled;
    return typeof legacyEnabled === 'boolean' ? (legacyEnabled ? 'enabled' : 'disabled') : null;
  };
  const requestedEncoderSettingsFor = (manifest?: StreamManifest | null): EncoderSettings | Record<string, unknown> | null =>
    ((requestedEncoderRecordFor(manifest)?.settings as EncoderSettings | undefined) ?? asRecord(asRecord(manifest)?.encoder_settings));
  const requestedDecoderRecordFor = (manifest?: StreamManifest | null): Record<string, unknown> | null =>
    asRecord(asRecord(manifest)?.decoder);
  const requestedDecoderIdFor = (manifest?: StreamManifest | null): string | null => {
    const requestedId = asTrimmedString(requestedDecoderRecordFor(manifest)?.id);
    if (requestedId) return requestedId;
    const legacyId = asTrimmedString(asRecord(manifest)?.decoder_id);
    return legacyId || null;
  };
  const requestedDecoderStateFor = (manifest?: StreamManifest | null): 'enabled' | 'disabled' | null => {
    const requestedState = asTrimmedString(requestedDecoderRecordFor(manifest)?.state).toLowerCase();
    if (requestedState === 'enabled' || requestedState === 'disabled') return requestedState;
    const legacyEnabled = asRecord(manifest)?.decoder_enabled;
    return typeof legacyEnabled === 'boolean' ? (legacyEnabled ? 'enabled' : 'disabled') : null;
  };
  const requestedDecoderSettingsFor = (manifest?: StreamManifest | null): Record<string, unknown> | null =>
    asRecord(requestedDecoderRecordFor(manifest)?.settings) ?? asRecord(asRecord(manifest)?.decoder_settings);
  const asFiniteNumber = (value: unknown): number | null => {
    const numeric = Number(value);
    return Number.isFinite(numeric) ? numeric : null;
  };

  async function loadBackends(manifest?: StreamManifest | null): Promise<void> {
    const netcamDevice = buildNetcamBackend(state.stream, manifest);
    if (netcamDevice) {
      state.devices = [netcamDevice];
      state.selectedDeviceIndex = 0;
      state.selectedBackendIndex = 0;
      return;
    }
    const fileDevice = buildFileBackend(state.stream, manifest);
    if (fileDevice) {
      state.devices = [fileDevice];
      state.selectedDeviceIndex = 0;
      state.selectedBackendIndex = 0;
      return;
    }
    try {
      const list = await deps.streamsApi.listBackends();
      state.devices = Array.isArray(list) ? list : [];
      applyManifestSelections(manifest);
    } catch (err) {
      console.warn('Failed to load backends', err);
    }
  }

  async function loadCodecs(manifest?: StreamManifest | null): Promise<void> {
    try {
      const list = await deps.streamsApi.listCodecs();
      const runtimeStatus = await deps.streamsApi.runtimeStatus();
      const resolvedEncoderId = resolvedCodecId(resolvedEncoderStateFor());
      const resolvedDecoderId = resolvedCodecId(resolvedDecoderStateFor());
      state.streamDefaults = resolveStreamCreationDefaults(runtimeStatus?.streams?.capabilities);
      const defaultEncoderId = state.streamDefaults?.defaultEncoderId ?? null;
      state.codecs = Array.isArray(list) ? list : [];
      state.decoderDefaultIdsByCaptureFormat = normalizeDecoderDefaultIdsByCaptureFormat(state.streamDefaults?.defaultDecoderIdsByCaptureFormat);
      const allEncoders = dedupeCodecs(
        state.codecs.filter((c) => String(c.kind).toLowerCase() === 'encoder'),
        (c) => `${c.name}::${c.implementation}::${c.input}::${c.output}`
      );
      state.encoders = dedupeCodecs(allEncoders, (c) => `${c.name}::${c.implementation}`);

      state.decoders = dedupeCodecs(
        state.codecs.filter((c) => String(c.kind).toLowerCase() === 'decoder'),
        (c) => `${c.name}::${c.implementation}::${c.input}::${c.output}`
      );
      const selections = deriveStreamCodecSelections({
        encoders: state.encoders,
        decoders: state.decoders,
        encoderImpl: state.encoderImpl,
        decoderImpl: state.decoderImpl,
        resolvedEncoderId,
        resolvedDecoderId,
        requestedEncoderId: requestedEncoderIdFor(manifest),
        requestedDecoderId: requestedDecoderIdFor(manifest),
        defaultEncoderId,
        selectedFormat: manifest?.capture?.mode?.format?.code ?? null,
        decoderDefaultIdsByCaptureFormat: state.decoderDefaultIdsByCaptureFormat,
        encoderSelectionMode: state.encoderSelectionMode,
        decoderSelectionMode: state.decoderSelectionMode
      });
      applyEditorState({ ...editorStateFor(), encoderImpl: selections.encoderImpl, decoderImpl: selections.decoderImpl });
    } catch (err) {
      console.warn('Failed to load codec catalog', err);
    }
  }

  function applyManifestSelections(manifest?: StreamManifest | null): void {
    const capture = manifest?.capture;
    const manifestRecord = asRecord(manifest);
    const captureRecord = captureRecordFor(manifest);
    const captureModeRecord = asRecord(captureRecord?.mode);
    const identityRecord = identityRecordFor(manifest);
    const resolvedEncoder = resolvedEncoderStateFor();
    const resolvedDecoder = resolvedDecoderStateFor();
    const streamDefaults = state.streamDefaults;
    const resolvedEncoderId = resolvedCodecId(resolvedEncoder);
    const resolvedDecoderId = resolvedCodecId(resolvedDecoder);
    const requestedEncoderState = requestedEncoderStateFor(manifest);
    const encoderSettingsWire = requestedEncoderSettingsFor(manifest);
    const requestedDecoderState = requestedDecoderStateFor(manifest);
    const decoderSettingsWire = requestedDecoderSettingsFor(manifest);
    if (manifest) {
      state.decoderFpsLimit = deps.normalizeFpsLimit(decoderSettingsWire?.fps_limit ?? null);
      state.decoderRotationDegrees = deps.normalizeRotationDegrees(decoderSettingsWire?.rotation_degrees ?? 0);
      state.decoderMirrorHorizontal = Boolean(decoderSettingsWire?.mirror_horizontal ?? false);
    }
    if (!state.devices.length) {
      state.selectedDeviceIndex = -1;
      return;
    }

    const targetKeys = capture?.device_keys ?? [];
    const manifestKeys = manifest?.identity?.keys ?? [];
    const desiredKeys = [...targetKeys, ...manifestKeys].filter((key): key is string => typeof key === 'string' && key.length > 0);
    const deviceIdx = state.devices.findIndex((dev) => {
      const keys = dev.identity?.keys ?? [];
      return desiredKeys.some((k) => keys.includes(k));
    });
    state.selectedDeviceIndex = deviceIdx >= 0 ? deviceIdx : 0;

    const backends = currentDevice()?.backends ?? [];
    const backendIdx = backends.findIndex((b) => {
      if (capture && b.kind === capture.backend) {
        try {
          return JSON.stringify(b.handle) === JSON.stringify(capture.handle);
        } catch {
          return b.kind === capture.backend;
        }
      }
      return false;
    });
    state.selectedBackendIndex = backendIdx >= 0 ? backendIdx : 0;

    const modes = deps.effectiveModes();
    const wantedFormat = captureModeRecord?.format ?? null;
    const mode = modes.find((m) => deps.mediaFormatMatches(m, wantedFormat)) ?? modes[0] ?? null;
    const wantedInterval = asInterval(captureRecord?.interval ?? captureModeRecord?.interval ?? null);
    const nextEditorState = applyCameraStreamModeSelection(
      {
        ...editorStateFor(),
        selectedBackendIndex: backendIdx >= 0 ? backendIdx : 0,
        selectedModeKey: deps.modeKey(captureModeRecord ?? null) ?? deps.modeKey(mode?.id) ?? null,
        selectedFormat: deps.modeFormat(mode),
        selectedResolution: deps.modeResolution(mode),
        selectedIntervalIdx: 0
      },
      modes,
      deps
    );
    const nextIntervals = cameraIntervalsForSelection(
      modes,
      nextEditorState.selectedFormat,
      nextEditorState.selectedResolution
    );
    const nextIntervalIdx = wantedInterval
      ? Math.max(
          0,
          nextIntervals.findIndex(
            (int) => int?.numerator === wantedInterval?.numerator && int?.denominator === wantedInterval?.denominator
          )
        )
      : 0;
    const hydratedEditorState = clampCameraStreamIntervalSelection(
      {
        ...nextEditorState,
        selectedIntervalIdx: nextIntervalIdx
      },
      modes
    );
    applyEditorState(hydratedEditorState);
    const intervals = cameraIntervalsForSelection(
      modes,
      hydratedEditorState.selectedFormat,
      hydratedEditorState.selectedResolution
    );
    const isLibcamera = String(capture?.backend ?? '').toLowerCase() === 'libcamera';
    state.libcameraTargetFps =
      (typeof captureRecord?.target_fps === 'number' ? captureRecord.target_fps : null) ??
      deps.intervalToFps(wantedInterval) ??
      deps.intervalToFps(intervals[state.selectedIntervalIdx]) ??
      (isLibcamera ? deps.DEFAULT_LIBCAMERA_TARGET_FPS : null);
    const netcamHandleFps = (() => {
      const handle = asRecord(capture?.handle);
      const direct = handle && String(handle.type ?? '').toLowerCase() === 'netcam' ? handle : null;
      const legacy = handle ? asRecord(handle.Netcam) : null;
      const fps = asPositiveNumber(direct?.fps);
      if (fps) return Math.round(fps);
      const legacyFps = asPositiveNumber(legacy?.fps);
      if (legacyFps) return Math.round(legacyFps);
      return null;
    })();
    state.netcamTargetFps = netcamHandleFps ?? deps.intervalToFps(wantedInterval) ?? deps.intervalToFps(intervals[state.selectedIntervalIdx]) ?? null;

    if (isFileBackend(capture?.backend)) {
      const fileHandle = extractFileHandle(capture?.handle);
      const fpsRaw = Number(fileHandle?.fps ?? 0);
      const fallbackFps = Number.isFinite(fpsRaw) && fpsRaw > 0 ? fpsRaw : null;
      const controlMeta = (currentBackend()?.descriptor?.controls ?? state.stream?.descriptor?.controls ?? []).find(
        (ctrl) => ctrl.name === 'file.duration_ms'
      );
      const controlValue = controlMeta?.id != null ? (capture?.controls ?? []).find((entry) => entry.id === controlMeta.id)?.value : null;
      const durationMs = deps.extractValue(controlValue);
      const durationFps = typeof durationMs === 'number' && durationMs > 0 ? Math.max(1, Math.round(1000 / durationMs)) : null;
      state.fileBackendFps = durationFps ?? fallbackFps;
      state.fileBackendLoop = fileHandle?.loop_forever ?? true;
      const paths = Array.isArray(fileHandle?.paths) ? fileHandle.paths.filter((p) => typeof p === 'string' && p.trim().length) : [];
      state.fileBackendPathsText = paths.join('\n');
    }

    state.hostBuffer =
      asFiniteNumber(manifest?.host_buffer ?? manifestRecord?.host_buffer) ??
      streamDefaults?.defaultHostBuffer ??
      state.hostBuffer;
    state.shadowRecorderEnabled = isFileBackend(capture?.backend)
      ? false
      : recordingModeEnabled(manifest?.recording_mode ?? streamDefaults?.defaultRecordingMode);
    state.cameraAlias = asTrimmedString(identityRecord?.alias ?? identityRecord?.display);
    const encoderEnabledFlag = manifestRecord?.encoder_enabled;
    const selections = deriveStreamCodecSelections({
      encoders: state.encoders,
      decoders: state.decoders,
      encoderImpl: state.encoderImpl,
      decoderImpl: state.decoderImpl,
      resolvedEncoderId,
      resolvedDecoderId,
      requestedEncoderId: requestedEncoderIdFor(manifest),
      requestedDecoderId: requestedDecoderIdFor(manifest),
      defaultEncoderId: streamDefaults?.defaultEncoderId ?? null,
      selectedFormat: state.selectedFormat,
      decoderDefaultIdsByCaptureFormat: state.decoderDefaultIdsByCaptureFormat,
      encoderSelectionMode: state.encoderSelectionMode,
      decoderSelectionMode: state.decoderSelectionMode
    });
    const nextEncoderId = selections.encoderImpl;
    const nextDecoderId = selections.decoderImpl;
    state.encoderEnabled = typeof resolvedEncoder?.enabled === 'boolean'
      ? resolvedEncoder.enabled
      : requestedEncoderState === 'enabled'
        ? true
        : requestedEncoderState === 'disabled'
          ? false
          : typeof encoderEnabledFlag === 'boolean'
            ? Boolean(encoderEnabledFlag)
            : streamDefaults?.defaultEncoderEnabled ?? Boolean(nextEncoderId);
    state.decoderEnabled = typeof resolvedDecoder?.enabled === 'boolean'
      ? resolvedDecoder.enabled
      : requestedDecoderState === 'enabled'
        ? true
        : requestedDecoderState === 'disabled'
          ? false
        : streamDefaults?.defaultDecoderEnabled ?? Boolean(nextDecoderId);
    state.previewJpegQuality =
      asJpegQuality(manifest?.preview_jpeg_quality ?? manifestRecord?.preview_jpeg_quality) ??
      defaultPreviewJpegQualityForEncoder(streamDefaults, state.encoderEnabled) ??
      state.previewJpegQuality;
    if (encoderSettingsWire) {
      const parsedEncoderSettings = encoderSettingsDraftFromUnknown(encoderSettingsWire);
      state.encoderSettings = parsedEncoderSettings;
      state.encoderFpsLimit =
        deps.frameRateToFps(
          parsedEncoderSettings.framerateNum && parsedEncoderSettings.framerateDen
            ? { numerator: parsedEncoderSettings.framerateNum, denominator: parsedEncoderSettings.framerateDen }
            : null
        ) ?? null;
    } else {
      state.encoderSettings = createEncoderSettingsDraft();
      state.encoderFpsLimit = null;
    }
    applyEditorState(
      syncCameraStreamEditorCodecs({
        state: {
          ...editorStateFor(),
          encoderEnabled: state.encoderEnabled,
          decoderEnabled: state.decoderEnabled,
          encoderImpl: nextEncoderId,
          decoderImpl: nextDecoderId
        },
        encoders: state.encoders,
        decoders: state.decoders,
        resolvedEncoderId,
        resolvedDecoderId,
        requestedEncoderId: requestedEncoderIdFor(manifest),
        requestedDecoderId: requestedDecoderIdFor(manifest),
        defaultEncoderId: streamDefaults?.defaultEncoderId ?? null,
        decoderDefaultIdsByCaptureFormat: state.decoderDefaultIdsByCaptureFormat
      })
    );

    const pipelineEnabled = manifest?.pipeline_enabled ?? manifestRecord?.pipeline_enabled ?? false;
    state.selectedPipelineId =
      !pipelineEnabled ? null : asTrimmedString(manifestRecord?.active_pipeline_id ?? manifestRecord?.pipeline_id) || null;
    state.selectedPipelineOutput =
      !pipelineEnabled ? null : asTrimmedString(manifestRecord?.active_pipeline_output ?? manifestRecord?.pipeline_output) || null;

    const normalizedSelected = state.selectedPipelineId ? String(state.selectedPipelineId).trim() : '';
    state.selectedPipelineId = normalizedSelected.length ? normalizedSelected : null;
    if (state.selectedPipelineId === RAW_PIPELINE_UUID) {
      state.selectedPipelineId = RAW_PIPELINE_ID;
    }
    if (
      state.selectedPipelineId === RAW_PIPELINE_ID &&
      typeof state.selectedPipelineOutput === 'string' &&
      state.selectedPipelineOutput.trim().toLowerCase() === 'frame'
    ) {
      state.selectedPipelineOutput = 'raw';
    }

    const layout = manifestRecord?.pipeline_layout ?? null;
    const pipelines = manifestRecord?.pipelines;
    const parsedLayout = deps.parseManifestLayout(layout);
    if (pipelineEnabled && parsedLayout && parsedLayout.rows === 1 && parsedLayout.columns === 1) {
      const outputCellId = parsedLayout.slots[PIPELINE_OUTPUT_CELL_KEY];
      const fallbackEntry = Object.entries(parsedLayout.slots).find(([, id]) => typeof id === 'string' && id.trim().length);
      const selectedSlotKey =
        typeof outputCellId === 'string' && outputCellId.trim().length
          ? PIPELINE_OUTPUT_CELL_KEY
          : (fallbackEntry?.[0] ?? null);
      const selectedSlotId =
        typeof outputCellId === 'string' && outputCellId.trim().length
          ? outputCellId.trim()
          : (typeof fallbackEntry?.[1] === 'string' && fallbackEntry[1].trim().length ? fallbackEntry[1].trim() : null);

      if (selectedSlotId) {
        state.selectedPipelineId = selectedSlotId;
        const slotOutput =
          selectedSlotKey && typeof parsedLayout.outputKeys[selectedSlotKey] === 'string'
            ? String(parsedLayout.outputKeys[selectedSlotKey]).trim()
            : '';
        if (slotOutput.length) {
          state.selectedPipelineOutput = slotOutput;
        }
      }
      if (
        state.selectedPipelineId === RAW_PIPELINE_ID &&
        typeof state.selectedPipelineOutput === 'string' &&
        state.selectedPipelineOutput.trim().toLowerCase() === 'frame'
      ) {
        state.selectedPipelineOutput = 'raw';
      }
    }
    const currentLayoutSignature = layoutSignature(
      state.pipelineGridRows,
      state.pipelineGridColumns,
      state.pipelineGridSlots,
      state.pipelineGridSlotOutputKeys
    );
    const manifestLayoutSignature = parsedLayout
      ? layoutSignature(parsedLayout.rows, parsedLayout.columns, parsedLayout.slots, parsedLayout.outputKeys)
      : null;
    const gridHasSlots = Object.values(state.pipelineGridSlots ?? {}).some(Boolean);
    const gridIsMultiplex = Math.trunc(state.pipelineGridRows) * Math.trunc(state.pipelineGridColumns) > 1;
    const hasUiLayout =
      state.pipelineLayoutTouched ||
      state.pipelineAssignmentsTouched ||
      gridIsMultiplex ||
      gridHasSlots ||
      (state.assignedPipelineIds?.length ?? 0) > 0;
    const hasManifestPipelines =
      Boolean(state.selectedPipelineId) ||
      (layout && typeof layout === 'object') ||
      (Array.isArray(pipelines) && pipelines.length > 0);
    if (!hasManifestPipelines) {
      if (!hasUiLayout) {
        state.pipelineGridRows = 1;
        state.pipelineGridColumns = 1;
        state.pipelineGridSlots = { [PIPELINE_OUTPUT_CELL_KEY]: RAW_PIPELINE_ID };
        state.pipelineGridSlotOutputKeys = {};
        state.assignedPipelineIds = [];
        state.pipelineAssignDraft = [];
        state.pipelineOutputByPipelineId = {};
        state.selectedPipelineId = RAW_PIPELINE_ID;
        state.selectedPipelineOutput = 'raw';
        state.pipelineLayoutTouched = false;
        state.pipelineAssignmentsTouched = false;
      }
      return;
    }

    if (manifestLayoutSignature && manifestLayoutSignature === currentLayoutSignature) {
      state.pipelineLayoutTouched = false;
    }

    try {
      if (parsedLayout && !state.pipelineLayoutTouched) {
        state.pipelineGridRows = parsedLayout.rows;
        state.pipelineGridColumns = parsedLayout.columns;
        state.pipelineGridSlots = parsedLayout.slots;
        state.pipelineGridSlotOutputKeys = parsedLayout.outputKeys;
        state.pipelineLayoutTouched = false;
      }

      let manifestPipelineIds: string[] = [];
      let normalizedOutputs: Record<string, string | null> | null = null;
      if (Array.isArray(pipelines)) {
        manifestPipelineIds = pipelines
          .map((p) => {
            const record = asRecord(p);
            if (!record) return '';
            const raw =
              typeof record.pipeline_id === 'string'
                ? record.pipeline_id
                : typeof record.pipelineId === 'string'
                  ? record.pipelineId
                  : typeof record.id === 'string'
                    ? record.id
                    : '';
            const id = raw.trim();
            return id === RAW_PIPELINE_UUID ? RAW_PIPELINE_ID : id;
          })
          .filter(Boolean);
        const outputs: Record<string, string | null> = {};
        pipelines.forEach((p) => {
          const record = asRecord(p);
          if (!record) return;
          let id =
            typeof record.pipeline_id === 'string'
              ? record.pipeline_id.trim()
              : typeof record.pipelineId === 'string'
                ? record.pipelineId.trim()
                : typeof record.id === 'string'
                  ? record.id.trim()
                  : '';
          if (id === RAW_PIPELINE_UUID) id = RAW_PIPELINE_ID;
          if (!id.length) return;
          const out = typeof record.pipeline_output === 'string' ? record.pipeline_output.trim() : '';
          outputs[id] = out.length ? out : null;
        });
        normalizedOutputs = normalizePipelineOutputMap(outputs);
      }
      const layoutPipelineIds = Object.values(state.pipelineGridSlots ?? {})
        .map((id) => (typeof id === 'string' ? id.trim() : ''))
        .filter(Boolean);
      const baseAssigned = normalizeAssignedPipelineIds([...manifestPipelineIds, ...layoutPipelineIds]);
      const previousAssigned = normalizeAssignedPipelineIds(state.assignedPipelineIds);
      const assignmentSignature = (ids: string[]) => normalizeAssignedPipelineIds(ids).slice().sort().join('|');
      if (state.pipelineAssignmentsTouched && assignmentSignature(baseAssigned) === assignmentSignature(previousAssigned)) {
        state.pipelineAssignmentsTouched = false;
      }
      const preferLocalAssignments = state.pipelineAssignmentsTouched;
      const mergedAssigned = preferLocalAssignments
        ? normalizeAssignedPipelineIds([...previousAssigned, ...layoutPipelineIds])
        : state.pipelineLayoutTouched
          ? normalizeAssignedPipelineIds([...baseAssigned, ...previousAssigned])
          : baseAssigned;
      state.assignedPipelineIds = mergedAssigned;
      state.pipelineAssignDraft = normalizeAssignedPipelineIds(state.assignedPipelineIds);
      const outputBase = normalizedOutputs ? { ...state.pipelineOutputByPipelineId, ...normalizedOutputs } : state.pipelineOutputByPipelineId;
      state.pipelineOutputByPipelineId = Object.fromEntries(
        Object.entries(outputBase).filter(([id]) => mergedAssigned.includes(id))
      );
    } catch (err) {
      console.warn('Failed to hydrate pipeline layout from manifest', err);
    }

    if (state.selectedPipelineId) {
      if (!state.pipelineAssignmentsTouched) {
        state.assignedPipelineIds = normalizeAssignedPipelineIds([state.selectedPipelineId, ...state.assignedPipelineIds]);
        state.pipelineAssignDraft = normalizeAssignedPipelineIds(state.assignedPipelineIds);
      } else if (!state.assignedPipelineIds.includes(state.selectedPipelineId)) {
        state.selectedPipelineId = null;
        state.selectedPipelineOutput = null;
      }
      if (state.selectedPipelineId && state.selectedPipelineOutput) {
        deps.setOutputSelectionForPipeline(state.selectedPipelineId, state.selectedPipelineOutput);
      }
    }

    if (state.selectedPipelineId && !Object.values(state.pipelineGridSlots).some(Boolean)) {
      state.pipelineGridSlots = { ...state.pipelineGridSlots, [PIPELINE_OUTPUT_CELL_KEY]: state.selectedPipelineId };
    }
  }

  function currentDevice(): ProbedDevice | null {
    return state.selectedDeviceIndex >= 0 ? state.devices[state.selectedDeviceIndex] ?? null : null;
  }

  function currentBackend(): ProbedBackend | null {
    return currentDevice()?.backends?.[state.selectedBackendIndex] ?? null;
  }

  return {
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
  };
}
