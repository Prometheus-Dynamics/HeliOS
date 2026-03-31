import type { CodecInfo, Interval, Mode, ProbedBackend, ProbedDevice, StreamInfo, StreamManifest } from '$lib/api/httpClient';
import { OpenAPI, getHttpClientBase } from '$lib/api/httpClient';
import { extractError } from '$lib/api/errors';
import type { PipelinesApi } from '$lib/api/pipelinesApi';
import type { StreamsApi } from '$lib/api/streamsApi';
import { normalizeGridSlots } from './cameraPipelineState';
import { fpsToFrameRate } from './cameraStreamState';
import {
  PIPELINE_OUTPUT_CELL_KEY,
  RAW_PIPELINE_ID,
  RAW_PIPELINE_UUID
} from './cameraPipelineTuningController';

type PresetState = {
  get streamId(): string;
  get stream(): StreamInfo | null;
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
  get encoderSettings(): {
    bitrate: number | null;
    gop: number | null;
    threadCount: number | null;
    outWidth: number | null;
    outHeight: number | null;
  };
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
  const asRecord = (value: unknown): Record<string, unknown> | null =>
    value && typeof value === 'object' ? (value as Record<string, unknown>) : null;

  const asPositiveNumber = (value: unknown): number | null => {
    const numeric = Number(value);
    return Number.isFinite(numeric) && numeric > 0 ? numeric : null;
  };

  const identityRecordFor = (manifest?: StreamManifest | null): Record<string, unknown> | null =>
    asRecord(manifest?.identity) ?? asRecord(asRecord(manifest)?.identity);

  function extractFileHandle(handle: unknown): { fps?: number; loop_forever?: boolean; paths?: string[] } | null {
    const record = asRecord(handle);
    if (!record) return null;
    const direct = String(record.type ?? '').toLowerCase() === 'file' ? record : null;
    const legacy = asRecord(record.File);
    const resolved = direct ?? legacy;
    if (!resolved) return null;
    return {
      fps: asPositiveNumber(resolved.fps) ?? undefined,
      loop_forever: typeof resolved.loop_forever === 'boolean' ? resolved.loop_forever : undefined,
      paths: Array.isArray(resolved.paths)
        ? resolved.paths.filter((path): path is string => typeof path === 'string' && path.trim().length > 0)
        : undefined
    };
  }

  function selectStableHardwareId(keys: unknown): string | null {
    if (!Array.isArray(keys)) return null;
    const normalized = keys
      .map((value) => (typeof value === 'string' ? value.trim() : ''))
      .filter((value): value is string => value.length > 0);
    const withSlash = normalized.find((value) => value.includes('/'));
    if (withSlash) return withSlash;
    const withColon = normalized.find((value) => value.includes(':'));
    if (withColon) return withColon;
    const sorted = [...normalized].sort();
    return sorted[0] ?? null;
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
      const manifestIdentity = identityRecordFor(state.stream?.manifest);
      const existingHardwareId =
        typeof manifestIdentity?.hardware_id === 'string' && manifestIdentity.hardware_id.trim().length > 0
          ? manifestIdentity.hardware_id.trim()
          : null;

      const identity = {
        id: state.stream?.id ?? null,
        alias: state.cameraAlias.trim().length ? state.cameraAlias.trim() : null,
        hardware_id: existingHardwareId ?? selectStableHardwareId(device.identity?.keys) ?? null
      } as unknown as StreamManifest['identity'];

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

      const pipelineAssignments = effectiveAssigned.map((pipelineId) =>
        pipelineId === RAW_PIPELINE_ID
          ? {
              pipeline_id: RAW_PIPELINE_UUID,
              pipeline_graph: null,
              pipeline_output: deps.outputSelectionForPipeline(RAW_PIPELINE_ID)
            }
          : {
              pipeline_id: pipelineId,
              pipeline_graph: null,
              pipeline_output: deps.outputSelectionForPipeline(pipelineId)
            }
      );

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
            return { row, column, pipeline_id: RAW_PIPELINE_UUID, output_key };
          }
          return { row, column, pipeline_id: pipelineId, output_key };
        })
        .filter(Boolean);

      const isFileBackend = String(backend.kind ?? '')
        .trim()
        .toLowerCase() === 'file';
      // Shadow recorder (rolling buffer) should stay off for media/file streams by default.
      const shadowRecorderEnabled = !isFileBackend && state.shadowRecorderEnabled;

      const encoderId = (() => {
        if (!state.encoderEnabled) return null;
        const selected = String(state.encoderImpl ?? '').trim();
        return selected.length ? selected : null;
      })();
      const decoderId = (() => {
        if (!state.decoderEnabled) return null;
        const selected = String(state.decoderImpl ?? '').trim();
        return selected.length ? selected : null;
      })();
      const decoderEnabled = state.decoderEnabled && Boolean(decoderId);
      const modeWidth = Number(mode.format?.resolution?.width ?? 0);
      const modeHeight = Number(mode.format?.resolution?.height ?? 0);
      const defaultEncoderOutputResolution =
        Number.isFinite(modeWidth) && Number.isFinite(modeHeight) && modeWidth > 0 && modeHeight > 0
          ? { width: Math.max(1, Math.trunc(modeWidth / 2)), height: Math.max(1, Math.trunc(modeHeight / 2)) }
          : null;

      const backendKind = String(backend.kind ?? '');
      const normalizedBackendKind = backendKind.trim().toLowerCase();
      let captureHandle: unknown = backend.handle;
      if (normalizedBackendKind === 'file') {
        const backendFileHandle = extractFileHandle(backend.handle);
        const parsedPaths = String(state.fileBackendPathsText ?? '')
          .split('\n')
          .map((value) => value.trim())
          .filter((value) => value.length > 0);
        const dedupedPaths = Array.from(new Set(parsedPaths));
        const fallbackPaths = Array.isArray(backendFileHandle?.paths)
          ? backendFileHandle.paths.filter((value): value is string => typeof value === 'string' && value.trim().length > 0)
          : [];
        const parsedFps = Number(state.fileBackendFps ?? NaN);
        const fallbackFps = Number(backendFileHandle?.fps ?? NaN);
        const fps = Number.isFinite(parsedFps)
          ? parsedFps
          : Number.isFinite(fallbackFps)
            ? fallbackFps
            : null;
        captureHandle = {
          type: 'file',
          fps,
          loop_forever: Boolean(state.fileBackendLoop),
          paths: dedupedPaths.length ? dedupedPaths : fallbackPaths
        };
      }

      const existingPipelineWires =
        Array.isArray(state.stream?.manifest?.pipeline_wires) ? state.stream.manifest.pipeline_wires : [];

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
        encoder: state.encoderEnabled
          ? {
              state: 'enabled',
              id: encoderId,
              settings: {
                bitrate: state.encoderSettings?.bitrate ?? null,
                gop: state.encoderSettings?.gop ?? null,
                thread_count: state.encoderSettings?.threadCount ?? null,
                framerate: fpsToFrameRate(state.encoderFpsLimit),
                output_resolution:
                  state.encoderSettings?.outWidth && state.encoderSettings?.outHeight
                    ? { width: state.encoderSettings.outWidth, height: state.encoderSettings.outHeight }
                    : defaultEncoderOutputResolution
              }
            }
          : {
              state: 'disabled'
            },
        decoder: decoderEnabled
          ? {
              state: 'enabled',
              id: decoderId,
              settings: {
                fps_limit: state.decoderFpsLimit ?? null,
                rotation_degrees: state.decoderRotationDegrees ?? 0,
                mirror_horizontal: state.decoderMirrorHorizontal ?? false
              }
            }
          : {
              state: 'disabled'
            },
        shadow_recorder_enabled: shadowRecorderEnabled,
        host_buffer: state.hostBuffer ?? 8,
        preview_jpeg_quality: state.previewJpegQuality ?? 65,
        pipeline_enabled: enablePipeline,
        pipeline_id: activePipelineIdWire,
        pipeline_output: activePipelineOutputWire ?? null,
        pipeline_graph: null,
        pipeline_layout: enablePipeline ? { rows: state.pipelineGridRows, columns: state.pipelineGridColumns, slots: pipelineLayoutSlots } : null,
        // Preserve any pipeline wiring (output -> input) when re-applying presets, unless the user
        // explicitly disables pipelines.
        pipeline_wires: enablePipeline ? existingPipelineWires : [],
        pipelines: pipelineAssignments
      } as unknown as StreamManifest;

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
