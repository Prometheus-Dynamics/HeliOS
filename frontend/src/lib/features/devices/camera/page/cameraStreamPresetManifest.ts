import { withCurrentStreamManifestSchema } from '$lib/api/streamSchema';
import type { Interval, Mode, ProbedBackend, ProbedDevice, StreamInfo, StreamManifest } from '$lib/api/client';
import { buildEncoderSettingsForSelection, type EncoderSettingsDraft } from '$lib/api/streamEncoderSettings';
import { defaultPreviewJpegQualityForEncoder, type StreamCreationDefaults } from '$lib/api/streamDefaults';
import { recordingModeFromToggle } from '$lib/api/streamRecordingMode';

import { normalizeGridSlots } from './cameraPipelineState';
import { fpsToFrameRate } from './cameraStreamState';
import {
  PIPELINE_OUTPUT_CELL_KEY,
  RAW_PIPELINE_ID,
  RAW_PIPELINE_UUID
} from './cameraPipelineShared';

export type CameraStreamPresetManifestInput = {
  stream: StreamInfo | null;
  streamDefaults: StreamCreationDefaults;
  backend: ProbedBackend;
  device: ProbedDevice;
  mode: Mode;
  interval: Interval | null;
  pipelineGridRows: number;
  pipelineGridColumns: number;
  pipelineGridSlots: Record<string, string | null>;
  pipelineGridSlotOutputKeys: Record<string, string | null>;
  assignedPipelineIds: string[];
  selectedPipelineId: string | null;
  cameraAlias: string;
  encoderImpl: string | null;
  decoderImpl: string | null;
  encoderEnabled: boolean;
  decoderEnabled: boolean;
  encoderSettings: EncoderSettingsDraft;
  encoderFpsLimit: number | null;
  decoderFpsLimit: number | null;
  decoderRotationDegrees: number | null;
  decoderMirrorHorizontal: boolean;
  shadowRecorderEnabled: boolean;
  libcameraTargetFps: number | null;
  netcamTargetFps: number | null;
  fileBackendFps: number | null;
  fileBackendLoop: boolean;
  fileBackendPathsText: string;
  hostBuffer: number | null;
  previewJpegQuality: number | null;
  outputSelectionForPipeline: (pipelineId: string) => string | null;
};

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
  const resolved = asRecord(record.File);
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

