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
import { layoutSignature, normalizeGridOutputKeys, normalizeGridSlots } from './cameraPipelineState';
import {
  PIPELINE_OUTPUT_CELL_KEY,
  RAW_PIPELINE_ID,
  RAW_PIPELINE_UUID,
  normalizeAssignedPipelineIds,
  normalizePipelineOutputMap
} from './cameraPipelineTuningController';

export type CalibrationParams = {
  fx: number;
  fy: number;
  cx: number;
  cy: number;
  k1: number;
  k2: number;
  p1: number;
  p2: number;
  k3: number;
  undistortIters: number;
};

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
  mediaFormatMatches: (mode: Mode | null | undefined, value: any) => boolean;
  intervalsForSelection: () => Interval[];
  fpsLabel: (interval: Interval | undefined | null) => string;
  intervalToFps: (interval: Interval | null | undefined) => number | null;
  frameRateToFps: (rate: any) => number | null;
  normalizeFpsLimit: (value: any) => number | null;
  normalizeRotationDegrees: (value: any) => number | null;
  decodersForCaptureFormat: (fmt: string | null | undefined) => CodecInfo[];
  parseManifestLayout: (layout: unknown) => { rows: number; columns: number; slots: Record<string, string | null>; outputKeys: Record<string, string | null> } | null;
  outputSelectionForPipeline: (pipelineId: string) => string | null;
  setOutputSelectionForPipeline: (pipelineId: string, output: string | null) => void;
  extractValue: (value: any) => number | boolean | null;
  DEFAULT_LIBCAMERA_TARGET_FPS: number;
};

