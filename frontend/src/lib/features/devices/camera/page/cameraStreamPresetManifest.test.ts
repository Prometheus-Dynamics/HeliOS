import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { Interval, Mode, ProbedBackend, ProbedDevice, StreamInfo, StreamManifest } from '$lib/api/httpClient';
import type { StreamCreationDefaults } from '$lib/api/streamDefaults';

import { buildCameraStreamPresetManifest, type CameraStreamPresetManifestInput } from './cameraStreamPresetManifest';
import { RAW_PIPELINE_ID } from './cameraPipelineShared';

type RoundTripFixtureCase = {
  name: string;
  requested_manifest: StreamManifest;
};

type RoundTripFixtureFile = {
  cases: RoundTripFixtureCase[];
};

const fixtureDir = dirname(fileURLToPath(import.meta.url));
const fixturePath = resolve(fixtureDir, '../../../../../../../testdata/stream_config_roundtrip.json');
const fixtureData = JSON.parse(readFileSync(fixturePath, 'utf8')) as RoundTripFixtureFile;
const fixtures = new Map(fixtureData.cases.map((entry) => [entry.name, entry]));

function fixture(name: string): RoundTripFixtureCase {
  const entry = fixtures.get(name);
  if (!entry) throw new Error(`missing round-trip fixture ${name}`);
  return entry;
}

function mode(width: number, height: number): Mode {
  return {
    format: {
      code: 'RGB3',
      color: 'Srgb',
      resolution: { width, height }
    },
    interval: null
  } as unknown as Mode;
}

function virtualBackend(): ProbedBackend {
  return {
    kind: 'Virtual',
    handle: { type: 'virtual' }
  } as unknown as ProbedBackend;
}

function virtualDevice(primaryKey: string): ProbedDevice {
  return {
    identity: {
      keys: [primaryKey, 'virtual']
    }
  } as ProbedDevice;
}

function virtualDefaults(): StreamCreationDefaults {
  return {
    rawOutput: 'raw',
    undistortedOutput: 'undistorted',
    pipelineEnabledWhenBindingsPresent: true,
    defaultEncoderEnabled: true,
    defaultDecoderEnabled: true,
    defaultHostBuffer: 5,
    defaultPreviewJpegQuality: 67,
    defaultPreviewJpegQualityWhenEncoderDisabled: 31,
    defaultRecordingMode: { state: 'disabled' },
    defaultStartOnBoot: false,
    defaultEncoderId: 'turbojpeg',
    defaultDecoderIdsByCaptureFormat: { RGB3: 'rgb-pass' }
  };
}

function preservedManifest(overrides: Partial<StreamManifest>): StreamManifest {
  return {
    schema_version: 1,
    identity: {
      id: null,
      alias: null,
      hardware_id: null
    },
    capture: {
      device_keys: [],
      backend: 'Virtual',
      handle: { type: 'virtual' },
      mode: mode(1, 1),
      target_fps: null,
      interval: null,
      controls: []
    },
    host_buffer: 2,
    internal: false,
    pipeline_enabled: false,
    pipelines: [],
    active_pipeline_id: null,
    active_pipeline_output: null,
    pipeline_layout: null,
    pipeline_wires: [],
    pipeline_host_inputs: {},
    calibration: null,
    pose: null,
    encoder: { state: 'disabled' },
    decoder: { state: 'disabled' },
    preview_jpeg_quality: 30,
    recording_mode: { state: 'disabled' },
    start_on_boot: false,
    ...overrides
  } as StreamManifest;
}

function stream(id: string, manifest: StreamManifest): StreamInfo {
  return {
    id,
    manifest
  } as StreamInfo;
}

