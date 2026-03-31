import type {
  CodecInfo,
  ProbedBackend,
  ProbedDevice,
  StreamInfo,
  StreamManifest,
  Mode,
  Interval
} from '$lib/api/httpClient';
import type { StreamsApi } from '$lib/api/streamsApi';
import {
  decoderPreferencesForFormat,
  dedupeCodecs,
  normalizeDecoderDefaultIdsByCaptureFormat,
  pickCodecId
} from './cameraBackendCodecs';
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
import { layoutSignature } from './cameraPipelineState';
import {
  PIPELINE_OUTPUT_CELL_KEY,
  RAW_PIPELINE_ID,
  RAW_PIPELINE_UUID,
  normalizeAssignedPipelineIds,
  normalizePipelineOutputMap
} from './cameraPipelineTuningController';

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
  get encoderSelectionTouched(): boolean;
  set encoderSelectionTouched(value: boolean);
  get decoderSelectionTouched(): boolean;
  set decoderSelectionTouched(value: boolean);
  get encoderSettings(): {
    bitrate: number | null;
    gop: number | null;
    threadCount: number | null;
    outWidth: number | null;
    outHeight: number | null;
  };
  set encoderSettings(value: { bitrate: number | null; gop: number | null; threadCount: number | null; outWidth: number | null; outHeight: number | null });
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
  get selectedInterval(): string;
  set selectedInterval(value: string);
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
  const requestedEncoderSettingsFor = (manifest?: StreamManifest | null): Record<string, unknown> | null =>
    asRecord(requestedEncoderRecordFor(manifest)?.settings) ?? asRecord(asRecord(manifest)?.encoder_settings);
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
      const defaultEncoderId = asTrimmedString(runtimeStatus?.streams?.capabilities?.defaults?.defaultEncoderId);
      state.codecs = Array.isArray(list) ? list : [];
      state.decoderDefaultIdsByCaptureFormat = normalizeDecoderDefaultIdsByCaptureFormat(
        runtimeStatus?.streams?.capabilities?.defaults?.defaultDecoderIdsByCaptureFormat
      );
      const allEncoders = dedupeCodecs(
        state.codecs.filter((c) => String(c.kind).toLowerCase() === 'encoder'),
        (c) => `${c.name}::${c.implementation}::${c.input}::${c.output}`
      );
      state.encoders = dedupeCodecs(allEncoders, (c) => `${c.name}::${c.implementation}`);

      state.decoders = dedupeCodecs(
        state.codecs.filter((c) => String(c.kind).toLowerCase() === 'decoder'),
        (c) => `${c.name}::${c.implementation}::${c.input}::${c.output}`
      );

      state.encoderImpl = pickCodecId(
        state.encoders,
        resolvedEncoderId || requestedEncoderIdFor(manifest),
        defaultEncoderId ? [defaultEncoderId] : []
      );
      state.decoderImpl = pickCodecId(
        state.decoders,
        resolvedDecoderId || requestedDecoderIdFor(manifest),
        decoderPreferencesForFormat(
          manifest?.capture?.mode?.format?.code ?? null,
          state.decoderDefaultIdsByCaptureFormat
        )
      );
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
    const resolvedEncoderId = resolvedCodecId(resolvedEncoder);
    const resolvedDecoderId = resolvedCodecId(resolvedDecoder);
    const requestedEncoderState = requestedEncoderStateFor(manifest);
    const encoderSettingsWire = requestedEncoderSettingsFor(manifest);
    const requestedDecoderState = requestedDecoderStateFor(manifest);
    const decoderSettingsWire = requestedDecoderSettingsFor(manifest);
    if (manifest) {
      state.decoderFpsLimit = deps.normalizeFpsLimit(decoderSettingsWire?.fps_limit ?? encoderSettingsWire?.decode_fps_limit ?? null);
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
    state.selectedModeKey = deps.modeKey(captureModeRecord ?? null) ?? deps.modeKey(mode?.id) ?? null;
    state.selectedFormat = deps.modeFormat(mode);
    state.selectedResolution = deps.modeResolution(mode);
    const intervals = deps.intervalsForSelection();
    const wantedInterval = asInterval(captureRecord?.interval ?? captureModeRecord?.interval ?? null);
    if (wantedInterval) {
      const idx = intervals.findIndex((int) => int?.numerator === wantedInterval?.numerator && int?.denominator === wantedInterval?.denominator);
      state.selectedIntervalIdx = idx >= 0 ? idx : 0;
    } else {
      state.selectedIntervalIdx = 0;
    }
    state.selectedInterval = intervals[state.selectedIntervalIdx] ? deps.fpsLabel(intervals[state.selectedIntervalIdx]) : '';
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

    state.hostBuffer = manifest?.host_buffer ?? state.hostBuffer;
    state.previewJpegQuality = asJpegQuality(manifest?.preview_jpeg_quality ?? manifestRecord?.preview_jpeg_quality) ?? 65;
    state.shadowRecorderEnabled = isFileBackend(capture?.backend) ? false : (manifest?.shadow_recorder_enabled ?? false);
    state.cameraAlias = asTrimmedString(identityRecord?.alias ?? identityRecord?.display);
    const encoderEnabledFlag = manifestRecord?.encoder_enabled;
    const nextEncoderId = pickCodecId(
      state.encoders,
      resolvedEncoderId || requestedEncoderIdFor(manifest) || state.encoderImpl
    );
    const nextDecoderId = pickCodecId(
      state.decoders,
      resolvedDecoderId || requestedDecoderIdFor(manifest) || state.decoderImpl,
      decoderPreferencesForFormat(state.selectedFormat, state.decoderDefaultIdsByCaptureFormat)
    );
    state.encoderEnabled = typeof resolvedEncoder?.enabled === 'boolean'
      ? resolvedEncoder.enabled
      : requestedEncoderState === 'enabled'
        ? true
        : requestedEncoderState === 'disabled'
          ? false
          : typeof encoderEnabledFlag === 'boolean'
            ? Boolean(encoderEnabledFlag)
            : Boolean(nextEncoderId);
    state.decoderEnabled = typeof resolvedDecoder?.enabled === 'boolean'
      ? resolvedDecoder.enabled
      : requestedDecoderState === 'enabled'
        ? true
        : requestedDecoderState === 'disabled'
          ? false
        : Boolean(nextDecoderId);
    if (encoderSettingsWire) {
      const outputResolution = asRecord(encoderSettingsWire.output_resolution);
      state.encoderSettings = {
        bitrate: asFiniteNumber(encoderSettingsWire.bitrate),
        gop: asFiniteNumber(encoderSettingsWire.gop),
        threadCount: asFiniteNumber(encoderSettingsWire.thread_count),
        outWidth: asFiniteNumber(outputResolution?.width),
        outHeight: asFiniteNumber(outputResolution?.height)
      };
      state.encoderFpsLimit = deps.frameRateToFps(encoderSettingsWire.framerate) ?? null;
    } else {
      state.encoderFpsLimit = null;
    }
    if (state.encoders.length && !state.encoderSelectionTouched) {
      state.encoderImpl = nextEncoderId;
    }
    if (state.decoders.length && !state.decoderSelectionTouched) {
      state.decoderImpl = nextDecoderId;
    }

    const pipelineEnabled = manifestRecord?.pipeline_enabled;
    state.selectedPipelineId =
      pipelineEnabled === false ? null : asTrimmedString(manifestRecord?.active_pipeline_id ?? manifestRecord?.pipeline_id) || null;
    state.selectedPipelineOutput =
      pipelineEnabled === false ? null : asTrimmedString(manifestRecord?.active_pipeline_output ?? manifestRecord?.pipeline_output) || null;

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
    if (pipelineEnabled !== false && parsedLayout && parsedLayout.rows === 1 && parsedLayout.columns === 1) {
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