export function createCameraBackendController(state: BackendState, deps: BackendDeps) {
  function isFileBackend(kind: unknown): boolean {
    return String(kind ?? '').toLowerCase() === 'file';
  }

  function isNetcamBackend(kind: unknown): boolean {
    return String(kind ?? '').toLowerCase() === 'netcam';
  }

  function extractFileHandle(handle: unknown): { fps?: number; loop_forever?: boolean; paths?: string[] } | null {
    if (!handle || typeof handle !== 'object') return null;
    if (String((handle as any).type ?? '').toLowerCase() !== 'file') return null;
    return handle as { fps?: number; loop_forever?: boolean; paths?: string[] };
  }

  function descriptorFromStreamOrManifest(manifest?: StreamManifest | null): { modes: Mode[]; controls: any[] } {
    const streamDescriptor: any = state.stream?.descriptor ?? null;
    const streamModes: Mode[] = Array.isArray(streamDescriptor?.modes) ? (streamDescriptor.modes.filter(Boolean) as Mode[]) : [];
    if (streamModes.length) {
      return { modes: streamModes, controls: Array.isArray(streamDescriptor?.controls) ? streamDescriptor.controls : [] };
    }

    const captureMode: any = (manifest as any)?.capture?.mode ?? null;
    if (captureMode) {
      // Streams can be "stopped" after a restart and come back with an empty descriptor,
      // but the manifest still has a capture mode. Synthesize a minimal descriptor so
      // format/resolution selection (and Apply/Start) still work.
      const synthesized: any = {
        id: captureMode,
        format: captureMode?.format ?? null,
        intervals: captureMode?.interval ? [captureMode.interval] : [],
        interval_stepwise: null
      };
      return { modes: [synthesized as Mode], controls: Array.isArray(streamDescriptor?.controls) ? streamDescriptor.controls : [] };
    }

    return { modes: [], controls: Array.isArray(streamDescriptor?.controls) ? streamDescriptor.controls : [] };
  }

  function buildNetcamBackend(manifest?: StreamManifest | null): ProbedDevice | null {
    if (!isNetcamBackend(manifest?.capture?.backend)) return null;
    const descriptor = descriptorFromStreamOrManifest(manifest);
    const identity = (manifest as any)?.identity ?? null;
    const display =
      (typeof identity?.alias === 'string' && identity.alias.trim().length ? identity.alias.trim() : null) ??
      (typeof identity?.hardware_id === 'string' && identity.hardware_id.trim().length ? identity.hardware_id.trim() : null) ??
      'Netcam';
    const keys =
      Array.isArray(manifest?.capture?.device_keys) && manifest?.capture?.device_keys?.length
        ? manifest.capture.device_keys
        : Array.isArray(manifest?.identity?.keys) && manifest?.identity?.keys?.length
          ? manifest.identity.keys
          : ['netcam'];
    const backend: ProbedBackend = {
      descriptor,
      handle: (manifest?.capture?.handle ?? { type: 'netcam', url: keys[0] ?? '', width: 0, height: 0, fps: 30 }) as any,
      kind: 'Netcam',
      properties: []
    };
    return { identity: { display, keys }, backends: [backend] };
  }

  function buildFileBackend(manifest?: StreamManifest | null): ProbedDevice | null {
    if (!isFileBackend(manifest?.capture?.backend)) return null;
    const descriptor = descriptorFromStreamOrManifest(manifest);
    const identity = (manifest as any)?.identity ?? null;
    const display =
      (typeof identity?.alias === 'string' && identity.alias.trim().length ? identity.alias.trim() : null) ??
      (typeof identity?.hardware_id === 'string' && identity.hardware_id.trim().length ? identity.hardware_id.trim() : null) ??
      'Media library';
    const keys = Array.isArray(manifest?.identity?.keys) && manifest?.identity?.keys.length ? manifest.identity.keys : ['media-file'];
    const backend: ProbedBackend = {
      descriptor,
      handle: (manifest?.capture?.handle ?? { type: 'file', fps: 30, loop_forever: true, paths: [] }) as any,
      kind: 'File',
      properties: []
    };
    return { identity: { display, keys }, backends: [backend] };
  }

  async function loadBackends(manifest?: StreamManifest | null): Promise<void> {
    const netcamDevice = buildNetcamBackend(manifest);
    if (netcamDevice) {
      state.devices = [netcamDevice];
      state.selectedDeviceIndex = 0;
      state.selectedBackendIndex = 0;
      return;
    }
    const fileDevice = buildFileBackend(manifest);
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

  function dedupeCodecs(list: CodecInfo[], keyFn: (codec: CodecInfo) => string): CodecInfo[] {
    const seen = new Set<string>();
    const out: CodecInfo[] = [];
    for (const codec of list) {
      const key = keyFn(codec);
      if (seen.has(key)) continue;
      seen.add(key);
      out.push(codec);
    }
    return out;
  }

  function codecSelectionId(codec: CodecInfo): string | null {
    const name = String(codec.name ?? '').trim();
    const implementation = String(codec.implementation ?? '').trim();
    return implementation || name || null;
  }

  function pickCodecId(
    list: CodecInfo[],
    desired: string | null | undefined,
    preferred: string[] = [],
  ): string | null {
    if (!list.length) return null;
    const wanted = typeof desired === 'string' ? desired.trim() : '';
    if (wanted) {
      const match = list.find((c) => c.implementation === wanted) ?? list.find((c) => c.name === wanted);
      if (match) return codecSelectionId(match);
    }
    for (const pref of preferred) {
      const key = String(pref ?? '').trim();
      if (!key) continue;
      const match = list.find((c) => c.implementation === key) ?? list.find((c) => c.name === key);
      if (match) return codecSelectionId(match);
    }
    return list[0] ? codecSelectionId(list[0]) : null;
  }

  async function loadCodecs(manifest?: StreamManifest | null): Promise<void> {
    try {
      const list = await deps.streamsApi.listCodecs();
      state.codecs = Array.isArray(list) ? list : [];
      const allEncoders = dedupeCodecs(
        state.codecs.filter((c) => String(c.kind).toLowerCase() === 'encoder'),
        (c) => `${c.name}::${c.implementation}::${c.input}::${c.output}`
      );
      const preferredEncoders = allEncoders.filter((c) => c.input === 'RG24');
      state.encoders = preferredEncoders.length
        ? dedupeCodecs(preferredEncoders, (c) => `${c.name}::${c.implementation}`)
        : dedupeCodecs(allEncoders, (c) => `${c.name}::${c.implementation}`);

      state.decoders = dedupeCodecs(
        state.codecs.filter((c) => String(c.kind).toLowerCase() === 'decoder'),
        (c) => `${c.name}::${c.implementation}::${c.input}::${c.output}`
      );

      state.encoderImpl = pickCodecId(state.encoders, manifest?.encoder_id, ['mjpeg']);
      state.decoderImpl = pickCodecId(state.decoders, manifest?.decoder_id, ['mono8-replicate']);
    } catch (err) {
      console.warn('Failed to load codec catalog', err);
    }
  }

  function applyManifestSelections(manifest?: StreamManifest | null): void {
    const capture = manifest?.capture;
    if (manifest) {
      const decoderSettings = manifest.decoder_settings ?? null;
      const encoderSettingsWire = manifest.encoder_settings ?? null;
      state.decoderFpsLimit = deps.normalizeFpsLimit(decoderSettings?.fps_limit ?? encoderSettingsWire?.decode_fps_limit ?? null);
      state.decoderRotationDegrees = deps.normalizeRotationDegrees(decoderSettings?.rotation_degrees ?? 0);
      state.decoderMirrorHorizontal = Boolean(decoderSettings?.mirror_horizontal ?? false);
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
    const wantedFormat = (capture as any)?.mode?.format ?? null;
    const mode = modes.find((m) => deps.mediaFormatMatches(m, wantedFormat)) ?? modes[0] ?? null;
    state.selectedModeKey = deps.modeKey((capture as any)?.mode) ?? deps.modeKey(mode?.id) ?? null;
    state.selectedFormat = deps.modeFormat(mode);
    state.selectedResolution = deps.modeResolution(mode);
    const intervals = deps.intervalsForSelection();
    const wantedInterval = (capture as any)?.interval ?? (capture as any)?.mode?.interval ?? null;
    if (wantedInterval) {
      const idx = intervals.findIndex((int) => int?.numerator === wantedInterval?.numerator && int?.denominator === wantedInterval?.denominator);
      state.selectedIntervalIdx = idx >= 0 ? idx : 0;
    } else {
      state.selectedIntervalIdx = 0;
    }
    state.selectedInterval = intervals[state.selectedIntervalIdx] ? deps.fpsLabel(intervals[state.selectedIntervalIdx]) : '';
    const isLibcamera = String(capture?.backend ?? '').toLowerCase() === 'libcamera';
    state.libcameraTargetFps =
      (capture as any)?.target_fps ??
      deps.intervalToFps(wantedInterval) ??
      deps.intervalToFps(intervals[state.selectedIntervalIdx]) ??
      (isLibcamera ? deps.DEFAULT_LIBCAMERA_TARGET_FPS : null);
    const netcamHandleFps = (() => {
      const handle = capture?.handle as any;
      const fps = Number(handle?.fps);
      if (String(handle?.type ?? '').toLowerCase() === 'netcam' && Number.isFinite(fps) && fps > 0) return Math.round(fps);
      const legacyFps = Number(handle?.Netcam?.fps);
      if (handle?.Netcam && Number.isFinite(legacyFps) && legacyFps > 0) return Math.round(legacyFps);
      return null;
    })();
    state.netcamTargetFps = netcamHandleFps ?? deps.intervalToFps(wantedInterval) ?? deps.intervalToFps(intervals[state.selectedIntervalIdx]) ?? null;

    if (isFileBackend(capture?.backend)) {
      const fileHandle = extractFileHandle(capture?.handle);
      const fpsRaw = Number(fileHandle?.fps ?? 0);
      const fallbackFps = Number.isFinite(fpsRaw) && fpsRaw > 0 ? fpsRaw : null;
      const controlMeta = (currentBackend()?.descriptor?.controls ?? (state.stream as any)?.descriptor?.controls ?? []).find((ctrl) => ctrl.name === 'file.duration_ms');
      const controlValue = controlMeta?.id != null ? (capture?.controls ?? []).find((entry) => entry.id === controlMeta.id)?.value : null;
      const durationMs = deps.extractValue(controlValue);
      const durationFps = typeof durationMs === 'number' && durationMs > 0 ? Math.max(1, Math.round(1000 / durationMs)) : null;
      state.fileBackendFps = durationFps ?? fallbackFps;
      state.fileBackendLoop = fileHandle?.loop_forever ?? true;
      const paths = Array.isArray(fileHandle?.paths) ? fileHandle.paths.filter((p) => typeof p === 'string' && p.trim().length) : [];
      state.fileBackendPathsText = paths.join('\n');
    }

    state.hostBuffer = manifest?.host_buffer ?? state.hostBuffer;
    state.shadowRecorderEnabled = isFileBackend(capture?.backend) ? false : (manifest?.shadow_recorder_enabled ?? true);
    state.cameraAlias = String((manifest as any)?.identity?.alias ?? (manifest as any)?.identity?.display ?? '').trim();
    const encoderEnabledFlag = (manifest as any)?.encoder_enabled;
    const decoderEnabledFlag = (manifest as any)?.decoder_enabled;
    const isFileManifestBackend = isFileBackend(capture?.backend);
    const isNetcamManifestBackend = isNetcamBackend(capture?.backend);
    const isMediaManifestBackend = isFileManifestBackend || isNetcamManifestBackend;
    const hasMediaCodecs = isMediaManifestBackend && (state.encoders.length > 0 || state.decoders.length > 0);
    state.encoderEnabled = typeof encoderEnabledFlag === 'boolean'
      ? (isMediaManifestBackend && !encoderEnabledFlag && hasMediaCodecs ? true : encoderEnabledFlag)
      : manifest?.encoder_id != null || (isMediaManifestBackend && state.encoders.length > 0);
    state.decoderEnabled = typeof decoderEnabledFlag === 'boolean'
      ? (isMediaManifestBackend && !decoderEnabledFlag && hasMediaCodecs ? true : decoderEnabledFlag)
      : manifest?.decoder_id != null || (isMediaManifestBackend && state.decoders.length > 0);
    if (manifest?.encoder_settings) {
      state.encoderSettings = {
        bitrate: manifest.encoder_settings.bitrate ?? null,
        gop: manifest.encoder_settings.gop ?? null,
        threadCount: manifest.encoder_settings.thread_count ?? null,
        outWidth: manifest.encoder_settings.output_resolution?.width ?? null,
        outHeight: manifest.encoder_settings.output_resolution?.height ?? null
      };
      state.encoderFpsLimit = deps.frameRateToFps(manifest.encoder_settings.framerate) ?? null;
    } else {
      state.encoderFpsLimit = null;
    }
    if (!manifest?.encoder_id && state.encoders.length && !state.encoderSelectionTouched) {
      state.encoderImpl = pickCodecId(state.encoders, state.encoderImpl, ['mjpeg']);
    }
    if (!manifest?.decoder_id && state.decoders.length && !state.decoderSelectionTouched) {
      const compatible = deps.decodersForCaptureFormat(state.selectedFormat);
      state.decoderImpl = pickCodecId(compatible.length ? compatible : state.decoders, state.decoderImpl, ['mono8-replicate']);
    }

    const pipelineEnabled = (manifest as any)?.pipeline_enabled;
    state.selectedPipelineId =
      pipelineEnabled === false ? null : (manifest as any)?.active_pipeline_id ?? (manifest as any)?.pipeline_id ?? null;
    state.selectedPipelineOutput =
      pipelineEnabled === false ? null : (manifest as any)?.active_pipeline_output ?? (manifest as any)?.pipeline_output ?? null;

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

    const layout = (manifest as any)?.pipeline_layout ?? null;
    const pipelines = (manifest as any)?.pipelines;
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
          .map((p: any) => {
            const raw =
              typeof p?.pipeline_id === 'string'
                ? p.pipeline_id
                : typeof p?.pipelineId === 'string'
                  ? p.pipelineId
                  : typeof p?.id === 'string'
                    ? p.id
                    : '';
            const id = raw.trim();
            return id === RAW_PIPELINE_UUID ? RAW_PIPELINE_ID : id;
          })
          .filter(Boolean);
        const outputs: Record<string, string | null> = {};
        pipelines.forEach((p: any) => {
          let id =
            typeof p?.pipeline_id === 'string'
              ? p.pipeline_id.trim()
              : typeof p?.pipelineId === 'string'
                ? p.pipelineId.trim()
                : typeof p?.id === 'string'
                  ? p.id.trim()
                  : '';
          if (id === RAW_PIPELINE_UUID) id = RAW_PIPELINE_ID;
          if (!id.length) return;
          const out = typeof p?.pipeline_output === 'string' ? p.pipeline_output.trim() : '';
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
