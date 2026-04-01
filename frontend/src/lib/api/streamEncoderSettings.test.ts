import { describe, expect, test } from 'bun:test';

import {
  encoderRecordingCodecForSelection,
  encoderSelectionId,
  encoderSettingsKindForSelection,
} from './streamEncoderSettings';

describe('stream encoder codec families', () => {
  test('preserves exact runtime implementation ids for encoder selections', () => {
    expect(
      encoderSelectionId({
        implementation: 'h264_v4l2m2m',
        name: 'h264',
        output: 'H264',
      }),
    ).toBe('h264_v4l2m2m');

    expect(
      encoderSelectionId({
        implementation: 'hevc_v4l2m2m',
        name: 'hevc',
        output: 'HEVC',
      }),
    ).toBe('hevc_v4l2m2m');
  });

  test('maps runtime implementation ids back to encoder settings kinds', () => {
    expect(encoderSettingsKindForSelection('h264_v4l2m2m')).toBe('h264');
    expect(encoderSettingsKindForSelection('hevc_v4l2m2m')).toBe('h265');
    expect(encoderSettingsKindForSelection('turbojpeg')).toBe('turbojpeg');
  });

  test('derives recording codecs from generated encoder family bindings', () => {
    expect(encoderRecordingCodecForSelection('h264_v4l2m2m')).toBe('h264');
    expect(encoderRecordingCodecForSelection('hevc_v4l2m2m')).toBe('h265');
    expect(encoderRecordingCodecForSelection('turbojpeg')).toBeNull();
  });
});
