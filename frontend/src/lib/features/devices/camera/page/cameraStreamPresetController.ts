import type { ProbedBackend, ProbedDevice, Mode, Interval, CodecInfo } from '$lib/api/httpClient';
import { OpenAPI, getHttpClientBase } from '$lib/api/httpClient';
import { extractError } from '$lib/api/errors';
import type { PipelinesApi } from '$lib/api/pipelinesApi';
import type { StreamsApi } from '$lib/api/streamsApi';
import { normalizeGridSlots } from './cameraPipelineState';
import { fpsToFrameRate } from './cameraStreamState';
import {
  PIPELINE_OUTPUT_CELL_KEY,
  RAW_LOOPBACK_GRAPH,
  RAW_PIPELINE_ID,
  RAW_PIPELINE_UUID,
  normalizeAssignedPipelineIds
} from './cameraPipelineTuningController';

type PresetState = {
  get streamId(): string;
  // This is an internal controller interface; keep the type loose so we can read through to
  // the latest manifest shape without fighting generated client types.
  get stream(): any | null;
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
  get encoderSettings(): any;
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
  decodersForCaptureFormat: (fmt: string | null | undefined) => CodecInfo[];
  pickCodecId: (list: CodecInfo[], desired: string | null | undefined, preferred?: string[]) => string | null;
  outputSelectionForPipeline: (pipelineId: string) => string | null;
  applyPipelineOverridesToGraph: (pipelineId: string, graph: any) => any;
  dropPipelineEverywhere: (pipelineId: string) => void;
  onExternalLayoutApplied?: () => void;
  reportError: (params: { title: string; error: unknown; fallback: string }) => void;
};

