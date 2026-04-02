import { describe, expect, test } from 'bun:test';

import {
  defaultCameraStreamEditorState,
  reduceCameraStreamEditorState,
  syncCameraStreamEditorCodecs
} from './cameraStreamEditorReducer';

const TEST_DEPS = {
  modeKey: (value: unknown): string | null =>
    typeof value === 'string' && value.trim().length ? value.trim() : null
};

const TEST_MODES = [
  {
    id: 'rgb3-640',
    format: {
      code: 'RGB3',
      resolution: { width: 640, height: 480 },
      color: 'srgb'
    },
    intervals: [
      { numerator: 1, denominator: 30 },
      { numerator: 1, denominator: 60 }
    ]
  },
  {
    id: 'yuyv-1280',
    format: {
      code: 'YUYV',
      resolution: { width: 1280, height: 720 },
      color: 'yuv'
    },
    intervals: [{ numerator: 1, denominator: 24 }]
  }
] as never[];

describe('camera stream editor reducer', () => {
  test('replaces stale resolution and interval selections when the format changes', () => {
    const next = reduceCameraStreamEditorState(
      defaultCameraStreamEditorState({
        selectedModeKey: 'rgb3-640',
        selectedFormat: 'RGB3',
        selectedResolution: '640x480',
        selectedIntervalIdx: 1
      }),
      {
        type: 'format_selected',
        format: 'YUYV',
        modes: TEST_MODES
      },
      TEST_DEPS
    );

    expect(next.selectedModeKey).toBe('yuyv-1280');
    expect(next.selectedFormat).toBe('YUYV');
    expect(next.selectedResolution).toBe('1280x720');
    expect(next.selectedIntervalIdx).toBe(0);
  });

  test('keeps manual codec selections during automatic sync', () => {
    const next = syncCameraStreamEditorCodecs({
      state: defaultCameraStreamEditorState({
        selectedFormat: 'RGB3',
        encoderImpl: 'manual-encoder',
        decoderImpl: 'manual-decoder',
        encoderSelectionMode: 'manual',
        decoderSelectionMode: 'manual'
      }),
      encoders: [
        { kind: 'encoder', name: 'mjpeg', implementation: 'turbojpeg', output: 'MJPG' } as never,
        { kind: 'encoder', name: 'h264', implementation: 'h264_v4l2m2m', output: 'H264' } as never
      ],
      decoders: [
        { kind: 'decoder', name: 'rgb3', implementation: 'rgb3-cpu', input: 'RGB3' } as never,
        { kind: 'decoder', name: 'yuyv', implementation: 'yuyv-cpu', input: 'YUYV' } as never
      ],
      resolvedEncoderId: 'turbojpeg',
      resolvedDecoderId: 'rgb3-cpu',
      requestedEncoderId: 'h264_v4l2m2m',
      requestedDecoderId: 'yuyv-cpu',
      defaultEncoderId: 'turbojpeg',
      decoderDefaultIdsByCaptureFormat: { RGB3: 'rgb3-cpu' }
    });

    expect(next.encoderImpl).toBe('manual-encoder');
    expect(next.decoderImpl).toBe('manual-decoder');
  });
});
