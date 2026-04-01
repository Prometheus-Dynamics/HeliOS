import { encoderRecordingCodecForSelection } from './streamEncoderSettings';

export type StreamRecordingCodec = 'h264' | 'h265';

export type StreamRecordingModeWire =
  | { state: 'disabled' }
  | {
      state: 'shadow_buffer';
      codec: StreamRecordingCodec;
    };

export const DEFAULT_STREAM_RECORDING_CODEC: StreamRecordingCodec = 'h264';

export function defaultRecordingMode(): StreamRecordingModeWire {
  return { state: 'disabled' };
}

export function inferRecordingCodecFromEncoderId(value: string | null | undefined): StreamRecordingCodec | null {
  return encoderRecordingCodecForSelection(value);
}

export function normalizeRecordingMode(value: unknown): StreamRecordingModeWire {
  if (!value || typeof value !== 'object') return defaultRecordingMode();
  const record = value as Record<string, unknown>;
  const state = String(record.state ?? '').trim().toLowerCase();
  if (state !== 'shadow_buffer') return defaultRecordingMode();
  const codec = String(record.codec ?? '').trim().toLowerCase();
  return {
    state: 'shadow_buffer',
    codec: codec === 'h265' ? 'h265' : 'h264'
  };
}

export function recordingModeEnabled(value: unknown): boolean {
  return normalizeRecordingMode(value).state === 'shadow_buffer';
}

export function recordingModeFromToggle(enabled: boolean, encoderId: string | null | undefined): StreamRecordingModeWire {
  if (!enabled) return defaultRecordingMode();
  return {
    state: 'shadow_buffer',
    codec: inferRecordingCodecFromEncoderId(encoderId) ?? DEFAULT_STREAM_RECORDING_CODEC
  };
}
