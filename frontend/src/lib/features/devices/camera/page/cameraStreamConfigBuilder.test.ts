import { describe, expect, test } from 'bun:test';

import type { ProbedBackend, ProbedDevice, StreamInfo } from '$lib/api/httpClient';
import { createEncoderSettingsDraft } from '$lib/api/streamEncoderSettings';
import type { StreamCreationDefaults } from '$lib/api/streamDefaults';

import {
  buildCameraStreamManifest,
  deriveStreamCodecSelections,
  type CameraStreamConfigBuilderInput
} from './cameraStreamConfigBuilder';
import { RAW_PIPELINE_ID, RAW_PIPELINE_UUID } from './cameraPipelineConstants';

const DEFAULTS: StreamCreationDefaults = {
  rawOutput: 'raw',
  undistortedOutput: 'undistorted',
  pipelineEnabledWhenBindingsPresent: true,
  defaultEncoderEnabled: true,
  defaultDecoderEnabled: true,
  defaultHostBuffer: 4,
  defaultPreviewJpegQuality: 37,
  defaultPreviewJpegQualityWhenEncoderDisabled: 21,
  defaultRecordingMode: { state: 'disabled' },
  defaultStartOnBoot: false,
  defaultEncoderId: 'turbojpeg',
  defaultDecoderIdsByCaptureFormat: {
    RGB3: 'rgb3-cpu',
    YUYV: 'yuyv-cpu',
    ANY: 'fallback-decoder'
  }
};

function createBackend(kind: string, handle: unknown = { type: kind.toLowerCase(), id: 'cam0' }): ProbedBackend {
  return {
    kind,
    handle,
    descriptor: { modes: [], controls: [] },
    properties: []
  } as unknown as ProbedBackend;
}

function createDevice(backend: ProbedBackend, keys: string[] = ['pci/0000:00:00.0']): ProbedDevice {
  return {
    identity: {
      display: 'Camera A',
      keys
    },
    backends: [backend]
  } as unknown as ProbedDevice;
}

function createBuilderInput(overrides: Partial<CameraStreamConfigBuilderInput> = {}): CameraStreamConfigBuilderInput {
  const backend = createBackend('Libcamera');
  return {
    stream: {
      id: '00000000-0000-0000-0000-000000000111',
      manifest: {
        schema_version: 1,
        identity: {
          id: '00000000-0000-0000-0000-000000000111',
          alias: 'Existing',
          hardware_id: 'persisted-device-id'
        },
        capture: {
          backend: 'Libcamera',
          handle: { type: 'libcamera', id: 'cam0' },
          mode: {
            format: { code: 'YUYV', resolution: { width: 1280, height: 720 }, color: 'Srgb' },
            interval: null
          },
          interval: null,
          target_fps: 30,
          controls: [],
          device_keys: ['persisted-device-id']
        },
        host_buffer: 2,
        internal: false,
        pipeline_enabled: true,
        pipelines: [],
        pipeline_wires: [{ from: { pipeline_id: 'pipe-a' }, to: { pipeline_id: 'pipe-b' } }],
        pipeline_host_inputs: {},
        encoder: { state: 'enabled', id: 'turbojpeg' },
        decoder: { state: 'disabled' },
        preview_jpeg_quality: 30,
        recording_mode: { state: 'disabled' },
        start_on_boot: false
      }
    } as unknown as StreamInfo,
    streamDefaults: DEFAULTS,
    backend,
    device: createDevice(backend),
    mode: {
      id: {
        format: { code: 'YUYV', resolution: { width: 1280, height: 720 }, color: 'Srgb' },
        interval: null
      },
      format: { code: 'YUYV', resolution: { width: 1280, height: 720 }, color: 'Srgb' },
      intervals: [],
      interval_stepwise: null
    },
    interval: null,
    draft: {
      cameraAlias: '  Driver Cam  ',
      encoderImpl: 'turbojpeg',
      decoderImpl: 'yuyv-cpu',
      encoderEnabled: true,
      decoderEnabled: true,
      encoderSettings: createEncoderSettingsDraft(),
      encoderFpsLimit: 30,
      decoderFpsLimit: 18,
      decoderRotationDegrees: 90,
      decoderMirrorHorizontal: true,
      shadowRecorderEnabled: true,
      libcameraTargetFps: 60,
      netcamTargetFps: null,
      fileBackendFps: null,
      fileBackendLoop: true,
      fileBackendPathsText: '',
      hostBuffer: null,
      previewJpegQuality: null
    },
    pipeline: {
      rows: 1,
      columns: 1,
      slots: { '0:0': RAW_PIPELINE_ID },
      slotOutputKeys: { '0:0': 'raw' },
      assignedPipelineIds: [RAW_PIPELINE_ID],
      selectedPipelineId: RAW_PIPELINE_ID,
      pipelineOutputByPipelineId: { [RAW_PIPELINE_ID]: 'raw' },
      existingPipelineWires: [{ from: { pipeline_id: 'pipe-a' }, to: { pipeline_id: 'pipe-b' } }]
    },
    ...overrides
  };
}