export function buildCameraStreamPresetManifest(input: CameraStreamPresetManifestInput): StreamManifest {
  const existingManifest = input.stream?.manifest ?? null;
  const manifestIdentity = identityRecordFor(input.stream?.manifest);
  const existingHardwareId =
    typeof manifestIdentity?.hardware_id === 'string' && manifestIdentity.hardware_id.trim().length > 0
      ? manifestIdentity.hardware_id.trim()
      : null;

  const identity = {
    id: input.stream?.id ?? null,
    alias: input.cameraAlias.trim().length ? input.cameraAlias.trim() : null,
    hardware_id: existingHardwareId ?? selectStableHardwareId(input.device.identity?.keys) ?? null
  } as unknown as StreamManifest['identity'];

  const normalizedAssigned = Array.from(new Set((input.assignedPipelineIds ?? []).map((id) => String(id).trim()).filter(Boolean)));
  let normalizedActive = input.selectedPipelineId ? String(input.selectedPipelineId).trim() : null;
  let normalizedLayoutSlots = normalizeGridSlots(input.pipelineGridRows, input.pipelineGridColumns, input.pipelineGridSlots);
  const layoutIds = Object.values(normalizedLayoutSlots).filter((id): id is string => typeof id === 'string' && id.trim().length > 0);
  const assignedUnion = Array.from(new Set([...normalizedAssigned, ...layoutIds]));
  const effectiveAssigned = normalizedActive && !assignedUnion.includes(normalizedActive) ? [normalizedActive, ...assignedUnion] : assignedUnion;
  const gridIsMultiplex = Math.trunc(input.pipelineGridRows) * Math.trunc(input.pipelineGridColumns) > 1;
  if (!normalizedActive && !gridIsMultiplex && effectiveAssigned.length) {
    normalizedActive = effectiveAssigned[0];
  }

  const pipelineAssignments = effectiveAssigned.map((pipelineId) =>
    pipelineId === RAW_PIPELINE_ID
      ? {
          pipeline_id: RAW_PIPELINE_UUID,
          pipeline_graph: null,
          pipeline_output: input.outputSelectionForPipeline(RAW_PIPELINE_ID)
        }
      : {
          pipeline_id: pipelineId,
          pipeline_graph: null,
          pipeline_output: input.outputSelectionForPipeline(pipelineId)
        }
  );

  let hasAnySlot = Object.values(normalizedLayoutSlots).some((value) => typeof value === 'string' && value.trim().length > 0);
  const hasAnyPipelineAssignment = pipelineAssignments.length > 0;
  if (!hasAnyPipelineAssignment) {
    normalizedActive = null;
    normalizedLayoutSlots = normalizeGridSlots(input.pipelineGridRows, input.pipelineGridColumns, {});
    hasAnySlot = false;
  }
  const multiplex = gridIsMultiplex;
  if (!multiplex && hasAnySlot) {
    const outputCell = normalizedLayoutSlots[PIPELINE_OUTPUT_CELL_KEY];
    const fallbackSlot =
      outputCell && outputCell.trim().length
        ? outputCell
        : Object.values(normalizedLayoutSlots).find((value) => typeof value === 'string' && value.trim().length > 0) ?? null;
    if (typeof fallbackSlot === 'string' && fallbackSlot.trim().length) {
      normalizedActive = fallbackSlot;
    }
  }
  if (normalizedActive && !hasAnySlot && !multiplex) {
    normalizedLayoutSlots = { ...normalizedLayoutSlots, [PIPELINE_OUTPUT_CELL_KEY]: normalizedActive };
    hasAnySlot = true;
  }
  const enablePipeline = multiplex || Boolean(normalizedActive) || hasAnySlot || hasAnyPipelineAssignment;
  const activePipelineIdWire = enablePipeline
    ? normalizedActive === RAW_PIPELINE_ID
      ? RAW_PIPELINE_UUID
      : normalizedActive && normalizedActive !== RAW_PIPELINE_ID
        ? normalizedActive
        : null
    : null;
  const activePipelineOutputWire = enablePipeline && normalizedActive ? input.outputSelectionForPipeline(normalizedActive) : null;

  const pipelineLayoutSlots = Object.entries(normalizedLayoutSlots)
    .map(([key, pipelineId]) => {
      if (!pipelineId) return null;
      const [rRaw, cRaw] = key.split(':');
      const row = Number(rRaw);
      const column = Number(cRaw);
      if (!Number.isInteger(row) || !Number.isInteger(column)) return null;
      let outputKey = input.pipelineGridSlotOutputKeys[key] ?? null;
      if ((!outputKey || !String(outputKey).trim().length) && pipelineId) {
        outputKey = input.outputSelectionForPipeline(pipelineId);
      }
      outputKey = typeof outputKey === 'string' && outputKey.trim().length ? outputKey.trim() : null;
      if (pipelineId === RAW_PIPELINE_ID) {
        return { row, column, pipeline_id: RAW_PIPELINE_UUID, output_key: outputKey };
      }
      return { row, column, pipeline_id: pipelineId, output_key: outputKey };
    })
    .filter(Boolean);

  const backendKind = String(input.backend.kind ?? '');
  const normalizedBackendKind = backendKind.trim().toLowerCase();
  let captureHandle: unknown = input.backend.handle;
  if (normalizedBackendKind === 'file') {
    const backendFileHandle = extractFileHandle(input.backend.handle);
    const parsedPaths = String(input.fileBackendPathsText ?? '')
      .split('\n')
      .map((value) => value.trim())
      .filter((value) => value.length > 0);
    const dedupedPaths = Array.from(new Set(parsedPaths));
    const fallbackPaths = Array.isArray(backendFileHandle?.paths)
      ? backendFileHandle.paths.filter((value): value is string => typeof value === 'string' && value.trim().length > 0)
      : [];
    const parsedFps = Number(input.fileBackendFps ?? NaN);
    const fallbackFps = Number(backendFileHandle?.fps ?? NaN);
    const fps = Number.isFinite(parsedFps)
      ? parsedFps
      : Number.isFinite(fallbackFps)
        ? fallbackFps
        : null;
    captureHandle = {
      File: {
        fps,
        loop_forever: Boolean(input.fileBackendLoop),
        paths: dedupedPaths.length ? dedupedPaths : fallbackPaths
      }
    };
  }

  const modeWidth = Number(input.mode.format?.resolution?.width ?? 0);
  const modeHeight = Number(input.mode.format?.resolution?.height ?? 0);
  const defaultEncoderOutputResolution =
    Number.isFinite(modeWidth) && Number.isFinite(modeHeight) && modeWidth > 0 && modeHeight > 0
      ? { width: Math.max(1, Math.trunc(modeWidth / 2)), height: Math.max(1, Math.trunc(modeHeight / 2)) }
      : null;

  const isFileBackend = normalizedBackendKind === 'file';
  const shadowRecorderEnabled = !isFileBackend && input.shadowRecorderEnabled;

  const encoderId = (() => {
    if (!input.encoderEnabled) return null;
    const selected = String(input.encoderImpl ?? '').trim();
    return selected.length ? selected : null;
  })();
  const decoderId = (() => {
    if (!input.decoderEnabled) return null;
    const selected = String(input.decoderImpl ?? '').trim();
    return selected.length ? selected : null;
  })();
  const decoderEnabled = input.decoderEnabled && Boolean(decoderId);

  const encoderSettingsWire = buildEncoderSettingsForSelection(encoderId, input.encoderSettings, {
    frameRate: fpsToFrameRate(input.encoderFpsLimit),
    defaultOutputResolution: defaultEncoderOutputResolution
  });

  const existingPipelineWires =
    Array.isArray(input.stream?.manifest?.pipeline_wires) ? input.stream.manifest.pipeline_wires : [];

  return withCurrentStreamManifestSchema({
    identity,
    capture: {
      backend: input.backend.kind,
      handle: captureHandle,
      mode: input.mode,
      interval: input.interval,
      target_fps: backendKind === 'Libcamera' ? input.libcameraTargetFps : backendKind === 'Netcam' ? input.netcamTargetFps : null,
      controls: [],
      device_keys: input.device.identity?.keys ?? []
    },
    internal: existingManifest?.internal ?? false,
    encoder: input.encoderEnabled
      ? {
          state: 'enabled',
          id: encoderId,
          settings: encoderSettingsWire ?? undefined
        }
      : {
          state: 'disabled'
        },
    decoder: decoderEnabled
      ? {
          state: 'enabled',
          id: decoderId,
          settings: {
            fps_limit: input.decoderFpsLimit ?? null,
            rotation_degrees: input.decoderRotationDegrees ?? 0,
            mirror_horizontal: input.decoderMirrorHorizontal ?? false
          }
        }
      : {
          state: 'disabled'
        },
    recording_mode: recordingModeFromToggle(shadowRecorderEnabled, encoderId),
    host_buffer: input.hostBuffer ?? input.streamDefaults.defaultHostBuffer,
    preview_jpeg_quality:
      input.previewJpegQuality ??
      defaultPreviewJpegQualityForEncoder(input.streamDefaults, input.encoderEnabled) ??
      input.streamDefaults.defaultPreviewJpegQuality,
    pipeline_enabled: enablePipeline,
    active_pipeline_id: activePipelineIdWire,
    active_pipeline_output: activePipelineOutputWire ?? null,
    pipeline_layout: enablePipeline ? { rows: input.pipelineGridRows, columns: input.pipelineGridColumns, slots: pipelineLayoutSlots } : null,
    pipeline_wires: enablePipeline ? existingPipelineWires : [],
    pipeline_host_inputs: existingManifest?.pipeline_host_inputs ?? {},
    calibration: existingManifest?.calibration ?? null,
    pose: existingManifest?.pose ?? null,
    pipelines: pipelineAssignments,
    start_on_boot: existingManifest?.start_on_boot ?? input.streamDefaults.defaultStartOnBoot
  }) as unknown as StreamManifest;
}
