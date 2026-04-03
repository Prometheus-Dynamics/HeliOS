import type { StreamCapabilitiesResponse } from '$lib/api/client';
import { defaultRecordingMode, normalizeRecordingMode, type StreamRecordingModeWire } from './streamRecordingMode';

export type StreamCreationDefaults = {
  rawOutput: string;
  undistortedOutput: string;
  pipelineEnabledWhenBindingsPresent: boolean;
  defaultEncoderEnabled: boolean;
  defaultDecoderEnabled: boolean;
  defaultHostBuffer: number;
  defaultPreviewJpegQuality: number;
  defaultPreviewJpegQualityWhenEncoderDisabled: number;
  defaultRecordingMode: StreamRecordingModeWire;
  defaultStartOnBoot: boolean;
  defaultEncoderId: string | null;
  defaultDecoderIdsByCaptureFormat: Record<string, string>;
};

function asTrimmedString(value: unknown): string | null {
  return typeof value === 'string' && value.trim().length ? value.trim() : null;
}

function asPositiveInteger(value: unknown): number | null {
  const numeric = Number(value);
  if (!Number.isFinite(numeric) || numeric <= 0) return null;
  return Math.trunc(numeric);
}

function asJpegQuality(value: unknown): number | null {
  const numeric = Number(value);
  if (!Number.isFinite(numeric) || numeric < 1 || numeric > 100) return null;
  return Math.trunc(numeric);
}

export function resolveStreamCreationDefaults(
  capabilities: StreamCapabilitiesResponse | null | undefined
): StreamCreationDefaults | null {
  const defaults = capabilities?.defaults;
  const rawOutput = asTrimmedString(defaults?.rawOutput);
  const undistortedOutput = asTrimmedString(defaults?.undistortedOutput);
  const defaultHostBuffer = asPositiveInteger(defaults?.defaultHostBuffer);
  const defaultPreviewJpegQuality = asJpegQuality(defaults?.defaultPreviewJpegQuality);
  const defaultPreviewJpegQualityWhenEncoderDisabled = asJpegQuality(
    defaults?.defaultPreviewJpegQualityWhenEncoderDisabled
  );
  if (
    !rawOutput ||
    !undistortedOutput ||
    defaultHostBuffer == null ||
    defaultPreviewJpegQuality == null ||
    defaultPreviewJpegQualityWhenEncoderDisabled == null ||
    typeof defaults?.pipelineEnabledWhenBindingsPresent !== 'boolean' ||
    typeof defaults?.defaultEncoderEnabled !== 'boolean' ||
    typeof defaults?.defaultDecoderEnabled !== 'boolean' ||
    typeof defaults?.defaultStartOnBoot !== 'boolean'
  ) {
    return null;
  }

  const defaultDecoderIdsByCaptureFormat: Record<string, string> = {};
  const wireDefaults = defaults.defaultDecoderIdsByCaptureFormat ?? {};
  for (const [format, selector] of Object.entries(wireDefaults)) {
    const normalizedFormat = asTrimmedString(format)?.toUpperCase();
    const normalizedSelector = asTrimmedString(selector);
    if (!normalizedFormat || !normalizedSelector) continue;
    defaultDecoderIdsByCaptureFormat[normalizedFormat] = normalizedSelector;
  }

  return {
    rawOutput,
    undistortedOutput,
    pipelineEnabledWhenBindingsPresent: defaults.pipelineEnabledWhenBindingsPresent,
    defaultEncoderEnabled: defaults.defaultEncoderEnabled,
    defaultDecoderEnabled: defaults.defaultDecoderEnabled,
    defaultHostBuffer,
    defaultPreviewJpegQuality,
    defaultPreviewJpegQualityWhenEncoderDisabled,
    defaultRecordingMode: normalizeRecordingMode(defaults.defaultRecordingMode ?? defaultRecordingMode()),
    defaultStartOnBoot: defaults.defaultStartOnBoot,
    defaultEncoderId: asTrimmedString(defaults.defaultEncoderId),
    defaultDecoderIdsByCaptureFormat
  };
}

export function defaultPreviewJpegQualityForEncoder(
  defaults: StreamCreationDefaults | null,
  encoderEnabled: boolean
): number | null {
  if (!defaults) return null;
  return encoderEnabled ? defaults.defaultPreviewJpegQuality : defaults.defaultPreviewJpegQualityWhenEncoderDisabled;
}
