import type { StreamInfo } from '$lib/ts-bindings/http/client';

export type StreamPreviewFormat = 'mjpeg' | 'h264' | 'h265' | 'unknown';

export function normalizeStreamPreviewFormat(raw: unknown): StreamPreviewFormat {
  switch (String(raw ?? '').trim().toLowerCase()) {
    case 'mjpeg':
      return 'mjpeg';
    case 'h264':
      return 'h264';
    case 'h265':
      return 'h265';
    default:
      return 'unknown';
  }
}

export function streamPreviewFormatFromStreamInfo(stream: StreamInfo | null | undefined): StreamPreviewFormat {
  return normalizeStreamPreviewFormat(stream?.preview_format);
}

export function streamPreviewFormatFromPeerSummary(stream: { previewFormat?: unknown } | null | undefined): StreamPreviewFormat {
  return normalizeStreamPreviewFormat(stream?.previewFormat);
}
