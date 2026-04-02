import { withCurrentStreamManifestSchema } from '$lib/api/streamSchema';
import type {
  CodecInfo,
  Interval,
  Mode,
  ProbedBackend,
  ProbedDevice,
  StreamInfo,
  StreamManifest
} from '$lib/api/httpClient';
import { buildEncoderSettingsForSelection, type EncoderSettingsDraft } from '$lib/api/streamEncoderSettings';
import { defaultPreviewJpegQualityForEncoder, type StreamCreationDefaults } from '$lib/api/streamDefaults';
import { recordingModeFromToggle } from '$lib/api/streamRecordingMode';

import { decoderPreferencesForFormat, pickCodecId } from './cameraBackendCodecs';
import { asPositiveNumber, extractFileHandle, identityRecordFor, isFileBackend } from './cameraBackendSupport';
import { normalizeGridSlots } from './cameraPipelineState';
import { fpsToFrameRate } from './cameraStreamState';
import type { StreamSelectionMode } from './cameraStreamEditorReducer';
import {
  PIPELINE_OUTPUT_CELL_KEY,
  RAW_PIPELINE_ID,
  RAW_PIPELINE_UUID
} from './cameraPipelineConstants';

export type CameraStreamConfigDraft = {
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
};

export type CameraStreamPipelineDraft = {
  rows: number;
  columns: number;
  slots: Record<string, string | null>;
  slotOutputKeys: Record<string, string | null>;
  assignedPipelineIds: string[];
  selectedPipelineId: string | null;
  pipelineOutputByPipelineId: Record<string, string | null>;
  existingPipelineWires: StreamManifest['pipeline_wires'];
};

export type CameraStreamConfigBuilderInput = {
  stream: StreamInfo | null;
  streamDefaults: StreamCreationDefaults;
  backend: ProbedBackend;
  device: ProbedDevice;
  mode: Mode;
  interval: Interval | null;
  draft: CameraStreamConfigDraft;
  pipeline: CameraStreamPipelineDraft;
};

export type CameraStreamCodecSelectionInput = {
  encoders: CodecInfo[];
  decoders: CodecInfo[];
  encoderImpl: string | null;
  decoderImpl: string | null;
  resolvedEncoderId?: string | null;
  resolvedDecoderId?: string | null;
  requestedEncoderId?: string | null;
  requestedDecoderId?: string | null;
  defaultEncoderId?: string | null;
  selectedFormat: string | null;
  decoderDefaultIdsByCaptureFormat: Record<string, string>;
  encoderSelectionMode: StreamSelectionMode;
  decoderSelectionMode: StreamSelectionMode;
};

export type CameraStreamCodecSelections = {
  encoderImpl: string | null;
  decoderImpl: string | null;
};

function selectStableHardwareId(keys: unknown): string | null {
  if (!Array.isArray(keys)) return null;
  const normalized = keys
    .map((value) => (typeof value === 'string' ? value.trim() : ''))
    .filter((value): value is string => value.length > 0);
  const withSlash = normalized.find((value) => value.includes('/'));
  if (withSlash) return withSlash;
  const withColon = normalized.find((value) => value.includes(':'));
  if (withColon) return withColon;
  return [...normalized].sort()[0] ?? null;
}

function selectedOutputForPipeline(
  pipelineOutputByPipelineId: Record<string, string | null>,
  pipelineId: string
): string | null {
  const normalized = String(pipelineId ?? '').trim();
  if (!normalized.length) return null;
  const raw = pipelineOutputByPipelineId[normalized];
  const value = typeof raw === 'string' && raw.trim().length ? raw.trim() : null;
  if (normalized === RAW_PIPELINE_ID && value?.toLowerCase() === 'frame') {
    return 'raw';
  }
  return value;
}

function normalizeBuilderAssignedPipelineIds(ids: string[]): string[] {
  return Array.from(
    new Set(
      (ids ?? [])
        .map((id) => String(id ?? '').trim())
        .filter((id) => id.length > 0)
    )
  ).slice(0, 64);
}

export function deriveStreamCodecSelections(
  input: CameraStreamCodecSelectionInput
): CameraStreamCodecSelections {
  const preferredDecoderIds = decoderPreferencesForFormat(
    input.selectedFormat,
    input.decoderDefaultIdsByCaptureFormat
  );
  const preferredEncoderIds =
    typeof input.defaultEncoderId === 'string' && input.defaultEncoderId.trim().length > 0
      ? [input.defaultEncoderId.trim()]
      : [];

  return {
    encoderImpl: input.encoderSelectionMode === 'manual'
      ? input.encoderImpl
      : pickCodecId(
          input.encoders,
          input.resolvedEncoderId || input.requestedEncoderId || input.encoderImpl,
          preferredEncoderIds
        ),
    decoderImpl: input.decoderSelectionMode === 'manual'
      ? input.decoderImpl
      : pickCodecId(
          input.decoders,
          input.resolvedDecoderId || input.requestedDecoderId || input.decoderImpl,
          preferredDecoderIds
        )
  };
}

