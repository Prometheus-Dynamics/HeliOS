import type { StreamInfo } from '$lib/ts-bindings/http/client/models/StreamInfo';

export type StreamCaptureRuntimeState = 'running' | 'stopped' | 'disabled';
export type StreamHealthStatus = 'live' | 'idle' | 'degraded';

export function streamCaptureState(stream: StreamInfo | null | undefined): StreamCaptureRuntimeState {
  const runtimeState = stream?.runtime?.capture?.state;
  if (runtimeState === 'running' || runtimeState === 'stopped' || runtimeState === 'disabled') {
    return runtimeState;
  }
  return stream?.status?.state === 'disabled' ? 'disabled' : 'running';
}

export function streamHealthStatus(stream: StreamInfo | null | undefined): StreamHealthStatus {
  const captureState = streamCaptureState(stream);
  if (captureState === 'disabled') return 'degraded';
  if (captureState === 'stopped') return 'idle';
  return 'live';
}

export function streamRecordingActive(stream: StreamInfo | null | undefined): boolean {
  const runtimeState = stream?.runtime?.recording?.state;
  if (runtimeState === 'active') return true;
  if (runtimeState === 'inactive') return false;
  return Boolean(stream?.status?.recording_active);
}

export function streamRecordingSinceMs(stream: StreamInfo | null | undefined): number | null {
  const runtimeValue = stream?.runtime?.recording?.started_at_ms;
  if (typeof runtimeValue === 'number' && Number.isFinite(runtimeValue)) {
    return Math.trunc(runtimeValue);
  }
  const fallbackValue = stream?.status?.recording_since_ms;
  if (typeof fallbackValue === 'number' && Number.isFinite(fallbackValue)) {
    return Math.trunc(fallbackValue);
  }
  return null;
}

export function streamRuntimeStatusLabel(stream: StreamInfo | null | undefined): string {
  if (streamRecordingActive(stream)) return 'recording';
  return streamCaptureState(stream);
}