describe('camera stream preset manifest builder', () => {
  test('builds the canonical raw virtual fixture', () => {
    const input: CameraStreamPresetManifestInput = {
      stream: stream(
        '11111111-1111-1111-1111-111111111111',
        preservedManifest({
          identity: {
            id: '11111111-1111-1111-1111-111111111111',
            alias: 'Virtual Raw Roundtrip',
            hardware_id: 'virtual/raw'
          } as unknown as StreamManifest['identity'],
          pipeline_wires: []
        })
      ),
      streamDefaults: virtualDefaults(),
      backend: virtualBackend(),
      device: virtualDevice('virtual/raw'),
      mode: mode(1280, 720),
      interval: null,
      pipelineGridRows: 1,
      pipelineGridColumns: 1,
      pipelineGridSlots: { '0:0': RAW_PIPELINE_ID },
      pipelineGridSlotOutputKeys: { '0:0': 'raw' },
      assignedPipelineIds: [RAW_PIPELINE_ID],
      selectedPipelineId: RAW_PIPELINE_ID,
      cameraAlias: 'Virtual Raw Roundtrip',
      encoderImpl: 'turbojpeg',
      decoderImpl: null,
      encoderEnabled: true,
      decoderEnabled: false,
      encoderSettings: {
        quality: null,
        bitrate: null,
        gop: null,
        framerateNum: null,
        framerateDen: null,
        threadCount: null,
        outWidth: null,
        outHeight: null
      },
      encoderFpsLimit: null,
      decoderFpsLimit: null,
      decoderRotationDegrees: 0,
      decoderMirrorHorizontal: false,
      shadowRecorderEnabled: false,
      libcameraTargetFps: null,
      netcamTargetFps: null,
      fileBackendFps: null,
      fileBackendLoop: false,
      fileBackendPathsText: '',
      hostBuffer: null,
      previewJpegQuality: null,
      outputSelectionForPipeline: (pipelineId) => (pipelineId === RAW_PIPELINE_ID ? 'raw' : null)
    };

    expect(buildCameraStreamPresetManifest(input)).toEqual(fixture('virtual_raw_turbojpeg').requested_manifest);
  });

  test('preserves pose and start-on-boot when rebuilding a pipeline-disabled fixture', () => {
    const preservedPose = fixture('virtual_passthrough_pose').requested_manifest.pose;
    const input: CameraStreamPresetManifestInput = {
      stream: stream(
        '22222222-2222-2222-2222-222222222222',
        preservedManifest({
          identity: {
            id: '22222222-2222-2222-2222-222222222222',
            alias: 'Virtual Idle Roundtrip',
            hardware_id: 'virtual/idle'
          } as unknown as StreamManifest['identity'],
          pose: preservedPose,
          start_on_boot: true
        })
      ),
      streamDefaults: virtualDefaults(),
      backend: virtualBackend(),
      device: virtualDevice('virtual/idle'),
      mode: mode(640, 480),
      interval: {
        numerator: 1,
        denominator: 15
      } as Interval,
      pipelineGridRows: 1,
      pipelineGridColumns: 1,
      pipelineGridSlots: {},
      pipelineGridSlotOutputKeys: {},
      assignedPipelineIds: [],
      selectedPipelineId: null,
      cameraAlias: 'Virtual Idle Roundtrip',
      encoderImpl: null,
      decoderImpl: null,
      encoderEnabled: false,
      decoderEnabled: false,
      encoderSettings: {
        quality: null,
        bitrate: null,
        gop: null,
        framerateNum: null,
        framerateDen: null,
        threadCount: null,
        outWidth: null,
        outHeight: null
      },
      encoderFpsLimit: null,
      decoderFpsLimit: null,
      decoderRotationDegrees: 0,
      decoderMirrorHorizontal: false,
      shadowRecorderEnabled: false,
      libcameraTargetFps: null,
      netcamTargetFps: null,
      fileBackendFps: null,
      fileBackendLoop: false,
      fileBackendPathsText: '',
      hostBuffer: 9,
      previewJpegQuality: 44,
      outputSelectionForPipeline: () => null
    };

    expect(buildCameraStreamPresetManifest(input)).toEqual(fixture('virtual_passthrough_pose').requested_manifest);
  });
});