export function buildCameraStreamManifest(
  input: CameraStreamConfigBuilderInput
): StreamManifest {
  const existingIdentity = identityRecordFor(input.stream?.manifest);
  const existingHardwareId =
    typeof existingIdentity?.hardware_id === 'string' && existingIdentity.hardware_id.trim().length > 0
      ? existingIdentity.hardware_id.trim()
      : null;
  const normalizedBackendKind = String(input.backend.kind ?? '').trim().toLowerCase();
  const modeWidth = Number(input.mode.format?.resolution?.width ?? 0);
  const modeHeight = Number(input.mode.format?.resolution?.height ?? 0);
  const defaultEncoderOutputResolution =
    Number.isFinite(modeWidth) && Number.isFinite(modeHeight) && modeWidth > 0 && modeHeight > 0
      ? { width: Math.max(1, Math.trunc(modeWidth / 2)), height: Math.max(1, Math.trunc(modeHeight / 2)) }
      : null;

  let captureHandle: unknown = input.backend.handle;
  if (normalizedBackendKind === 'file') {
    const backendFileHandle = extractFileHandle(input.backend.handle);
    const parsedPaths = String(input.draft.fileBackendPathsText ?? '')
      .split('\n')
      .map((value) => value.trim())
      .filter((value) => value.length > 0);
    const dedupedPaths = Array.from(new Set(parsedPaths));
    const fallbackPaths = Array.isArray(backendFileHandle?.paths)
      ? backendFileHandle.paths.filter((value): value is string => typeof value === 'string' && value.trim().length > 0)
      : [];
    const parsedFps = Number(input.draft.fileBackendFps ?? NaN);
    const fallbackFps = Number(backendFileHandle?.fps ?? NaN);
    const fps = Number.isFinite(parsedFps)
      ? parsedFps
      : Number.isFinite(fallbackFps)
        ? fallbackFps
        : null;
    captureHandle = {
      type: 'file',
      fps,
      loop_forever: Boolean(input.draft.fileBackendLoop),
      paths: dedupedPaths.length ? dedupedPaths : fallbackPaths
    };
  }

  const encoderId = (() => {
    if (!input.draft.encoderEnabled) return null;
    const selected = String(input.draft.encoderImpl ?? '').trim();
    return selected.length ? selected : null;
  })();
  const decoderId = (() => {
    if (!input.draft.decoderEnabled) return null;
    const selected = String(input.draft.decoderImpl ?? '').trim();
    return selected.length ? selected : null;
  })();
  const decoderEnabled = input.draft.decoderEnabled && Boolean(decoderId);

  const encoderSettingsWire = buildEncoderSettingsForSelection(encoderId, input.draft.encoderSettings, {
    frameRate: fpsToFrameRate(input.draft.encoderFpsLimit),
    defaultOutputResolution: defaultEncoderOutputResolution
  });

  const normalizedAssigned = normalizeBuilderAssignedPipelineIds(input.pipeline.assignedPipelineIds);
  let normalizedActive = input.pipeline.selectedPipelineId
    ? String(input.pipeline.selectedPipelineId).trim()
    : null;
  let normalizedLayoutSlots = normalizeGridSlots(
    input.pipeline.rows,
    input.pipeline.columns,
    input.pipeline.slots
  );
  const layoutIds = Object.values(normalizedLayoutSlots).filter(
    (id): id is string => typeof id === 'string' && id.trim().length > 0
  );
  const effectiveAssigned = normalizeBuilderAssignedPipelineIds([
    ...(normalizedActive ? [normalizedActive] : []),
    ...normalizedAssigned,
    ...layoutIds
  ]);
  const multiplex = Math.trunc(input.pipeline.rows) * Math.trunc(input.pipeline.columns) > 1;
  if (!normalizedActive && !multiplex && effectiveAssigned.length) {
    normalizedActive = effectiveAssigned[0];
  }

  const pipelineAssignments = effectiveAssigned.map((pipelineId) =>
    pipelineId === RAW_PIPELINE_ID
      ? {
          pipeline_id: RAW_PIPELINE_UUID,
          pipeline_graph: null,
          pipeline_output: selectedOutputForPipeline(input.pipeline.pipelineOutputByPipelineId, RAW_PIPELINE_ID)
        }
      : {
          pipeline_id: pipelineId,
          pipeline_graph: null,
          pipeline_output: selectedOutputForPipeline(input.pipeline.pipelineOutputByPipelineId, pipelineId)
        }
  );

  let hasAnySlot = Object.values(normalizedLayoutSlots).some(
    (value) => typeof value === 'string' && value.trim().length > 0
  );
  const hasAnyPipelineAssignment = pipelineAssignments.length > 0;
  if (!hasAnyPipelineAssignment) {
    normalizedActive = null;
    normalizedLayoutSlots = normalizeGridSlots(input.pipeline.rows, input.pipeline.columns, {});
    hasAnySlot = false;
  }
  if (!multiplex && hasAnySlot) {
    const outputCell = normalizedLayoutSlots[PIPELINE_OUTPUT_CELL_KEY];
    const fallbackSlot =
      outputCell && outputCell.trim().length
        ? outputCell
        : Object.values(normalizedLayoutSlots).find(
            (value) => typeof value === 'string' && value.trim().length > 0
          ) ?? null;
    if (typeof fallbackSlot === 'string' && fallbackSlot.trim().length) {
      normalizedActive = fallbackSlot;
    }
  }
  if (normalizedActive && !hasAnySlot && !multiplex) {
    normalizedLayoutSlots = { ...normalizedLayoutSlots, [PIPELINE_OUTPUT_CELL_KEY]: normalizedActive };
    hasAnySlot = true;
  }
  const enablePipeline =
    multiplex || Boolean(normalizedActive) || hasAnySlot || hasAnyPipelineAssignment;
  const activePipelineIdWire = enablePipeline
    ? normalizedActive === RAW_PIPELINE_ID
      ? RAW_PIPELINE_UUID
      : normalizedActive && normalizedActive !== RAW_PIPELINE_ID
        ? normalizedActive
        : null
    : null;
  const activePipelineOutputWire =
    enablePipeline && normalizedActive
      ? selectedOutputForPipeline(input.pipeline.pipelineOutputByPipelineId, normalizedActive)
      : null;

  const pipelineLayoutSlots = Object.entries(normalizedLayoutSlots)
    .map(([key, pipelineId]) => {
      if (!pipelineId) return null;
      const [rowRaw, columnRaw] = key.split(':');
      const row = Number(rowRaw);
      const column = Number(columnRaw);
      if (!Number.isInteger(row) || !Number.isInteger(column)) return null;
      let output_key = input.pipeline.slotOutputKeys[key] ?? null;
      if ((!output_key || !String(output_key).trim().length) && pipelineId) {
        output_key = selectedOutputForPipeline(input.pipeline.pipelineOutputByPipelineId, pipelineId);
      }
      output_key = typeof output_key === 'string' && output_key.trim().length ? output_key.trim() : null;
      if (pipelineId === RAW_PIPELINE_ID) {
        return { row, column, pipeline_id: RAW_PIPELINE_UUID, output_key };
      }
      return { row, column, pipeline_id: pipelineId, output_key };
    })
    .filter(Boolean);

  const shadowRecorderEnabled = !isFileBackend(input.backend.kind) && input.draft.shadowRecorderEnabled;

  return withCurrentStreamManifestSchema({
    identity: {
      id: input.stream?.id ?? null,
      alias: input.draft.cameraAlias.trim().length ? input.draft.cameraAlias.trim() : null,
      hardware_id: existingHardwareId ?? selectStableHardwareId(input.device.identity?.keys) ?? null
    },
    capture: {
      backend: input.backend.kind,
      handle: captureHandle,
      mode: input.mode,
      interval: input.interval,
      target_fps:
        normalizedBackendKind === 'libcamera'
          ? input.draft.libcameraTargetFps
          : normalizedBackendKind === 'netcam'
            ? input.draft.netcamTargetFps
            : null,
      controls: [],
      device_keys: input.device.identity?.keys ?? []
    },
    encoder: input.draft.encoderEnabled
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
            fps_limit: input.draft.decoderFpsLimit ?? null,
            rotation_degrees: input.draft.decoderRotationDegrees ?? 0,
            mirror_horizontal: input.draft.decoderMirrorHorizontal ?? false
          }
        }
      : {
          state: 'disabled'
        },
    recording_mode: recordingModeFromToggle(shadowRecorderEnabled, encoderId),
    host_buffer: input.draft.hostBuffer ?? input.streamDefaults.defaultHostBuffer,
    preview_jpeg_quality:
      input.draft.previewJpegQuality ??
      defaultPreviewJpegQualityForEncoder(input.streamDefaults, input.draft.encoderEnabled) ??
      input.streamDefaults.defaultPreviewJpegQuality,
    pipeline_enabled: enablePipeline,
    pipeline_id: activePipelineIdWire,
    pipeline_output: activePipelineOutputWire ?? null,
    pipeline_graph: null,
    pipeline_layout: enablePipeline
      ? {
          rows: input.pipeline.rows,
          columns: input.pipeline.columns,
          slots: pipelineLayoutSlots
        }
      : null,
    pipeline_wires: enablePipeline ? input.pipeline.existingPipelineWires : [],
    pipelines: pipelineAssignments
  }) as unknown as StreamManifest;
}
