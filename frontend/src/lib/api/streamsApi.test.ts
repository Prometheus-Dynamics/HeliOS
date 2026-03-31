import { describe, expect, test } from 'bun:test';

import type { StreamCapabilitiesResponse } from '$lib/ts-bindings/http/client';

import { resolveStreamCreationDefaults } from './streamDefaults';
import { makeNetcamManifest } from './streamManifestBuilders';

function sampleCapabilities(): StreamCapabilitiesResponse {
  return {
    rawPipelineId: '11111111-1111-1111-1111-111111111111',
    calibrationModePipelineId: '22222222-2222-2222-2222-222222222222',
    defaults: {
      rawOutput: ' raw ',
      undistortedOutput: 'undistorted',
      pipelineEnabledWhenBindingsPresent: true,
      defaultEncoderEnabled: true,
      defaultDecoderEnabled: true,
      defaultHostBuffer: 17,
      defaultPreviewJpegQuality: 42,
      defaultPreviewJpegQualityWhenEncoderDisabled: 11,
      defaultShadowRecorderEnabled: true,
      defaultStartOnBoot: false,
      defaultEncoderId: 'turbojpeg',
      defaultDecoderIdsByCaptureFormat: {
        mjpg: ' turbojpeg '
      }
    },
    constraints: {
      requiresBackendHandleMatch: true,
      fileBackendRequiresNonEmptyPaths: true,
      fileBackendSupportedExtensions: [],
      minLayoutRows: 1,
      minLayoutColumns: 1,
      reservedPipelineIds: [],
      rawOutputValues: ['raw', 'undistorted']
    }
  };
}

describe('stream defaults contract', () => {
  test('normalizes requested stream defaults from capabilities', () => {
    const defaults = resolveStreamCreationDefaults(sampleCapabilities());
    expect(defaults).not.toBeNull();
    expect(defaults?.rawOutput).toBe('raw');
    expect(defaults?.defaultHostBuffer).toBe(17);
    expect(defaults?.defaultPreviewJpegQuality).toBe(42);
    expect(defaults?.defaultPreviewJpegQualityWhenEncoderDisabled).toBe(11);
    expect(defaults?.defaultDecoderIdsByCaptureFormat).toEqual({ MJPG: 'turbojpeg' });
  });

  test('netcam manifest uses backend-provided defaults instead of literals', () => {
    const manifest = makeNetcamManifest(
      {
        url: 'http://camera.local/mjpg',
        startOnBoot: null
      },
      sampleCapabilities()
    );

    expect(manifest.host_buffer).toBe(17);
    expect(manifest.preview_jpeg_quality).toBe(42);
    expect(manifest.shadow_recorder_enabled).toBe(true);
    expect(manifest.start_on_boot).toBe(false);
    expect(manifest.pipeline_enabled).toBe(true);
    expect(manifest.active_pipeline_output).toBe('raw');
  });
});