export function createCameraStreamPresetController(state: PresetState, deps: PresetDeps) {
  const SUPPORTED_FILE_MEDIA_EXTENSIONS = new Set([
    'bmp',
    'gif',
    'heic',
    'heif',
    'jpeg',
    'jpg',
    'png',
    'tif',
    'tiff',
    'webp',
    'h264',
    'avc',
    'h265',
    'hevc',
    'm4v',
    'mjpeg',
    'mjpg',
    'mov',
    'mp4',
    'mpe',
    'mpeg',
    'mpg',
    'webm',
    'wmv',
    'y4m'
  ]);

  function normalizeFileBackendPaths(raw: unknown): string[] {
    if (typeof raw !== 'string') return [];
    return Array.from(
      new Set(
        raw
          .split('\n')
          .map((value) => value.trim())
          .filter((value) => value.length > 0)
      )
    );
  }

  function isSupportedFileBackendPath(path: string): boolean {
    const value = String(path ?? '').trim();
    if (!value.length) return false;
    const extRaw = value.split('/').pop()?.split('\\').pop() ?? '';
    const ext = extRaw.includes('.') ? extRaw.slice(extRaw.lastIndexOf('.') + 1).toLowerCase() : '';
    return ext.length > 0 && SUPPORTED_FILE_MEDIA_EXTENSIONS.has(ext);
  }

  function selectStableHardwareId(device: ProbedDevice, stream: any | null): string | null {
    const existing = typeof stream?.manifest?.identity?.hardware_id === 'string' ? stream.manifest.identity.hardware_id.trim() : '';
    if (existing.length) return existing;

    const keys = Array.isArray(device.identity?.keys)
      ? device.identity.keys.map((value) => String(value ?? '').trim()).filter((value) => value.length > 0)
      : [];
    const slashKey = keys.find((value) => value.includes('/'));
    if (slashKey) return slashKey;
    const colonKey = keys.find((value) => value.includes(':'));
    if (colonKey) return colonKey;
    if (keys.length) return [...keys].sort((a, b) => a.localeCompare(b))[0];
    return null;
  }

  function ensureApiBase(): void {
    try {
      getHttpClientBase();
      return;
    } catch {
      // fall back to legacy injected base when local storage/env resolution fails
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
      const identity = {
        id: state.stream?.id ?? null,
        alias: state.cameraAlias.trim().length ? state.cameraAlias.trim() : null,
        hardware_id: selectStableHardwareId(device, state.stream)
      } as any;

      const normalizedAssigned = Array.from(new Set((state.assignedPipelineIds ?? []).map((id) => String(id).trim()).filter(Boolean)));
      let normalizedActive = state.selectedPipelineId ? String(state.selectedPipelineId).trim() : null;
      let normalizedLayoutSlots = normalizeGridSlots(state.pipelineGridRows, state.pipelineGridColumns, state.pipelineGridSlots);
      const layoutIds = Object.values(normalizedLayoutSlots).filter((id): id is string => typeof id === 'string' && id.trim().length > 0);
      const assignedUnion = Array.from(new Set([...normalizedAssigned, ...layoutIds]));
      const effectiveAssigned = normalizedActive && !assignedUnion.includes(normalizedActive) ? [normalizedActive, ...assignedUnion] : assignedUnion;
      const gridIsMultiplex = Math.trunc(state.pipelineGridRows) * Math.trunc(state.pipelineGridColumns) > 1;
      if (!normalizedActive && !gridIsMultiplex && effectiveAssigned.length) {
        normalizedActive = effectiveAssigned[0];
      }

      const pipelineAssignments: any[] = [];
      const missingPipelines: string[] = [];
      for (const pipelineId of effectiveAssigned) {
        if (pipelineId === RAW_PIPELINE_ID) {
          pipelineAssignments.push({
            pipeline_id: RAW_PIPELINE_UUID,
            pipeline_graph: null,
            pipeline_output: deps.outputSelectionForPipeline(RAW_PIPELINE_ID)
          });
          continue;
        }
        try {
          const doc = await deps.pipelinesApi.fetchGraph({ id: pipelineId });
          const graph = (doc as any)?.graph ?? null;
          if (!graph) throw new Error(`Pipeline graph missing: ${pipelineId}`);
          pipelineAssignments.push({
            pipeline_id: pipelineId,
            pipeline_graph: null,
            pipeline_output: deps.outputSelectionForPipeline(pipelineId)
          });
        } catch (err) {
          console.warn('Pipeline graph missing', pipelineId, err);
          missingPipelines.push(pipelineId);
        }
      }

      if (missingPipelines.length) {
        missingPipelines.forEach((id) => deps.dropPipelineEverywhere(id));
        state.assignedPipelineIds = normalizeAssignedPipelineIds(state.assignedPipelineIds.filter((id) => !missingPipelines.includes(id)));
        if (normalizedActive && missingPipelines.includes(normalizedActive)) {
          state.selectedPipelineId = null;
          state.selectedPipelineOutput = null;
        }
        deps.reportError({
          title: 'Missing pipeline graphs',
          error: new Error(`Removed ${missingPipelines.length} missing pipeline(s) from the layout.`),
          fallback: `Removed ${missingPipelines.length} missing pipeline(s) from the layout.`
        });
      }
      normalizedActive = state.selectedPipelineId ? String(state.selectedPipelineId).trim() : null;
      normalizedLayoutSlots = normalizeGridSlots(state.pipelineGridRows, state.pipelineGridColumns, state.pipelineGridSlots);

      // Keep layout slots/active selection consistent with the resolved assignment set so stale
      // IDs from rapid remove/apply cycles cannot reach the backend payload.
      const validPipelineIds = new Set(
        pipelineAssignments.map((binding) => (binding.pipeline_id === RAW_PIPELINE_UUID ? RAW_PIPELINE_ID : String(binding.pipeline_id)))
      );
      normalizedLayoutSlots = Object.fromEntries(
        Object.entries(normalizedLayoutSlots).map(([key, value]) => [key, value && validPipelineIds.has(value) ? value : null])
      );
      if (normalizedActive && !validPipelineIds.has(normalizedActive)) {
        normalizedActive = null;
      }

      let hasAnySlot = Object.values(normalizedLayoutSlots).some((v) => typeof v === 'string' && v.trim().length > 0);
      const hasAnyPipelineAssignment = pipelineAssignments.length > 0;
      if (!hasAnyPipelineAssignment) {
        normalizedActive = null;
        normalizedLayoutSlots = normalizeGridSlots(state.pipelineGridRows, state.pipelineGridColumns, {});
        hasAnySlot = false;
      }
      const multiplex = gridIsMultiplex;
      if (!multiplex && hasAnySlot) {
        const outputCell = normalizedLayoutSlots[PIPELINE_OUTPUT_CELL_KEY];
        const fallbackSlot =
          outputCell && outputCell.trim().length
            ? outputCell
            : Object.values(normalizedLayoutSlots).find((v) => typeof v === 'string' && v.trim().length > 0) ?? null;
        // In single-view mode the visible stream is always the output-cell pipeline.
        // Keep `active` pinned to that slot so apply/start payloads can't drift to another pipeline.
        if (typeof fallbackSlot === 'string' && fallbackSlot.trim().length) {
          normalizedActive = fallbackSlot;
        }
      }
      if (normalizedActive && !hasAnySlot && !multiplex) {
        normalizedLayoutSlots = { ...normalizedLayoutSlots, [PIPELINE_OUTPUT_CELL_KEY]: normalizedActive };
        hasAnySlot = true;
      }
      // Enable pipeline mode whenever the user is in multiplex, has any slot assignments (including raw),
      // has an active pipeline selected, or has any non-raw pipelines assigned.
      //
      // This is important for multiplex: when the grid is >1x1 but no slots are assigned, we should
      // render an empty canvas (no implicit raw fallback). New streams still default to 1x1 with raw
      // assigned, so the initial view isn't black.
      const enablePipeline = multiplex || Boolean(normalizedActive) || hasAnySlot || hasAnyPipelineAssignment;
      const activePipelineIdWire = enablePipeline
        ? normalizedActive === RAW_PIPELINE_ID
          ? RAW_PIPELINE_UUID
          : normalizedActive && normalizedActive !== RAW_PIPELINE_ID
            ? normalizedActive
            : null
        : null;
      const activePipelineOutputWire = enablePipeline && normalizedActive ? deps.outputSelectionForPipeline(normalizedActive) : null;

      const pipelineLayoutSlots = Object.entries(normalizedLayoutSlots)
        .map(([key, pipelineId]) => {
          if (!pipelineId) return null;
          const [rRaw, cRaw] = key.split(':');
          const row = Number(rRaw);
          const column = Number(cRaw);
          if (!Number.isInteger(row) || !Number.isInteger(column)) return null;
          let output_key = state.pipelineGridSlotOutputKeys[key] ?? null;
          if ((!output_key || !String(output_key).trim().length) && pipelineId) {
            output_key = deps.outputSelectionForPipeline(pipelineId);
          }
          output_key = typeof output_key === 'string' && output_key.trim().length ? output_key.trim() : null;
          if (pipelineId === RAW_PIPELINE_ID) {
            if (typeof output_key === 'string' && output_key.trim().toLowerCase() === 'frame') {
              output_key = 'raw';
            }
            return { row, column, pipeline_id: RAW_PIPELINE_UUID, output_key };
          }
          return { row, column, pipeline_id: pipelineId, output_key };
        })
        .filter(Boolean);

      const isFileBackend = String((backend as any)?.kind ?? '')
        .trim()
        .toLowerCase() === 'file';
      // Shadow recorder (rolling buffer) should stay off for media/file streams by default.
      const shadowRecorderEnabled = !isFileBackend && state.shadowRecorderEnabled;

      const encoderId = (() => {
        if (!state.encoderEnabled) return null;
        // When shadow recording is enabled and the user hasn't intentionally selected an encoder,
        // let backend defaults choose the safer CPU profile (h264) instead of inheriting stale codec state.
        if (shadowRecorderEnabled && !state.encoderSelectionTouched) return null;
        return state.encoderImpl;
      })();
      const decoderId = (() => {
        if (!state.decoderEnabled) return null;
        const selected = String(state.decoderImpl ?? '').trim();
        if (!selected.length) return null;
        const compatibleDecoders = deps.decodersForCaptureFormat(state.selectedFormat);
        const decoderPool = compatibleDecoders.length ? compatibleDecoders : state.decoders;
        const resolved = deps.pickCodecId(decoderPool, selected, []);
        if (!resolved) {
          console.warn('Selected decoder unavailable for capture format; disabling decoder for apply', {
            selected,
            format: state.selectedFormat
          });
          return null;
        }
        if (resolved !== selected) {
          console.warn('Selected decoder was not compatible; using first compatible decoder', {
            selected,
            resolved,
            format: state.selectedFormat
          });
        }
        return resolved;
      })();
      const decoderEnabled = state.decoderEnabled && Boolean(decoderId);
      const modeWidth = Number((mode as any)?.format?.resolution?.width ?? 0);
      const modeHeight = Number((mode as any)?.format?.resolution?.height ?? 0);
      const defaultEncoderOutputResolution =
        Number.isFinite(modeWidth) && Number.isFinite(modeHeight) && modeWidth > 0 && modeHeight > 0
          ? { width: Math.max(1, Math.trunc(modeWidth / 2)), height: Math.max(1, Math.trunc(modeHeight / 2)) }
          : null;

      const backendKind = String((backend as any)?.kind ?? '');
      const normalizedBackendKind = backendKind.trim().toLowerCase();
      let captureHandle: any = backend.handle;
      if (normalizedBackendKind === 'file') {
        const backendFileHandle = backend.handle as { paths?: unknown; fps?: unknown; loop_forever?: unknown } | null;
        const parsedPaths = normalizeFileBackendPaths(state.fileBackendPathsText);
        const unsupportedPaths = parsedPaths.filter((value) => !isSupportedFileBackendPath(value));
        if (unsupportedPaths.length) {
          deps.reportError({
            title: 'Unsupported media path type',
            error: new Error(`Unsupported media file extension for ${unsupportedPaths.length} path(s).`),
            fallback: 'Only image/video files are supported for File backend replay.'
          });
          return;
        }
        const fallbackPaths = Array.isArray(backendFileHandle?.paths)
          ? backendFileHandle.paths
              .map((value) => (typeof value === 'string' ? value.trim() : ''))
              .filter((value): value is string => value.length > 0)
              .filter((value) => isSupportedFileBackendPath(value))
          : [];
        const resolvedPaths = parsedPaths.length ? parsedPaths : fallbackPaths;
        if (resolvedPaths.length === 0) {
          deps.reportError({
            title: 'Missing media files',
            error: new Error('File backend apply requires at least one valid media file path.'),
            fallback: 'Add at least one image/video media file path before applying.'
          });
          return;
        }
        const parsedFps = Number(state.fileBackendFps ?? NaN);
        const fallbackFps = Number(backendFileHandle?.fps ?? NaN);
        const fps = Number.isFinite(parsedFps) && parsedFps > 0
          ? Math.max(1, Math.round(parsedFps))
          : Number.isFinite(fallbackFps) && fallbackFps > 0
            ? Math.max(1, Math.round(fallbackFps))
            : 30;
        captureHandle = {
          type: 'file',
          fps,
          loop_forever: Boolean(state.fileBackendLoop),
          paths: resolvedPaths
        };
      }

      const payload = {
        identity,
        capture: {
          backend: backend.kind,
          handle: captureHandle,
          mode: mode,
          interval,
          target_fps: backendKind === 'Libcamera' ? state.libcameraTargetFps : backendKind === 'Netcam' ? state.netcamTargetFps : null,
          controls: [],
          device_keys: device.identity?.keys ?? []
        },
        encoder_enabled: state.encoderEnabled,
        encoder_id: encoderId,
        decoder_enabled: decoderEnabled,
        decoder_id: decoderEnabled ? decoderId : null,
        encoder_settings: state.encoderEnabled
          ? {
              bitrate: state.encoderSettings?.bitrate ?? null,
              gop: state.encoderSettings?.gop ?? null,
              thread_count: state.encoderSettings?.threadCount ?? null,
              framerate: fpsToFrameRate(state.encoderFpsLimit),
              output_resolution:
                state.encoderSettings?.outWidth && state.encoderSettings?.outHeight
                  ? { width: state.encoderSettings.outWidth, height: state.encoderSettings.outHeight }
                  : defaultEncoderOutputResolution
            }
          : null,
        decoder_settings: decoderEnabled
          ? {
              fps_limit: state.decoderFpsLimit ?? null,
              rotation_degrees: state.decoderRotationDegrees ?? 0,
              mirror_horizontal: state.decoderMirrorHorizontal ?? false
            }
          : null,
        shadow_recorder_enabled: shadowRecorderEnabled,
        host_buffer: state.hostBuffer ?? null,
        pipeline_enabled: enablePipeline,
        pipeline_id: activePipelineIdWire,
        pipeline_output: activePipelineOutputWire ?? null,
        pipeline_graph: null,
        pipeline_layout: enablePipeline ? { rows: state.pipelineGridRows, columns: state.pipelineGridColumns, slots: pipelineLayoutSlots } : null,
        // Preserve any pipeline wiring (output -> input) when re-applying presets, unless the user
        // explicitly disables pipelines.
        pipeline_wires: enablePipeline
          ? (Array.isArray((state.stream?.manifest as any)?.pipeline_wires) ? (state.stream?.manifest as any).pipeline_wires : [])
          : [],
        pipelines: pipelineAssignments
      } as any;

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
