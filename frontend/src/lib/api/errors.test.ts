import { describe, expect, test } from 'bun:test';

import { ApiError } from '$lib/ts-bindings/http/client/core/ApiError';

import { extractErrorMetadata, extractValidationReport } from './errors';

function makeValidationApiError(body: unknown): ApiError {
  return new ApiError(
    { method: 'POST', url: '/streams' },
    {
      url: 'http://helios.local/v1/streams',
      ok: false,
      status: 422,
      statusText: 'Unprocessable Entity',
      body,
    },
    'validation failed',
  );
}

describe('validation error extraction', () => {
  test('extracts structured validation issues and warnings', () => {
    const error = makeValidationApiError({
      code: 'validation_error',
      error: 'Stream validation failed.',
      issues: [
        {
          path: '/encoder/id',
          code: 'shadow_recorder_requires_h26x_encoder',
          message: 'shadow recorder requires an h264/h265 encoder selection',
          remediation: 'Select an H264 or H265 encoder for this stream before enabling shadow recording.'
        }
      ],
      warnings: [
        {
          path: '/active_pipeline_output',
          code: 'raw_output_canonicalized',
          message: 'active pipeline output was canonicalized to a supported RAW output value'
        }
      ],
      timestamp_ms: 1234
    });

    expect(extractValidationReport(error)).toEqual({
      issues: [
        {
          path: '/encoder/id',
          code: 'shadow_recorder_requires_h26x_encoder',
          message: 'shadow recorder requires an h264/h265 encoder selection',
          remediation: 'Select an H264 or H265 encoder for this stream before enabling shadow recording.'
        }
      ],
      warnings: [
        {
          path: '/active_pipeline_output',
          code: 'raw_output_canonicalized',
          message: 'active pipeline output was canonicalized to a supported RAW output value'
        }
      ]
    });
  });

  test('promotes issue remediation into error metadata', () => {
    const error = makeValidationApiError({
      code: 'validation_error',
      error: 'Stream validation failed.',
      issues: [
        {
          path: '/shadow_recorder_enabled',
          code: 'shadow_recorder_requires_encoder',
          message: 'shadow recorder requires the stream encoder to be enabled',
          remediation: 'Enable an H264 or H265 encoder before turning on shadow recording.'
        }
      ],
      warnings: [],
      timestamp_ms: 9876
    });

    expect(extractErrorMetadata(error)).toMatchObject({
      code: 'validation_error',
      timestampMs: 9876,
      remediation: 'Enable an H264 or H265 encoder before turning on shadow recording.',
      validationIssues: [
        {
          path: '/shadow_recorder_enabled',
          code: 'shadow_recorder_requires_encoder',
          message: 'shadow recorder requires the stream encoder to be enabled',
          remediation: 'Enable an H264 or H265 encoder before turning on shadow recording.'
        }
      ]
    });
  });
});