describe('camera stream config builder', () => {
  test('builds a single-view raw pipeline manifest from deterministic draft state', () => {
    const manifest = buildCameraStreamManifest(createBuilderInput());
    const identity = manifest.identity as unknown as Record<string, unknown>;
    const manifestRecord = manifest as unknown as Record<string, unknown>;

    expect(identity.id).toBe('00000000-0000-0000-0000-000000000111');
    expect(identity.alias).toBe('Driver Cam');
    expect(identity.hardware_id).toBe('persisted-device-id');
    expect(manifest.host_buffer).toBe(DEFAULTS.defaultHostBuffer);
    expect(manifest.preview_jpeg_quality).toBe(DEFAULTS.defaultPreviewJpegQuality);
    expect(manifest.capture.target_fps).toBe(60);
    expect(manifest.recording_mode).toEqual({ state: 'shadow_buffer', codec: 'h264' });
    expect(manifest.pipeline_enabled).toBe(true);
    expect(manifestRecord.pipeline_id).toBe(RAW_PIPELINE_UUID);
    expect(manifestRecord.pipeline_output).toBe('raw');
    expect(manifest.pipeline_layout).toEqual({
      rows: 1,
      columns: 1,
      slots: [{ row: 0, column: 0, pipeline_id: RAW_PIPELINE_UUID, output_key: 'raw' }]
    });
    expect(manifest.pipeline_wires).toHaveLength(1);
    expect(manifest.encoder).toMatchObject({ state: 'enabled', id: 'turbojpeg' });
    expect(manifest.decoder).toEqual({
      state: 'enabled',
      id: 'yuyv-cpu',
      settings: {
        fps_limit: 18,
        rotation_degrees: 90,
        mirror_horizontal: true
      }
    });
  });

  test('pins single-view output to the populated grid slot instead of drifting to another selected pipeline', () => {
    const manifest = buildCameraStreamManifest(
      createBuilderInput({
        pipeline: {
          rows: 1,
          columns: 1,
          slots: { '0:0': 'pipe-a' },
          slotOutputKeys: {},
          assignedPipelineIds: ['pipe-a', 'pipe-b'],
          selectedPipelineId: 'pipe-b',
          pipelineOutputByPipelineId: {
            'pipe-a': 'processed',
            'pipe-b': 'debug'
          },
          existingPipelineWires: []
        }
      })
    );
    const manifestRecord = manifest as unknown as Record<string, unknown>;

    expect(manifest.pipeline_enabled).toBe(true);
    expect(manifestRecord.pipeline_id).toBe('pipe-a');
    expect(manifestRecord.pipeline_output).toBe('processed');
    expect(manifest.pipeline_layout).toEqual({
      rows: 1,
      columns: 1,
      slots: [{ row: 0, column: 0, pipeline_id: 'pipe-a', output_key: 'processed' }]
    });
    expect(manifest.pipelines).toEqual([
      { pipeline_id: 'pipe-b', pipeline_graph: null, pipeline_output: 'debug' },
      { pipeline_id: 'pipe-a', pipeline_graph: null, pipeline_output: 'processed' }
    ]);
  });

  test('forces file-backed manifests to disable rolling recording and normalize file handles', () => {
    const backend = createBackend('File', {
      type: 'file',
      fps: 12,
      loop_forever: true,
      paths: ['/capture/a.mp4']
    });
    const manifest = buildCameraStreamManifest(
      createBuilderInput({
        backend,
        device: createDevice(backend, ['media-file']),
        draft: {
          ...createBuilderInput().draft,
          shadowRecorderEnabled: true,
          fileBackendLoop: false,
          fileBackendFps: null,
          fileBackendPathsText: ' /tmp/a.mp4 \n/tmp/a.mp4\n/tmp/b.mp4 '
        },
        pipeline: {
          rows: 1,
          columns: 1,
          slots: {},
          slotOutputKeys: {},
          assignedPipelineIds: [],
          selectedPipelineId: null,
          pipelineOutputByPipelineId: {},
          existingPipelineWires: [{ from: { pipeline_id: 'x' }, to: { pipeline_id: 'y' } }]
        }
      })
    );
    const capture = manifest.capture as unknown as Record<string, unknown>;

    expect(capture.handle).toEqual({
      type: 'file',
      fps: 12,
      loop_forever: false,
      paths: ['/tmp/a.mp4', '/tmp/b.mp4']
    });
    expect(manifest.recording_mode).toEqual({ state: 'disabled' });
    expect(manifest.pipeline_enabled).toBe(false);
    expect(manifest.pipeline_wires).toEqual([]);
  });

  test('derives codec selections from resolved ids, defaults, and selection modes', () => {
    const selections = deriveStreamCodecSelections({
      encoders: [
        { kind: 'encoder', name: 'mjpeg', implementation: 'turbojpeg', output: 'MJPG' } as never,
        { kind: 'encoder', name: 'h264', implementation: 'h264_v4l2m2m', output: 'H264' } as never
      ],
      decoders: [
        { kind: 'decoder', name: 'rgb3', implementation: 'rgb3-cpu', input: 'RGB3' } as never,
        { kind: 'decoder', name: 'yuyv', implementation: 'yuyv-cpu', input: 'YUYV' } as never
      ],
      encoderImpl: 'manual-encoder',
      decoderImpl: null,
      resolvedEncoderId: 'h264_v4l2m2m',
      resolvedDecoderId: null,
      requestedEncoderId: 'turbojpeg',
      requestedDecoderId: null,
      defaultEncoderId: 'turbojpeg',
      selectedFormat: 'RGB3',
      decoderDefaultIdsByCaptureFormat: DEFAULTS.defaultDecoderIdsByCaptureFormat,
      encoderSelectionMode: 'manual',
      decoderSelectionMode: 'auto'
    });

    expect(selections.encoderImpl).toBe('manual-encoder');
    expect(selections.decoderImpl).toBe('rgb3-cpu');
  });
});
